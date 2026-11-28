//! The hover outline (and the selection's, built the same way) still draws
//! after the quality level crosses the multisampling line: their pipelines
//! have to follow the scene pass's sample count, or every frame after the switch fails validation and
//! the viewport stops updating.
//!
//! Needs a GPU. Without one the test says so and passes, the same way
//! `isolation.rs` does.

use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::Selected;
use rbx_viewer::{Headless, QualityLevel};

const SIZE: (u32, u32) = (160, 120);
const DRAIN: usize = 8;
const WORKSPACE: u32 = 1;
const MODEL: u32 = 2;
const BLOCK: u32 = 3;

/// `Workspace { Model { Block } }`: one dark block in the middle of the view,
/// inside a model so hovering it draws the hover pipeline's box.
fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(WORKSPACE), "Workspace", "Workspace"));
    dom.insert(Instance::new(Ref::new(MODEL), "Model", "Model"));
    let mut block = Instance::new(Ref::new(BLOCK), "Part", "Block");
    let properties = block.properties_mut();
    properties.insert(
        "size".to_string(),
        Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 4.0,
            z: 4.0,
        }),
    );
    properties.insert(
        "CFrame".to_string(),
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    properties.insert(
        "Color3uint8".to_string(),
        Variant::Color3uint8 {
            r: 20,
            g: 20,
            b: 20,
        },
    );
    dom.insert(block);
    dom.set_parent(Ref::new(WORKSPACE), None);
    dom.set_parent(Ref::new(MODEL), Some(Ref::new(WORKSPACE)));
    dom.set_parent(Ref::new(BLOCK), Some(Ref::new(MODEL)));
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

/// The hover cue's amber, over the dark block or the sky.
fn amber(pixels: &[u8], x: u32, y: u32) -> bool {
    let at = ((y * SIZE.0 + x) * 4) as usize;
    let [r, g, b] = [pixels[at], pixels[at + 1], pixels[at + 2]].map(i16::from);
    r > 120 && r > g + 40 && r > b + 80
}

#[test]
fn the_hover_outline_draws_after_multisampling_switches_off() {
    let dom = place();
    let mut headless = match Headless::from_dom(&dom, false) {
        Ok(headless) => headless,
        Err(why) => {
            println!("skipped: no GPU to render with ({why})");
            return;
        }
    };
    let model = Selected::read(&dom, ReflectionDatabase::shared(), Ref::new(MODEL));
    headless.set_quality(QualityLevel::Level(QualityLevel::MAX));
    headless.set_hover(vec![model]);
    // Looking away: a frame that failed after the switch would hand back one
    // of these, with no block in it.
    headless.open_at([0.0, 0.0, -12.0], [0.0, 0.0, -30.0], 70.0);
    frame(&mut headless);

    headless.set_quality(QualityLevel::Level(10));
    headless.open_at([0.0, 0.0, -12.0], [0.0, 0.0, 0.0], 70.0);
    let pixels = frame(&mut headless);

    let cy = SIZE.1 / 2;
    assert!(
        (0..SIZE.0).any(|x| amber(&pixels, x, cy)),
        "no hover outline on the centre row after the switch"
    );
}
