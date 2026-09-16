use std::time::{Duration, Instant};

use super::*;
use crate::quality::QualityLevel;

// Nothing but their order links the WGSL struct to this layout, so a field
// added to one alone reads the neighbouring attribute instead of failing to
// compile.
#[test]
fn the_shader_reads_the_wedge_flag_the_layout_supplies() {
    assert!(DECAL_SHADER.contains("@location(11) wedge: u32"));
    assert_eq!(INSTANCE_ATTRIBUTES.len(), 10);
    assert_eq!(INSTANCE_ATTRIBUTES[9].shader_location, 11);
    assert_eq!(
        INSTANCE_ATTRIBUTES[9].offset + INSTANCE_ATTRIBUTES[9].format.size(),
        std::mem::size_of::<DecalRaw>() as wgpu::BufferAddress
    );
}

fn group(value: u8) -> Group {
    Group {
        reference: AssetRef::Id(u64::from(value)),
        image: Arc::new(Image {
            width: 1,
            height: 1,
            pixels: vec![value, value, value, 255],
        }),
        opaque: Vec::new(),
        blended: Vec::new(),
    }
}

/// A first build has nothing to take over: every group is its own deferred
/// upload, at its own slot.
#[test]
fn a_first_build_reuses_nothing() {
    let groups: Vec<Group> = (0..5).map(group).collect();

    assert_eq!(reuse_plan(&[], &[], &groups), vec![None; 5]);
}

/// The rebuild decision that keeps a scene edit from re-uploading every
/// decal in the place: an asset the previous scene already uploaded is
/// found by reference, wherever its slot was, and a group's slot is its
/// position in the new list — the same rule `upload_pending` lands deferred
/// uploads by.
#[test]
fn a_rebuild_takes_over_the_previous_upload_of_the_same_asset() {
    let previous = [AssetRef::Id(1), AssetRef::Id(2), AssetRef::Id(3)];
    // 3 moved up, 1 stayed, 2 left the scene and 9 joined it.
    let groups = [group(3), group(1), group(9)];

    assert_eq!(
        reuse_plan(&previous, &[], &groups),
        vec![Some(2), Some(0), None]
    );
}

/// A slot whose real image was still queued only ever held the 1x1
/// placeholder: taking it over would leave that asset drawn white forever,
/// since the queue entry that would have replaced it is gone with the old
/// scene. It has to be re-queued instead.
#[test]
fn an_upload_still_queued_is_not_taken_over() {
    let previous = [AssetRef::Id(1), AssetRef::Id(2)];
    let groups = [group(1), group(2)];

    assert_eq!(reuse_plan(&previous, &[1], &groups), vec![Some(0), None]);
}

/// `count` distinct, procedurally generated `side`x`side` images — no
/// network, no committed asset (see `agents/AGENTS.md`'s asset rules), just
/// plain RGBA bytes standing in for what `assets::load` would otherwise have
/// decoded from real `Decal`/`Texture` downloads. Each pixel differs by
/// group so a real, non-degenerate mip chain gets built and uploaded for
/// every one of them, the same as a real image would.
fn synthetic_groups(count: usize, side: u32) -> Vec<Group> {
    (0..count)
        .map(|index| Group {
            reference: AssetRef::Id(index as u64 + 1),
            image: Arc::new(Image {
                width: side,
                height: side,
                pixels: (0..side * side * 4)
                    .map(|byte| ((index as u32 + byte) % 251) as u8)
                    .collect(),
            }),
            opaque: Vec::new(),
            blended: Vec::new(),
        })
        .collect()
}

/// Waits for every GPU write queued so far, so the elapsed time measured
/// around it is the real upload cost rather than just the time it took the
/// CPU to enqueue it — `Queue::write_texture` returns as soon as the copy is
/// recorded, well before the GPU has actually moved the bytes.
fn wait_for_gpu(device: &wgpu::Device) {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("the GPU never caught up");
}

/// Manual profiling harness, not part of the regular gate: run with
/// `cargo test --release -p rbx_viewer --lib renderer::textured::tests::profile_texture_upload_spread -- --ignored --nocapture`
/// to see the shape of loading a place with many `Decal`/`Texture` images —
/// a burst before this module existed versus a bounded upload spread across
/// several `Renderer::draw` calls after. `upload_pending`'s budget argument
/// is what actually changes shape here: `usize::MAX` reproduces the old
/// `Textured::new` behaviour of uploading everything inline before the
/// renderer was usable at all (what `Renderer::finish_loading` now uses
/// deliberately for the single-shot `--screenshot` path — see its doc
/// comment), while `texture::PER_FRAME` is what every other call site gets
/// automatically through `Renderer::draw`.
#[test]
#[ignore = "manual profiling harness; needs a real GPU"]
fn profile_texture_upload_spread() {
    let instance = crate::gpu::instance();
    let adapter = crate::gpu::adapter(&instance, None).expect("a GPU adapter");
    let (device, queue) = crate::gpu::device(&adapter).expect("a GPU device");

    let view_projection = pipeline::frame_layout(&device);
    let target = Target {
        format: crate::renderer::post::HDR_FORMAT,
        samples: 1,
    };
    let quality = QualityLevel::Automatic.profile();
    // 60 images, 1024x1024 (full mip chain each): the same order of
    // magnitude the CSG profiling elsewhere in this branch used for its own
    // "many assets" case, and a plausible upper end for one place's worth of
    // decals.
    let groups = synthetic_groups(60, 1024);

    let mut before = Textured::new(&device, &queue, target, &view_projection, &groups, &quality);
    let burst = Instant::now();
    before.upload_pending(&device, &queue, &quality, usize::MAX);
    wait_for_gpu(&device);
    let burst = burst.elapsed();

    let mut after = Textured::new(&device, &queue, target, &view_projection, &groups, &quality);
    let mut frames: Vec<Duration> = Vec::new();
    while !after.pending.is_empty() {
        let frame = Instant::now();
        after.upload_pending(&device, &queue, &quality, texture::PER_FRAME);
        wait_for_gpu(&device);
        frames.push(frame.elapsed());
    }

    let longest = frames.iter().max().copied().unwrap_or_default();
    println!(
        "before (one burst, {} images): {burst:?}\n\
         after  ({} frames, budget {} images/frame): {frames:?}\n\
         after  longest single frame: {longest:?}",
        groups.len(),
        frames.len(),
        texture::PER_FRAME,
    );
    assert!(
        longest < burst,
        "spreading the upload should bound every frame well under the old \
         burst's total cost"
    );
}
