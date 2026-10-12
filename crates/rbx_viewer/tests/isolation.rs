//! An isolated view draws the rig under its root on a flat grey backdrop
//! and nothing else of the place — not a wall behind it, however large and
//! however bright — and lifting the isolation draws the place exactly as
//! before.
//!
//! Needs a GPU. Without one the test says so and passes, rather than being
//! `#[ignore]`d: it opens a single device, and a software rasteriser (Mesa's
//! lavapipe, `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`) is
//! enough to run it anywhere.

use std::path::PathBuf;

use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_viewer::{Applied, Headless, ISOLATION_BACKDROP};

const SIZE: (u32, u32) = (160, 120);
/// Frames drawn before the compared one: `Renderer::draw` spreads uploads
/// over the frames after a rebuild.
const DRAIN: usize = 8;
const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
const WORKSPACE: u32 = 1;
const RIG: u32 = 2;
const TORSO: u32 = 3;
const WALL: u32 = 4;
const LIGHTING: u32 = 5;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl")
}

fn cframe(y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x: 0.0, y, z },
        rotation: IDENTITY_ROTATION,
    })
}

fn block(referent: u32, size: [f32; 3], z: f32, rgb: [u8; 3]) -> Instance {
    let mut instance = Instance::new(Ref::new(referent), "Part", "Part");
    let properties = instance.properties_mut();
    properties.insert(
        "size".to_string(),
        Variant::Vector3(Vector3Data {
            x: size[0],
            y: size[1],
            z: size[2],
        }),
    );
    properties.insert("CFrame".to_string(), cframe(0.0, z));
    let [r, g, b] = rgb;
    properties.insert("Color3uint8".to_string(), Variant::Color3uint8 { r, g, b });
    instance
}

/// `Workspace { Rig { Torso (white) }, Wall (huge, pure red) }` under a
/// midnight `Lighting`: everything an isolated view must not show.
fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(WORKSPACE), "Workspace", "Workspace"));
    dom.insert(Instance::new(Ref::new(RIG), "Model", "Rig"));
    dom.insert(block(TORSO, [2.0, 2.0, 1.0], 0.0, [255, 255, 255]));
    dom.insert(block(WALL, [400.0, 400.0, 1.0], 15.0, [255, 0, 0]));
    let mut lighting = Instance::new(Ref::new(LIGHTING), "Lighting", "Lighting");
    lighting
        .properties_mut()
        .insert("ClockTime".to_string(), Variant::Float32(0.0));
    dom.insert(lighting);
    dom.set_parent(Ref::new(WORKSPACE), None);
    dom.set_parent(Ref::new(LIGHTING), None);
    dom.set_parent(Ref::new(RIG), Some(Ref::new(WORKSPACE)));
    dom.set_parent(Ref::new(TORSO), Some(Ref::new(RIG)));
    dom.set_parent(Ref::new(WALL), Some(Ref::new(WORKSPACE)));
    dom.take_changes();
    dom
}

fn frame(headless: &mut Headless) -> Vec<u8> {
    for _ in 0..DRAIN {
        headless.render_frame(SIZE.0, SIZE.1).expect("a frame");
    }
    headless.take_frame().expect("the last frame");
    headless.render_frame(SIZE.0, SIZE.1).expect("a frame");
    headless
        .take_frame()
        .expect("the compared frame")
        .expect("one frame in flight")
        .pixels
}

fn count(pixels: &[u8], wanted: impl Fn(&[u8]) -> bool) -> usize {
    pixels.chunks(4).filter(|pixel| wanted(pixel)).count()
}

/// Any red at all, however dimly lit: the wall under a midnight sky.
fn red(pixel: &[u8]) -> bool {
    let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(u16::from);
    r > 20 && r > 2 * g + 8 && r > 2 * b + 8
}

fn backdrop(pixel: &[u8]) -> bool {
    pixel[..3]
        .iter()
        .zip(ISOLATION_BACKDROP)
        .all(|(&got, want)| got.abs_diff(want) <= 1)
}

/// Lit, and not grey: the torso, under the neutral key.
fn torso(pixel: &[u8]) -> bool {
    pixel[..3].iter().all(|&channel| channel > 90)
        && pixel[0].abs_diff(pixel[1]) < 24
        && pixel[1].abs_diff(pixel[2]) < 24
}

#[test]
fn an_isolated_rig_draws_alone_on_the_backdrop_and_lifting_it_restores_the_place() {
    let mut headless = match Headless::load(&fixture(), false) {
        Ok(headless) => headless,
        Err(why) => {
            println!("skipped: no GPU to render with ({why})");
            return;
        }
    };
    let mut dom = place();
    headless.reload(&dom).expect("the DOM rebuilds");
    // In front of the rig, looking at it and on into the wall behind it.
    headless.open_at([0.0, 1.0, -8.0], [0.0, 0.0, 0.0], 70.0);

    let whole = frame(&mut headless);
    let total = whole.len() / 4;
    assert!(
        count(&whole, red) > total / 2,
        "the wall fills the view behind the rig"
    );

    headless.pick_meshes_changed();
    headless.set_isolation(Some(vec![Ref::new(RIG)]));
    assert!(headless.isolation());
    assert!(
        headless.pick_meshes_changed(),
        "the host must re-take the meshes"
    );
    let isolated = frame(&mut headless);
    assert_eq!(count(&isolated, red), 0, "nothing of the wall is drawn");
    assert!(
        count(&isolated, backdrop) > total / 2,
        "the backdrop shows where the wall was"
    );
    assert!(count(&isolated, torso) > 50, "the rig is drawn, and lit");

    // A pose through the ordinary change path: a patch, still isolated.
    dom.set_property(Ref::new(TORSO), "CFrame", cframe(0.5, 0.0))
        .expect("a part takes a CFrame");
    let changes = dom.take_changes();
    assert_eq!(
        headless.apply_changes(&dom, &changes).expect("applies"),
        Applied::Patched
    );
    let posed = frame(&mut headless);
    assert_eq!(count(&posed, red), 0);
    assert_ne!(posed, isolated, "the torso moved");

    // A reload keeps the isolation.
    headless.reload(&dom).expect("the DOM rebuilds");
    assert_eq!(count(&frame(&mut headless), red), 0);

    dom.set_property(Ref::new(TORSO), "CFrame", cframe(0.0, 0.0))
        .expect("a part takes a CFrame");
    let changes = dom.take_changes();
    headless.apply_changes(&dom, &changes).expect("applies");
    headless.set_isolation(None);
    assert!(!headless.isolation());
    assert_eq!(
        frame(&mut headless),
        whole,
        "lifting the isolation draws the very frame drawn before it"
    );
}
