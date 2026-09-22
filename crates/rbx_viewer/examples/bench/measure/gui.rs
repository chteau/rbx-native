//! A step of a UI editor canvas drag: one `GuiObject`'s `Position` nudged,
//! handed over as the editor hands it (a snapshot mirrored into the render
//! thread's DOM), patched through `Headless::apply_changes`, and — for the
//! frame time — the canvas drawn and read back with `Headless::render_gui`,
//! which is what the canvas waits on before it shows the step.
//!
//! The element moved is in the place's largest `ScreenGui`: the
//! re-plan a GUI edit costs grows with the tree it is in, and the largest
//! is the one a drag is slowest in.

use std::path::Path;
use std::time::Instant;

use rbx_dom::{Ref, UDim, UDim2, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::{Applied, Headless};

use super::{drain_upload_budget, walk, Phase};
use crate::args::Args;
use crate::stats::Samples;

/// The screen the canvas simulates, as the editor's default preset.
const SCREEN: (u32, u32) = (1920, 1080);

pub(super) fn drag(path: &Path, dom: &mut WeakDom, args: &Args) -> Result<Option<Phase>, String> {
    let database = ReflectionDatabase::embedded();
    let Some((screen, element, mut position)) = target(dom, &database) else {
        return Ok(None);
    };
    let mut headless = Headless::load(path, args.textures)?;
    drain_upload_budget(&mut headless, args)?;
    // The first canvas pays for its target and pipelines.
    headless.render_gui(screen, SCREEN, [0.0; 3])?;
    let mut mirror = dom.clone();

    let mut call = Vec::with_capacity(args.patch_iters);
    let mut frame = Vec::with_capacity(args.patch_iters);
    for step in 0..args.patch_iters {
        // Back and forth, so the element stays where the place had it.
        position.x.offset += if step % 2 == 0 { 1 } else { -1 };
        dom.set_property(element, "Position", Variant::UDim2(position))
            .map_err(|err| format!("failed to move the element being dragged: {err}"))?;
        let log = dom.take_changes();

        let started = Instant::now();
        mirror.mirror(dom.snapshot(&log));
        let applied = headless.apply_changes(&mirror, &log)?;
        call.push(started.elapsed());
        if let Applied::Rebuilt(why) = applied {
            return Err(format!("a GUI edit fell back to a rebuild: {why}"));
        }
        headless.render_gui(screen, SCREEN, [0.0; 3])?;
        frame.push(started.elapsed());
    }

    Ok(Some(Phase {
        name: "canvas drag step",
        call: Samples::new(&call),
        frame: Samples::new(&frame),
    }))
}

/// The largest `ScreenGui`, a `GuiObject` in it, and that element's
/// `Position`.
fn target(dom: &WeakDom, database: &ReflectionDatabase) -> Option<(Ref, Ref, UDim2)> {
    let screen = walk(dom, dom.root_refs())
        .into_iter()
        .filter(|&r| dom.get(r).is_some_and(|i| i.class() == "ScreenGui"))
        .max_by_key(|&screen| walk(dom, &[screen]).len())?;
    walk(dom, &[screen]).into_iter().find_map(|child| {
        let instance = dom.get(child)?;
        if !database.is_subclass_of(instance.class(), "GuiObject") {
            return None;
        }
        let position = match instance.properties().get("Position") {
            Some(Variant::UDim2(position)) => *position,
            _ => UDim2 {
                x: UDim {
                    scale: 0.0,
                    offset: 0,
                },
                y: UDim {
                    scale: 0.0,
                    offset: 0,
                },
            },
        };
        Some((screen, child, position))
    })
}
