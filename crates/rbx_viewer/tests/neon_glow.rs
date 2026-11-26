//! A see-through Neon part does not glow, and draws as Studio does: the
//! same (255, 170, 0) part at Transparency 0 and 0.4 in front of a black
//! wall, measured in Studio 0.748 (Future, quality 21) at a halo of
//! (164, 131, 1) 5 px off the opaque one's edge at 1080p (2 px at this
//! size, the blur scales with the frame), none at all off the
//! see-through one's, whose middle is (238, 222, 0).
//!
//! Needs a GPU. Without one the test says so and passes, the same way
//! `isolation.rs` does.

use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_viewer::{Headless, QualityLevel};

const SIZE: (u32, u32) = (320, 160);
const DRAIN: usize = 8;
const NEON: u32 = 288;
const SMOOTH_PLASTIC: u32 = 272;

fn part(
    dom: &mut WeakDom,
    id: u32,
    x: f32,
    size: [f32; 3],
    color: [u8; 3],
    material: u32,
    transparency: f32,
) {
    let mut part = Instance::new(Ref::new(id), "Part", "Part");
    let properties = part.properties_mut();
    let [sx, sy, sz] = size;
    properties.insert(
        "size".to_string(),
        Variant::Vector3(Vector3Data {
            x: sx,
            y: sy,
            z: sz,
        }),
    );
    properties.insert(
        "CFrame".to_string(),
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x,
                y: 0.0,
                z: if material == NEON { 0.0 } else { -3.0 },
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    let [r, g, b] = color;
    properties.insert("Color3uint8".to_string(), Variant::Color3uint8 { r, g, b });
    properties.insert("Material".to_string(), Variant::Enum(material));
    properties.insert("Transparency".to_string(), Variant::Float32(transparency));
    dom.insert(part);
    dom.set_parent(Ref::new(id), Some(Ref::new(1)));
}

/// `Workspace { Wall, Opaque, SeeThrough }`: the two neon parts 6 studs
/// apart, the black wall behind both.
fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
    dom.set_parent(Ref::new(1), None);
    part(
        &mut dom,
        2,
        0.0,
        [60.0, 40.0, 1.0],
        [8, 8, 8],
        SMOOTH_PLASTIC,
        0.0,
    );
    part(&mut dom, 3, -3.0, [3.0, 3.0, 1.0], [255, 170, 0], NEON, 0.0);
    part(&mut dom, 4, 3.0, [3.0, 3.0, 1.0], [255, 170, 0], NEON, 0.4);
    dom.take_changes();
    dom
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let at = ((y * SIZE.0 + x) * 4) as usize;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

/// The first row above `x`'s middle that is no longer the part's own
/// saturated face, walking up from the centre: the edge.
fn top_edge(pixels: &[u8], x: u32) -> u32 {
    let cy = SIZE.1 / 2;
    let face = pixel(pixels, x, cy);
    (0..cy)
        .rev()
        .find(|&y| pixel(pixels, x, y) != face)
        .expect("an edge")
}

#[test]
fn a_see_through_neon_part_draws_dimmer_and_does_not_glow() {
    let mut headless = match Headless::from_dom(&place(), false) {
        Ok(headless) => headless,
        Err(why) => {
            println!("skipped: no GPU to render with ({why})");
            return;
        }
    };
    headless.set_quality(QualityLevel::Level(QualityLevel::MAX));
    headless.open_at([0.0, 0.0, 14.0], [0.0, 0.0, 0.0], 70.0);
    for _ in 0..DRAIN {
        headless.render_frame(SIZE.0, SIZE.1).expect("a frame");
    }
    headless.take_frame().expect("the last frame");
    headless.render_frame(SIZE.0, SIZE.1).expect("a frame");
    let pixels = headless
        .take_frame()
        .expect("the compared frame")
        .expect("one frame in flight")
        .pixels;

    // Left of centre the opaque one, right of it the see-through one.
    let (opaque, see_through) = (SIZE.0 / 2 - 24, SIZE.0 / 2 + 24);
    let cy = SIZE.1 / 2;
    let [r, g, b] = pixel(&pixels, see_through, cy);
    assert!(
        r.abs_diff(238) <= 6 && g.abs_diff(222) <= 8 && b <= 6,
        "see-through middle {:?}",
        [r, g, b]
    );
    let halo = |x: u32| pixel(&pixels, x, top_edge(&pixels, x).saturating_sub(2));
    assert!(
        halo(opaque)[0] > 60,
        "the opaque one glows: {:?}",
        halo(opaque)
    );
    assert!(
        halo(see_through)[0] < 12,
        "the see-through one doesn't: {:?}",
        halo(see_through)
    );
}
