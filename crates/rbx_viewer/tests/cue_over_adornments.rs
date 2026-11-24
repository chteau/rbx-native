//! The selection outline sits where Studio layers it among adornments:
//! over a depth-tested one but faded to a fifth where the opaque adornment
//! stands in front of the part's edge, and under an `AlwaysOnTop` one.
//! Measured in Studio 2026-11-24 with HandleAdornments across a selected
//! part's silhouette.
//!
//! Needs a GPU. Without one the test says so and passes, the same way
//! `isolation.rs` does.

use rbx_dom::{CFrameData, Color3Data, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::Selected;
use rbx_viewer::Headless;

const SIZE: (u32, u32) = (160, 120);
const DRAIN: usize = 8;
const WORKSPACE: u32 = 1;
const BLOCK: u32 = 2;
const IN_FRONT: u32 = 3;
const ON_TOP: u32 = 4;

fn vector(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn at(x: f32, y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    })
}

/// A bar across the block, `y` up from its middle and in front of it: wide
/// enough to cross both its left and right edges.
fn bar(referent: u32, y: f32, color: [f32; 3], on_top: bool) -> Instance {
    let mut bar = Instance::new(Ref::new(referent), "BoxHandleAdornment", "Bar");
    let properties = bar.properties_mut();
    properties.insert("Adornee".to_string(), Variant::Ref(Ref::new(BLOCK)));
    properties.insert("Size".to_string(), vector(10.0, 1.0, 1.0));
    properties.insert("CFrame".to_string(), at(0.0, y, -3.0));
    properties.insert(
        "Color3".to_string(),
        Variant::Color3(Color3Data {
            r: color[0],
            g: color[1],
            b: color[2],
        }),
    );
    properties.insert("AlwaysOnTop".to_string(), Variant::Bool(on_top));
    bar
}

fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(WORKSPACE), "Workspace", "Workspace"));
    let mut block = Instance::new(Ref::new(BLOCK), "Part", "Block");
    let properties = block.properties_mut();
    properties.insert("size".to_string(), vector(4.0, 4.0, 4.0));
    properties.insert("CFrame".to_string(), at(0.0, 0.0, 0.0));
    properties.insert(
        "Color3uint8".to_string(),
        Variant::Color3uint8 {
            r: 160,
            g: 160,
            b: 160,
        },
    );
    dom.insert(block);
    dom.insert(bar(IN_FRONT, 0.0, [1.0, 0.0, 0.0], false));
    dom.insert(bar(ON_TOP, 1.2, [0.0, 1.0, 0.0], true));
    dom.set_parent(Ref::new(WORKSPACE), None);
    for child in [BLOCK, IN_FRONT, ON_TOP] {
        dom.set_parent(Ref::new(child), Some(Ref::new(WORKSPACE)));
    }
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

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let at = ((y * SIZE.0 + x) * 4) as usize;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

fn red([r, g, b]: [u8; 3]) -> bool {
    r > 150 && g < 60 && b < 60
}

fn green([r, g, b]: [u8; 3]) -> bool {
    g > 150 && r < 60 && b < 60
}

/// The middle row of the run of rows whose centre pixel is `which`.
fn row_of(pixels: &[u8], which: fn([u8; 3]) -> bool) -> u32 {
    let rows: Vec<u32> = (0..SIZE.1)
        .filter(|&y| which(pixel(pixels, SIZE.0 / 2, y)))
        .collect();
    assert!(!rows.is_empty(), "no bar on the centre column");
    rows[rows.len() / 2]
}

/// The pixel of row `y` the selection changed the most, before and after,
/// with how far it moved summed over the channels.
fn most_changed(before: &[u8], after: &[u8], y: u32) -> ([u8; 3], [u8; 3], u32) {
    (0..SIZE.0)
        .map(|x| {
            let (a, b) = (pixel(before, x, y), pixel(after, x, y));
            (a, b, (0..3).map(|c| u32::from(a[c].abs_diff(b[c]))).sum())
        })
        .max_by_key(|&(_, _, moved)| moved)
        .expect("a row of pixels")
}

#[test]
fn the_outline_fades_behind_a_depth_tested_adornment_and_hides_under_an_on_top_one() {
    let mut headless = match Headless::from_dom(&place(), false) {
        Ok(headless) => headless,
        Err(why) => {
            println!("skipped: no GPU to render with ({why})");
            return;
        }
    };
    headless.open_at([0.0, 0.0, -12.0], [0.0, 0.0, 0.0], 70.0);
    headless.set_selection_occluded(false);
    let before = frame(&mut headless);

    let in_front = row_of(&before, red);
    let on_top = row_of(&before, green);
    // As far from the red bar as the green one, the other way: the block's
    // edge with nothing in front of it.
    let clear = 2 * in_front - on_top;

    let dom = place();
    let selected = Selected::read(&dom, ReflectionDatabase::shared(), Ref::new(BLOCK));
    headless.set_selection(&[selected]);
    let after = frame(&mut headless);

    let (_, outline, moved) = most_changed(&before, &after, clear);
    assert!(moved > 150, "the outline is drawn in full: {outline:?}");
    // Over the red bar, a fifth of the way from the bar to the outline, in
    // sRGB values, the way Studio blends it.
    let (under, faded, moved) = most_changed(&before, &after, in_front);
    assert!(moved > 15, "the outline still shows faintly: {faded:?}");
    for c in 0..3 {
        let expected = f32::from(under[c]) + 0.2 * (f32::from(outline[c]) - f32::from(under[c]));
        assert!(
            (f32::from(faded[c]) - expected).abs() <= 4.0,
            "faded behind the depth-tested bar: {faded:?} over {under:?}, outline {outline:?}"
        );
    }
    let (_, _, moved) = most_changed(&before, &after, on_top);
    assert!(moved <= 3, "covered by the AlwaysOnTop bar: {moved}");
}
