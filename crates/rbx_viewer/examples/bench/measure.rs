//! The measurements, taken through `Headless`'s public API and nothing else,
//! so the same harness runs unchanged against a branch that rewrites what is
//! underneath it: a cold load, a full reload, the edits in [`edits`], the
//! steady-state frame, and — in `streaming` — the two edits that name an
//! asset nobody has fetched.

mod edits;
mod gui;

use std::path::Path;
use std::time::Instant;

use rbx_dom::{CFrameData, Change, Ref, Variant, WeakDom};
use rbx_viewer::{Applied, Headless, QualityLevel};

use crate::args::Args;
use crate::stats::Samples;

/// How far a patched part is nudged per iteration, in studs.
///
/// Small enough to keep the part on screen for the whole run, large enough that
/// no float rounding can turn the write into a no-op the renderer might skip.
const NUDGE: f32 = 0.05;

/// How long a place is given to finish streaming its assets in before the run
/// is called broken. A cold `marked.rbxl` off a warm disk cache is a second or
/// two; anything near this is a fetch that never lands.
const SETTLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

/// One timed operation, reported at both ends of the GPU pipeline.
pub(crate) struct Phase {
    pub(crate) name: &'static str,
    /// Wall clock around the `Headless` call alone. The call queues GPU work
    /// and returns without waiting for it, so this is a floor on the cost, not
    /// the cost.
    pub(crate) call: Samples,
    /// The same operation measured from the same instant through to the first
    /// frame whose pixels are back in system memory — what someone waiting for
    /// the viewport to redraw actually waits for.
    pub(crate) frame: Samples,
}

/// Steady-state frame cost at one quality level.
pub(crate) struct Frames {
    pub(crate) level: u8,
    /// Wall clock around `render_frame`, which queues frame N and hands back
    /// frame N-1: with the pipeline full that is exactly the per-frame cost.
    pub(crate) wall: Samples,
    /// The renderer's own split of that, as `Rendered` reports it. `render`
    /// only covers recording and submitting the draw, so the GPU's actual work
    /// lands in `readback`, where it is waited for.
    pub(crate) render: Samples,
    pub(crate) readback: Samples,
}

pub(crate) struct Measured {
    pub(crate) instances: usize,
    pub(crate) phases: Vec<Phase>,
    pub(crate) frames: Vec<Frames>,
    /// Why a phase is missing, for a place that offered nothing to measure it
    /// with — an empty list means everything asked for was measured.
    pub(crate) notes: Vec<String>,
}

pub(crate) fn fixture(path: &Path, args: &Args) -> Result<Measured, String> {
    let mut dom = rbx_viewer::read_place(path)?;
    let instances = walk(&dom, dom.root_refs()).len();

    let mut phases = vec![
        cold_load(path, args)?,
        loaded(path, args)?,
        reload(path, &dom, args)?,
    ];
    let mut notes = Vec::new();
    // `&mut` from here on: the patch and edit phases move parts around in
    // this very DOM (and put them back), and every phase that shares it
    // read-only has already run.
    match edits::patch(path, &mut dom, args)? {
        Some(phase) => phases.push(phase),
        None => notes.push(
            "single-instance patch skipped: no BasePart in this place is patched in place"
                .to_string(),
        ),
    }
    match gui::drag(path, &mut dom, args)? {
        Some(phase) => phases.push(phase),
        None => notes.push("canvas drag skipped: no ScreenGui holds a GuiObject".to_string()),
    }
    match edits::phases(path, &mut dom, args)? {
        Some(edited) => phases.extend(edited),
        None => notes
            .push("edit phases skipped: no BasePart in this place is patched in place".to_string()),
    }
    phases.extend(crate::streaming::phases(path, &mut dom, args, &mut notes)?);
    match edits::unions(path, &mut dom, args)? {
        Some(edited) => phases.extend(edited),
        None => notes.push(
            "union edit phases skipped: this place draws no union as its recovered \
             fallback pieces"
                .to_string(),
        ),
    }

    Ok(Measured {
        instances,
        phases,
        frames: frames(path, args)?,
        notes,
    })
}

/// `Headless::load`: parsing the file, building the scene and uploading it to
/// a GPU device opened for the occasion.
///
/// The assets are *not* in this number. The load asks for them and returns;
/// they arrive on the ticks after it, and [`loaded`] is what times the place
/// being finished rather than drawable.
fn cold_load(path: &Path, args: &Args) -> Result<Phase, String> {
    // One load thrown away first. It pays for the OS page cache on the place
    // file and for the asset cache's own cold reads, neither of which anything
    // measured here should be charged for a second time.
    drop(Headless::load(path, args.textures)?);

    let mut call = Vec::with_capacity(args.load_iters);
    let mut frame = Vec::with_capacity(args.load_iters);
    for _ in 0..args.load_iters {
        let started = Instant::now();
        let mut headless = Headless::load(path, args.textures)?;
        call.push(started.elapsed());
        wait_for_frame(&mut headless, args.size)?;
        frame.push(started.elapsed());
    }

    Ok(Phase {
        name: "cold load",
        call: Samples::new(&call),
        frame: Samples::new(&frame),
    })
}

/// `Headless::load` through to the frame after the last of the place's assets
/// has landed and been swapped in — the picture somebody opening a file
/// actually waits for, as against the first drawable frame [`cold_load`]
/// times.
fn loaded(path: &Path, args: &Args) -> Result<Phase, String> {
    drop(Headless::load(path, args.textures)?);

    let mut call = Vec::with_capacity(args.load_iters);
    let mut frame = Vec::with_capacity(args.load_iters);
    for _ in 0..args.load_iters {
        let started = Instant::now();
        let mut headless = Headless::load(path, args.textures)?;
        settle(&mut headless, args)?;
        call.push(started.elapsed());
        wait_for_frame(&mut headless, args.size)?;
        frame.push(started.elapsed());
    }

    Ok(Phase {
        name: "load complete",
        call: Samples::new(&call),
        frame: Samples::new(&frame),
    })
}

/// `Headless::reload`: the whole scene rebuilt from a DOM already in memory,
/// which is what an editor pays for every script or command-bar edit.
fn reload(path: &Path, dom: &WeakDom, args: &Args) -> Result<Phase, String> {
    let mut headless = Headless::load(path, args.textures)?;
    // Measured against a place whose assets are all in, the way it was before
    // loading streamed: a reload part-way through one would be timing the
    // stream, not the reload.
    settle(&mut headless, args)?;
    // The first reload in a process still pays for pipeline caches the cold
    // load left cold, so it is warmup rather than a sample.
    headless.reload(dom)?;
    wait_for_frame(&mut headless, args.size)?;

    let mut call = Vec::with_capacity(args.reload_iters);
    let mut frame = Vec::with_capacity(args.reload_iters);
    for _ in 0..args.reload_iters {
        let started = Instant::now();
        headless.reload(dom)?;
        call.push(started.elapsed());
        wait_for_frame(&mut headless, args.size)?;
        frame.push(started.elapsed());
    }

    Ok(Phase {
        name: "full reload",
        call: Samples::new(&call),
        frame: Samples::new(&frame),
    })
}

/// `render_frame` to `take_frame` with the view held still, at each quality
/// level asked for.
fn frames(path: &Path, args: &Args) -> Result<Vec<Frames>, String> {
    let mut headless = Headless::load(path, args.textures)?;
    // A steady-state frame is one with the place's assets in it, not one drawn
    // while they are still arriving.
    settle(&mut headless, args)?;
    let mut measured = Vec::with_capacity(args.levels.len());

    for &level in &args.levels {
        headless.set_quality(QualityLevel::Level(level));
        // The camera is deliberately never ticked: a number that also measured
        // the controller would not be the frame cost it claims to be.
        for _ in 0..args.frame_warmup {
            headless.render_frame(args.size.0, args.size.1)?;
        }

        let mut wall = Vec::with_capacity(args.frame_iters);
        let mut render = Vec::with_capacity(args.frame_iters);
        let mut readback = Vec::with_capacity(args.frame_iters);
        for _ in 0..args.frame_iters {
            let started = Instant::now();
            let rendered = headless.render_frame(args.size.0, args.size.1)?;
            let elapsed = started.elapsed();
            // `None` only before the pipeline is full, which the warmup above
            // has already seen to — `--frame-warmup` will not take a zero.
            if let Some(rendered) = rendered {
                wall.push(elapsed);
                render.push(rendered.render);
                readback.push(rendered.readback);
            }
        }
        // Nothing may stay in flight across a quality change: a readback buffer
        // left mapped is one the next copy into it cannot use.
        headless.take_frame()?;

        measured.push(Frames {
            level,
            wall: Samples::new(&wall),
            render: Samples::new(&render),
            readback: Samples::new(&readback),
        });
    }

    Ok(measured)
}

/// Draws frames until the renderer has stopped finishing the load.
///
/// `Renderer::draw` uploads only part of a freshly loaded place's textures per
/// frame and spreads the rest over the frames after it, so a measurement taken
/// against a renderer still working through that backlog charges the operation
/// for the load. It is measurable: on the 16k-instance fixture the first dozen
/// or so frames after a load cost six to thirty times what every frame after
/// them costs. Only the phases that deliberately measure *the first* frame
/// (a cold load, a reload) skip this.
pub(crate) fn drain_upload_budget(headless: &mut Headless, args: &Args) -> Result<(), String> {
    for _ in 0..args.frame_warmup {
        headless.render_frame(args.size.0, args.size.1)?;
    }
    headless.take_frame()?;
    Ok(())
}

/// Waits until every asset the place asked for has landed and been folded into
/// the picture, then drains the texture-upload backlog that leaves behind.
///
/// `Headless::load` now returns with the scene drawable and its assets still
/// arriving, so every phase that means to measure a *finished* place has to say
/// so. Spun rather than ticked, for the same reason `streaming::swap` spins:
/// `Headless::tick` would fly the orbit camera, and a number that also measured
/// the controller would not be the one it claims to be.
pub(crate) fn settle(headless: &mut Headless, args: &Args) -> Result<(), String> {
    let waited = Instant::now();
    loop {
        let swapped = headless.swap_assets();
        if headless.assets_in_flight() == 0 && !swapped {
            break;
        }
        if waited.elapsed() > SETTLE_LIMIT {
            return Err("the place never finished loading its assets".to_string());
        }
    }
    drain_upload_budget(headless, args)
}

/// Draws one frame and waits until its pixels are in system memory.
///
/// `render_frame` only *queues* the draw and hands back the frame before it, so
/// nothing may be in flight when this is called — that is what makes the
/// following `take_frame` wait on the frame just queued and on nothing else.
/// Every phase here upholds that by draining through this same function.
pub(crate) fn wait_for_frame(headless: &mut Headless, size: (u32, u32)) -> Result<(), String> {
    if headless.render_frame(size.0, size.1)?.is_some() {
        return Err("a frame was still in flight when the measurement started".to_string());
    }
    headless
        .take_frame()?
        .ok_or_else(|| "the frame queued after the edit never came back".to_string())?;
    Ok(())
}

/// Finds up to `count` `BasePart`s the renderer really does patch in place,
/// each with its `CFrame` as it stands.
///
/// Tries candidates instead of trusting the first: `apply_changes` rebuilds
/// for anything the scene cannot patch as one part — a union drawn as its
/// fallback pieces, a material never uploaded — and timing that rebuild as
/// though it were a patch would report a lie. The trial is a `CFrame` write
/// the DOM has not actually changed for, so it re-reads the same placement.
fn patchables(
    headless: &mut Headless,
    dom: &WeakDom,
    count: usize,
) -> Result<Vec<(Ref, CFrameData)>, String> {
    let mut found = Vec::with_capacity(count);
    for referent in parts(dom) {
        if found.len() == count {
            break;
        }
        let Some(Variant::CFrame(cframe)) = dom
            .get(referent)
            .and_then(|instance| instance.properties().get("CFrame"))
        else {
            continue;
        };
        let trial = [Change::Property {
            referent,
            name: "CFrame".to_string(),
        }];
        if headless.apply_changes(dom, &trial)? == Applied::Patched {
            found.push((referent, *cframe));
        }
    }
    Ok(found)
}

/// Candidate parts, `Workspace` first: the scene is built from that subtree
/// alone, so a part anywhere else is guaranteed not to patch.
fn parts(dom: &WeakDom) -> Vec<Ref> {
    let workspace: Vec<Ref> = dom
        .root_refs()
        .iter()
        .copied()
        .filter(|referent| {
            dom.get(*referent)
                .is_some_and(|instance| instance.class() == "Workspace")
        })
        .collect();
    let roots = if workspace.is_empty() {
        dom.root_refs().to_vec()
    } else {
        workspace
    };

    walk(dom, &roots)
        .into_iter()
        .filter(|referent| {
            matches!(
                dom.get(*referent)
                    .and_then(|i| i.properties().get("CFrame")),
                Some(Variant::CFrame(_))
            )
        })
        .collect()
}

/// Every referent reachable from `roots`, including the roots themselves.
fn walk(dom: &WeakDom, roots: &[Ref]) -> Vec<Ref> {
    let mut stack = roots.to_vec();
    let mut seen = Vec::new();
    while let Some(referent) = stack.pop() {
        seen.push(referent);
        if let Some(instance) = dom.get(referent) {
            stack.extend_from_slice(instance.children());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::{parts, walk};
    use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};

    fn place() -> WeakDom {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let lighting = dom.new_instance("Lighting", "Lighting", None);
        let part = dom.new_instance("Part", "Part", Some(workspace));
        // A `CFrame` outside Workspace: never built into the scene, so never a
        // patch candidate however much it looks like one.
        let decoy = dom.new_instance("Part", "Decoy", Some(lighting));
        for referent in [part, decoy] {
            dom.set_property(
                referent,
                "CFrame",
                Variant::CFrame(CFrameData {
                    position: Vector3Data {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                }),
            )
            .expect("the instance was just created");
        }
        dom
    }

    #[test]
    fn candidates_come_from_workspace_alone() {
        let dom = place();
        let found = parts(&dom);
        assert_eq!(found.len(), 1);
        assert_eq!(
            dom.get(found[0]).expect("still in the dom").name(),
            "Part",
            "a CFrame under Lighting is not something the scene ever built"
        );
    }

    #[test]
    fn walking_counts_every_instance_in_the_tree() {
        let dom = place();
        assert_eq!(walk(&dom, dom.root_refs()).len(), 4);
    }

    #[test]
    fn a_place_without_a_workspace_still_offers_candidates() {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        dom.set_property(
            model,
            "CFrame",
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        )
        .expect("the instance was just created");
        assert_eq!(parts(&dom), vec![model]);
    }
}
