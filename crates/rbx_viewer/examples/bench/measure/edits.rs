//! The edits: one part's property write, an insert, a delete, a hundred
//! parts moved at once, a legacy union drawn as its recovered fallback
//! pieces moved and recoloured, and the undo of each — all through
//! `Headless::apply_changes`, exactly as the editor reflects them, with the
//! undo handing over the *mutation's* own change log against the restored
//! DOM the way `rbxstudio`'s history does.
//!
//! Every phase asserts `Applied::Patched`: a number taken while the viewer
//! quietly fell back to a rebuild would be a reload timing wearing the wrong
//! label.

use std::path::Path;
use std::time::Instant;

use rbx_dom::{CFrameData, Change, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_viewer::{Applied, Headless};

use super::{drain_upload_budget, patchables, wait_for_frame, walk, Phase, NUDGE};
use crate::args::Args;
use crate::stats::Samples;

/// How many parts the batch move carries — a script's `for _, part in
/// workspace:GetChildren()` on a small model, and enough to tell one
/// coalesced upload from a hundred separate ones.
const BATCH: usize = 100;

/// `Headless::apply_changes` after one `BasePart`'s `CFrame` moves — the
/// Properties-panel edit, as the one-write change log it produces.
pub(super) fn patch(path: &Path, dom: &mut WeakDom, args: &Args) -> Result<Option<Phase>, String> {
    let mut headless = Headless::load(path, args.textures)?;
    // The target, its pipelines and the freshly loaded place's textures must
    // all be on the GPU already, or the first patches would be charged for
    // finishing the load — see `drain_upload_budget`.
    drain_upload_budget(&mut headless, args)?;

    let Some(&(referent, cframe)) = patchables(&mut headless, dom, 1)?.first() else {
        return Ok(None);
    };
    let mut cframe = cframe;
    // `patchables`' trial patch uploaded an instance without drawing it.
    wait_for_frame(&mut headless, args.size)?;

    let mut call = Vec::with_capacity(args.patch_iters);
    let mut frame = Vec::with_capacity(args.patch_iters);
    for _ in 0..args.patch_iters {
        cframe.position.y += NUDGE;
        dom.set_property(referent, "CFrame", Variant::CFrame(cframe))
            .map_err(|err| format!("failed to move the part being patched: {err}"))?;
        let log = dom.take_changes();

        let started = Instant::now();
        let applied = headless.apply_changes(dom, &log)?;
        call.push(started.elapsed());
        if let Applied::Rebuilt(why) = applied {
            return Err(format!(
                "the part stopped being patchable part-way through the run: {why}"
            ));
        }
        wait_for_frame(&mut headless, args.size)?;
        frame.push(started.elapsed());
    }

    Ok(Some(Phase {
        name: "patch instance",
        call: Samples::new(&call),
        frame: Samples::new(&frame),
    }))
}

/// Measures the six structural edit phases against `dom`, which is left as
/// it was found. `None` for a place with no part the viewer patches in
/// place.
pub(super) fn phases(
    path: &Path,
    dom: &mut WeakDom,
    args: &Args,
) -> Result<Option<Vec<Phase>>, String> {
    let mut headless = Headless::load(path, args.textures)?;
    drain_upload_budget(&mut headless, args)?;
    let parts = patchables(&mut headless, dom, BATCH)?;
    let Some(&(anchor, frame)) = parts.first() else {
        return Ok(None);
    };
    let Some(workspace) = dom.parent(anchor) else {
        return Ok(None);
    };
    // `patchables`' trial patches uploaded instances without drawing them.
    wait_for_frame(&mut headless, args.size)?;

    let (insert, undo_insert) = insert(&mut headless, dom, args, workspace, frame)?;
    let (delete, undo_delete) = delete(&mut headless, dom, args, anchor)?;
    let (moved, undo_move) = batch_move(&mut headless, dom, args, &parts)?;
    Ok(Some(vec![
        insert,
        undo_insert,
        delete,
        undo_delete,
        moved,
        undo_move,
    ]))
}

/// A `Part` inserted under `Workspace` with the defaults `rbxstudio`'s
/// quick-insert sets (see `shell::keys`), next to a part known to be on
/// screen; then taken out again with the insert's own log.
fn insert(
    headless: &mut Headless,
    dom: &mut WeakDom,
    args: &Args,
    workspace: Ref,
    beside: CFrameData,
) -> Result<(Phase, Phase), String> {
    let mut forward = Timings::new(args.patch_iters);
    let mut backward = Timings::new(args.patch_iters);
    for _ in 0..args.patch_iters {
        let part = dom.new_instance("Part", "Part", Some(workspace));
        let mut frame = beside;
        frame.position.y += 2.0;
        for (name, value) in [
            (
                "size",
                Variant::Vector3(Vector3Data {
                    x: 4.0,
                    y: 1.2,
                    z: 2.0,
                }),
            ),
            ("CFrame", Variant::CFrame(frame)),
            (
                "Color3uint8",
                Variant::Color3uint8 {
                    r: 163,
                    g: 162,
                    b: 165,
                },
            ),
            ("Transparency", Variant::Float32(0.0)),
            ("CastShadow", Variant::Bool(true)),
        ] {
            dom.set_property(part, name, value)
                .map_err(|err| format!("failed to set up the inserted part: {err}"))?;
        }
        let log = dom.take_changes();
        forward.measure(headless, dom, &log, args, "insert")?;

        dom.remove(part);
        dom.take_changes();
        backward.measure(headless, dom, &log, args, "undo of insert")?;
    }
    Ok((forward.phase("insert part"), backward.phase("undo insert")))
}

/// `part`'s whole subtree removed, as the Explorer's Delete does; then put
/// back — every instance re-inserted under its old parent — and reflected
/// with the delete's own log.
fn delete(
    headless: &mut Headless,
    dom: &mut WeakDom,
    args: &Args,
    part: Ref,
) -> Result<(Phase, Phase), String> {
    let mut forward = Timings::new(args.patch_iters);
    let mut backward = Timings::new(args.patch_iters);
    for _ in 0..args.patch_iters {
        let subtree: Vec<(Instance, Option<Ref>)> = walk(dom, &[part])
            .into_iter()
            .filter_map(|referent| Some((dom.get(referent)?.clone(), dom.parent(referent))))
            .collect();
        dom.remove(part);
        let log = dom.take_changes();
        forward.measure(headless, dom, &log, args, "delete")?;

        // Parents first: `walk` is pre-order from `part`, so each instance's
        // parent is either outside the subtree or already back in.
        for (instance, parent) in &subtree {
            let referent = instance.referent();
            dom.insert(instance.clone());
            dom.set_parent(referent, *parent);
        }
        dom.take_changes();
        backward.measure(headless, dom, &log, args, "undo of delete")?;
    }
    Ok((forward.phase("delete part"), backward.phase("undo delete")))
}

/// Every part in `parts` nudged in one go — what a script moving a model
/// writes, or a group drag's one step — then moved back with the same log.
fn batch_move(
    headless: &mut Headless,
    dom: &mut WeakDom,
    args: &Args,
    parts: &[(Ref, CFrameData)],
) -> Result<(Phase, Phase), String> {
    let mut forward = Timings::new(args.patch_iters);
    let mut backward = Timings::new(args.patch_iters);
    let mut frames: Vec<(Ref, CFrameData)> = parts.to_vec();
    for _ in 0..args.patch_iters {
        for (referent, frame) in &mut frames {
            frame.position.y += NUDGE;
            dom.set_property(*referent, "CFrame", Variant::CFrame(*frame))
                .map_err(|err| format!("failed to move a batched part: {err}"))?;
        }
        let log = dom.take_changes();
        forward.measure(headless, dom, &log, args, "batch move")?;

        for (referent, frame) in &mut frames {
            frame.position.y -= NUDGE;
            dom.set_property(*referent, "CFrame", Variant::CFrame(*frame))
                .map_err(|err| format!("failed to move a batched part back: {err}"))?;
        }
        dom.take_changes();
        backward.measure(headless, dom, &log, args, "undo of batch move")?;
    }
    let count = parts.len();
    Ok((
        forward.phase(if count == BATCH {
            "move 100 parts"
        } else {
            "move n parts"
        }),
        backward.phase(if count == BATCH {
            "undo move 100"
        } else {
            "undo move n"
        }),
    ))
}

/// The union edits: a legacy `UnionOperation` drawn as the fallback pieces
/// recovered from its operation tree, moved and recoloured, with the undo of
/// each. Kept apart from [`phases`] because a place has to *have* such a
/// union for any of it to mean anything — one whose boolean succeeded draws
/// as a single computed mesh and is an ordinary mesh-instance patch, already
/// covered by `patch instance`.
///
/// `None` for a place that draws no union as its pieces, which the caller
/// reports as a skip rather than quietly measuring something else.
pub(super) fn unions(
    path: &Path,
    dom: &mut WeakDom,
    args: &Args,
) -> Result<Option<Vec<Phase>>, String> {
    let mut headless = Headless::load(path, args.textures)?;
    drain_upload_budget(&mut headless, args)?;
    let Some(union) = fallback_union(&headless, dom) else {
        return Ok(None);
    };

    let (moved, undo_moved) = union_move(&mut headless, dom, args, union)?;
    let (painted, undo_painted) = union_colour(&mut headless, dom, args, union)?;
    Ok(Some(vec![moved, undo_moved, painted, undo_painted]))
}

/// The first instance the viewer draws as several recovered pieces.
///
/// Asked of the renderer rather than guessed from the class: whether a given
/// `UnionOperation` is drawn as one computed mesh or as its pieces depends on
/// what its asset carved to, and a place usually has both. The whole tree is
/// searched, unlike `patchables`' capped candidate list — a place's unions
/// are wherever the builder put them, and this costs one walk outside
/// everything measured.
fn fallback_union(headless: &Headless, dom: &WeakDom) -> Option<Ref> {
    walk(dom, dom.root_refs())
        .into_iter()
        .find(|&referent| headless.fallback_pieces(referent) > 0)
}

/// The union nudged, then put back with the move's own log — the drag, and
/// the undo of it.
fn union_move(
    headless: &mut Headless,
    dom: &mut WeakDom,
    args: &Args,
    union: Ref,
) -> Result<(Phase, Phase), String> {
    let mut frame = cframe_of(dom, union)?;
    let mut forward = Timings::new(args.patch_iters);
    let mut backward = Timings::new(args.patch_iters);
    for _ in 0..args.patch_iters {
        frame.position.y += NUDGE;
        set(dom, union, "CFrame", Variant::CFrame(frame))?;
        let log = dom.take_changes();
        forward.measure(headless, dom, &log, args, "union move")?;

        frame.position.y -= NUDGE;
        set(dom, union, "CFrame", Variant::CFrame(frame))?;
        dom.take_changes();
        backward.measure(headless, dom, &log, args, "undo of a union move")?;
    }
    Ok((
        forward.phase("move union"),
        backward.phase("undo move union"),
    ))
}

/// The union's own `Color3uint8` written, then put back with the write's own
/// log. A union drawn as its pieces does not repaint them — they keep the
/// colours of the parts they were recovered from, which is what a rebuild
/// draws too — so this measures the edit, not a change of picture: the point
/// is that a Properties row nobody can see the effect of still must not cost
/// a reload.
fn union_colour(
    headless: &mut Headless,
    dom: &mut WeakDom,
    args: &Args,
    union: Ref,
) -> Result<(Phase, Phase), String> {
    let before = colour_of(dom, union);
    let mut forward = Timings::new(args.patch_iters);
    let mut backward = Timings::new(args.patch_iters);
    for _ in 0..args.patch_iters {
        set(
            dom,
            union,
            "Color3uint8",
            Variant::Color3uint8 {
                r: 200,
                g: 40,
                b: 40,
            },
        )?;
        let log = dom.take_changes();
        forward.measure(headless, dom, &log, args, "union recolour")?;

        set(dom, union, "Color3uint8", before.clone())?;
        dom.take_changes();
        backward.measure(headless, dom, &log, args, "undo of a union recolour")?;
    }
    Ok((
        forward.phase("recolour union"),
        backward.phase("undo recolour"),
    ))
}

fn cframe_of(dom: &WeakDom, referent: Ref) -> Result<CFrameData, String> {
    match dom
        .get(referent)
        .and_then(|instance| instance.properties().get("CFrame"))
    {
        Some(Variant::CFrame(frame)) => Ok(*frame),
        _ => Err("the union being measured has no CFrame".to_string()),
    }
}

/// The union's colour as it stands, or the studio default for one that has
/// never been painted — so the undo half writes the value the place had.
fn colour_of(dom: &WeakDom, referent: Ref) -> Variant {
    dom.get(referent)
        .and_then(|instance| instance.properties().get("Color3uint8"))
        .cloned()
        .unwrap_or(Variant::Color3uint8 {
            r: 163,
            g: 162,
            b: 165,
        })
}

fn set(dom: &mut WeakDom, referent: Ref, name: &str, value: Variant) -> Result<(), String> {
    dom.set_property(referent, name, value)
        .map(|_| ())
        .map_err(|err| format!("failed to write {name} on the union being measured: {err}"))
}

/// One phase's two sample sets, filled one iteration at a time.
struct Timings {
    call: Vec<std::time::Duration>,
    frame: Vec<std::time::Duration>,
}

impl Timings {
    fn new(iterations: usize) -> Self {
        Timings {
            call: Vec::with_capacity(iterations),
            frame: Vec::with_capacity(iterations),
        }
    }

    /// Times `apply_changes` and the first readable frame after it, the same
    /// discipline as every other phase (see `wait_for_frame`).
    fn measure(
        &mut self,
        headless: &mut Headless,
        dom: &WeakDom,
        log: &[Change],
        args: &Args,
        what: &str,
    ) -> Result<(), String> {
        let started = Instant::now();
        let applied = headless.apply_changes(dom, log)?;
        self.call.push(started.elapsed());
        if let Applied::Rebuilt(why) = applied {
            return Err(format!("the {what} was not patched in place: {why}"));
        }
        wait_for_frame(headless, args.size)?;
        self.frame.push(started.elapsed());
        Ok(())
    }

    fn phase(self, name: &'static str) -> Phase {
        Phase {
            name,
            call: Samples::new(&self.call),
            frame: Samples::new(&self.frame),
        }
    }
}
