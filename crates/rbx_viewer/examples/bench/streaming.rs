//! The two edits a streaming loader exists for: one that names a `MeshId` and
//! one that names a `TextureID` this session has never decoded.
//!
//! Both are the case that used to end in `Ok(false)` and a full reload with a
//! download in front of it. Each is reported twice — what the edit itself cost
//! (which is what somebody typing waits for) and how long the asset then took
//! to reach a frame (which is what they watch happen afterwards) — because the
//! whole point of the change is that those are two different numbers.
//!
//! # Staging an unseen asset without emptying the cache
//!
//! The assets of a fixture are all in the on-disk cache, or the run would be
//! measuring the network (see BENCHMARKS.md). So "unseen" is staged from the
//! other end: `Headless::forget_asset` drops one reference's decoded copy and
//! skips the next request for it, the reload after that rebuilds the place as
//! it stood before the asset was ever seen, and the edit that follows is a
//! genuine first sight of it — resolved from the disk cache, never downloaded.

use std::time::{Duration, Instant};

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::Headless;

use crate::args::Args;
use crate::measure::{drain_upload_budget, settle, wait_for_frame, Phase};
use crate::stats::Samples;

/// How long one requested asset is given to come back off the disk cache
/// before the run is called broken. Generous by two orders of magnitude: a
/// cached mesh decodes in single-digit milliseconds, and the only thing this
/// is really guarding against is a fetch that never lands at all.
const LIMIT: Duration = Duration::from_secs(20);

/// Which property a phase edits, and what its two rows are called.
struct Kind {
    property: &'static str,
    edit: &'static str,
    swap: &'static str,
}

const MESH: Kind = Kind {
    property: "MeshId",
    edit: "edit: new mesh",
    swap: "  mesh swapped",
};

const TEXTURE: Kind = Kind {
    property: "TextureID",
    edit: "edit: new texture",
    swap: "  texture swapped",
};

/// Both phases for both properties, in the order they are reported.
pub(crate) fn phases(
    path: &std::path::Path,
    dom: &mut WeakDom,
    args: &Args,
    notes: &mut Vec<String>,
) -> Result<Vec<Phase>, String> {
    let mut phases = Vec::new();
    for kind in [MESH, TEXTURE] {
        match unseen(path, dom, args, &kind)? {
            Some((edit, swap)) => {
                phases.push(edit);
                phases.push(swap);
            }
            None => notes.push(format!(
                "{} skipped: this place has no MeshPart with two distinct {} values to swap between",
                kind.edit, kind.property
            )),
        }
    }
    Ok(phases)
}

fn unseen(
    path: &std::path::Path,
    dom: &mut WeakDom,
    args: &Args,
    kind: &Kind,
) -> Result<Option<(Phase, Phase)>, String> {
    if !args.textures {
        return Ok(None);
    }
    let Some((referent, resident, unseen)) = candidate(dom, kind.property) else {
        return Ok(None);
    };

    let mut headless = Headless::load(path, args.textures)?;
    settle(&mut headless, args)?;

    let mut call = Vec::with_capacity(args.patch_iters);
    let mut frame = Vec::with_capacity(args.patch_iters);
    let mut swap_call = Vec::with_capacity(args.patch_iters);
    let mut swap_frame = Vec::with_capacity(args.patch_iters);

    for _ in 0..args.patch_iters {
        // Back to the value the place opened with, then forget the other one
        // and rebuild around its absence. Neither step is measured.
        set(dom, referent, kind.property, &resident)?;
        headless.forget_asset(&unseen);
        headless.reload(dom)?;
        drain_upload_budget(&mut headless, args)?;

        set(dom, referent, kind.property, &unseen)?;
        let started = Instant::now();
        if !headless.patch_instance(dom, referent)? {
            return Err(format!(
                "the {} edit stopped being patched in place part-way through the run",
                kind.property
            ));
        }
        call.push(started.elapsed());
        wait_for_frame(&mut headless, args.size)?;
        frame.push(started.elapsed());

        swap_call.push(swap(&mut headless)?);
        wait_for_frame(&mut headless, args.size)?;
        swap_frame.push(started.elapsed());
    }

    // Left as it was found: the phases after this share the DOM.
    set(dom, referent, kind.property, &resident)?;

    Ok(Some((
        Phase {
            name: kind.edit,
            call: Samples::new(&call),
            frame: Samples::new(&frame),
        },
        Phase {
            name: kind.swap,
            call: Samples::new(&swap_call),
            frame: Samples::new(&swap_frame),
        },
    )))
}

/// Spins on the swap-in until the requested asset has been folded into the
/// picture, and reports what that one fold cost.
///
/// Spinning rather than ticking: `Headless::tick` would advance the orbit
/// camera, and a frame drawn from somewhere else is not the frame this is
/// timing.
fn swap(headless: &mut Headless) -> Result<Duration, String> {
    let waited = Instant::now();
    loop {
        let entered = Instant::now();
        if headless.swap_assets() {
            return Ok(entered.elapsed());
        }
        if waited.elapsed() > LIMIT {
            return Err("the requested asset never landed".to_string());
        }
    }
}

/// A `MeshPart` in `Workspace` with a non-empty `property`, together with a
/// second, different value for that property found elsewhere in the place.
///
/// Two values from the place itself rather than one invented: an id nothing
/// else references would not be in the on-disk cache, and the fetch would be a
/// network round trip rather than the decode this is about.
fn candidate(dom: &WeakDom, property: &str) -> Option<(Ref, String, String)> {
    let mut found: Vec<(Ref, String)> = Vec::new();
    let mut stack: Vec<Ref> = dom
        .root_refs()
        .iter()
        .copied()
        .filter(|referent| {
            dom.get(*referent)
                .is_some_and(|instance| instance.class() == "Workspace")
        })
        .collect();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        stack.extend_from_slice(instance.children());
        if instance.class() != "MeshPart" {
            continue;
        }
        let Some(Variant::String(value)) = instance.properties().get(property) else {
            continue;
        };
        if !value.is_empty() {
            found.push((referent, value.clone()));
        }
    }

    let (referent, resident) = found.first().cloned()?;
    let unseen = found
        .iter()
        .map(|(_, value)| value)
        .find(|value| **value != resident)?
        .clone();
    Some((referent, resident, unseen))
}

fn set(dom: &mut WeakDom, referent: Ref, property: &str, value: &str) -> Result<(), String> {
    dom.set_property(referent, property, Variant::String(value.to_string()))
        .map(|_| ())
        .map_err(|err| format!("failed to set {property}: {err}"))
}
