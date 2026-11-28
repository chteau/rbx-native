//! Neon against Studio 0.748: a see-through part does not glow and blends
//! as Studio does (the same (255, 170, 0) part at Transparency 0 and 0.4 in
//! front of a black wall, Future, quality 21: a halo off the opaque one's
//! edge, none at all off the see-through one's, whose middle is
//! (238, 222, 0)), and an opaque part glows by the quality level.
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
    place_of(&[([255, 170, 0], 0.0), ([255, 170, 0], 0.4)])
}

/// The black wall with a 3-stud Neon part per `(colour, transparency)`, 6
/// studs apart and centred on the view.
fn place_of(parts: &[([u8; 3], f32)]) -> WeakDom {
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
    let left = -3.0 * (parts.len() as f32 - 1.0);
    for (i, (color, transparency)) in parts.iter().enumerate() {
        let x = left + 6.0 * i as f32;
        part(
            &mut dom,
            3 + i as u32,
            x,
            [3.0, 3.0, 1.0],
            *color,
            NEON,
            *transparency,
        );
    }
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

/// The frame `dom` draws at full quality, `None` without a GPU.
fn render(dom: &WeakDom) -> Option<Vec<u8>> {
    render_at(
        dom,
        SIZE,
        QualityLevel::MAX,
        [0.0, 0.0, 14.0],
        [0.0, 0.0, 0.0],
    )
}

fn render_at(
    dom: &WeakDom,
    size: (u32, u32),
    level: u8,
    eye: [f32; 3],
    look_at: [f32; 3],
) -> Option<Vec<u8>> {
    let mut headless = viewer(dom, eye, look_at)?;
    Some(frame_at(&mut headless, size, level))
}

fn viewer(dom: &WeakDom, eye: [f32; 3], look_at: [f32; 3]) -> Option<Headless> {
    match Headless::from_dom(dom, false) {
        Ok(mut headless) => {
            headless.open_at(eye, look_at, 70.0);
            Some(headless)
        }
        Err(why) => {
            println!("skipped: no GPU to render with ({why})");
            None
        }
    }
}

fn frame_at(headless: &mut Headless, (width, height): (u32, u32), level: u8) -> Vec<u8> {
    headless.set_quality(QualityLevel::Level(level));
    for _ in 0..DRAIN {
        headless.render_frame(width, height).expect("a frame");
    }
    headless.take_frame().expect("the last frame");
    headless.render_frame(width, height).expect("a frame");
    headless
        .take_frame()
        .expect("the compared frame")
        .expect("one frame in flight")
        .pixels
}

/// The Baseplate template's bloom, which the measurements were taken under.
fn with_template_bloom(mut dom: WeakDom) -> WeakDom {
    let lighting = Ref::new(20);
    dom.insert(Instance::new(lighting, "Lighting", "Lighting"));
    dom.set_parent(lighting, None);
    let mut bloom = Instance::new(Ref::new(21), "BloomEffect", "Bloom");
    let properties = bloom.properties_mut();
    properties.insert("Intensity".to_string(), Variant::Float32(1.0));
    properties.insert("Size".to_string(), Variant::Float32(24.0));
    properties.insert("Threshold".to_string(), Variant::Float32(2.0));
    dom.insert(bloom);
    dom.set_parent(Ref::new(21), Some(lighting));
    dom.take_changes();
    dom
}

#[test]
fn a_see_through_neon_part_draws_dimmer_and_does_not_glow() {
    let Some(pixels) = render(&place()) else {
        return;
    };

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

/// Opaque Neon squares its colour and glows past a line, as in Studio 0.748
/// (ShadowMap, quality 21): grey 64 reads 43 in the middle, Studio's own
/// (255, 140, 0) reads (255, 251, 33), grey 192 glows as white does and
/// grey 160 not at all. The camera here sits closer than Studio's did, so
/// the halo is only checked for being there.
#[test]
fn opaque_neon_squares_its_colour_and_glows_past_a_line() {
    let dom = with_template_bloom(place_of(&[
        ([64, 64, 64], 0.0),
        ([255, 140, 0], 0.0),
        ([160, 160, 160], 0.0),
        ([192, 192, 192], 0.0),
    ]));
    let Some(pixels) = render(&dom) else {
        return;
    };
    // 6 studs at 14 away under a 70 degree view, 160 px tall: 49 px apart.
    let column = |i: u32| SIZE.0 / 2 - 73 + 49 * i;
    let cy = SIZE.1 / 2;

    let grey = pixel(&pixels, column(0), cy);
    assert!(grey[0].abs_diff(43) <= 8, "grey 64 middle {grey:?}");
    let orange = pixel(&pixels, column(1), cy);
    assert!(
        orange[0] == 255 && orange[1] >= 230 && orange[2] <= 40,
        "orange middle {orange:?}"
    );
    // The glow is a couple of pixels wide at this size: read the first row
    // off the face.
    let halo = |x: u32| pixel(&pixels, x, top_edge(&pixels, x));
    assert!(
        halo(column(3))[0] > 60,
        "grey 192 glows: {:?}",
        halo(column(3))
    );
    assert!(
        halo(column(2))[0] < 30,
        "grey 160 doesn't: {:?}",
        halo(column(2))
    );
}

/// See-through Neon blends in the square root of radiance, as in Studio 0.748
/// (Future, quality 21, Transparency 0.4 over a black wall): grey 60 stays dark
/// at 14 rather than turning light grey, grey 128 reads 148 and (0, 85, 255)
/// reads (0, 47, 238) rather than cyan. The wall here is near-black and lit,
/// not black, hence the slack.
#[test]
fn a_see_through_neon_part_keeps_its_colour() {
    let Some(pixels) = render(&place_of(&[
        ([60, 60, 60], 0.4),
        ([128, 128, 128], 0.4),
        ([0, 85, 255], 0.4),
    ])) else {
        return;
    };
    let column = |i: u32| SIZE.0 / 2 - 49 + 49 * i;
    let cy = SIZE.1 / 2;
    let near = |got: [u8; 3], want: [u8; 3]| got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 12);

    let dark = pixel(&pixels, column(0), cy);
    assert!(near(dark, [14, 14, 14]), "grey 60 middle {dark:?}");
    let grey = pixel(&pixels, column(1), cy);
    assert!(near(grey, [148, 148, 148]), "grey 128 middle {grey:?}");
    let blue = pixel(&pixels, column(2), cy);
    assert!(near(blue, [0, 47, 238]), "blue middle {blue:?}");
}

/// Opaque Neon grey 60 and 128, (255, 140, 0) and (0, 85, 255) at every glow
/// Studio has, read in Studio 0.748 (Future, the template's bloom, a 759 by
/// 397 viewport, 24 studs off, `EditQualityLevel` stepped 1 to 21): the
/// middle of each part and the glow 4 pixels over its top edge. Without the
/// glow (levels 1 to 5) the middles are what Neon's colour alone gives; the
/// glow puts the weaker channels back, a little at 6 to 11 and more from 12.
#[test]
fn opaque_neon_glows_by_the_level_as_studio_does() {
    const VIEW: (u32, u32) = (759, 397);
    let mut dom = with_template_bloom(place_of(&[
        ([60, 60, 60], 0.0),
        ([128, 128, 128], 0.0),
        ([255, 140, 0], 0.0),
        ([0, 85, 255], 0.0),
    ]));
    // Future, as the place Studio was measured in.
    dom.get_mut(Ref::new(20))
        .expect("Lighting")
        .properties_mut()
        .insert("Technology".to_string(), Variant::Enum(4));
    dom.take_changes();
    // Studio's own framing: the parts 22, 16, 10 and 4 studs left of the eye
    // and 8 above it.
    let (eye, look_at) = ([13.0, -8.0, 24.0], [13.0, -8.0, 0.0]);
    let per_stud = 198.5 / (35.0f32.to_radians().tan() * 23.5);
    let column = |i: usize| (379.5 + per_stud * (-22.0 + 6.0 * i as f32)).round() as u32;
    let (middle, over) = (
        (198.5 - per_stud * 8.0).round() as u32,
        (198.5 - per_stud * 9.5 - 4.0).round() as u32,
    );
    let read = |pixels: &[u8], x: u32, y: u32| {
        let at = ((y * VIEW.0 + x) * 4) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    };
    let near = |got: [u8; 3], want: [u8; 3], slack: u8| {
        got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= slack)
    };

    // One viewer stepped through the levels, as the frame rate manager does:
    // the reduced glow builds a deeper bloom chain than the full one.
    let Some(mut headless) = viewer(&dom, eye, look_at) else {
        return;
    };
    let mut halos = Vec::new();
    for (level, middles) in [
        (1, [[31; 3], [215; 3], [253, 231, 0], [0, 99, 253]]),
        (6, [[31; 3], [215; 3], [254, 242, 0], [0, 124, 254]]),
        (21, [[31; 3], [215; 3], [255, 250, 0], [0, 156, 255]]),
        (12, [[31; 3], [215; 3], [255, 249, 0], [0, 154, 255]]),
    ] {
        let pixels = frame_at(&mut headless, VIEW, level);
        for (i, want) in middles.into_iter().enumerate() {
            let got = read(&pixels, column(i), middle);
            assert!(
                near(got, want, 6),
                "Q{level} part {i} middle {got:?}, Studio {want:?}"
            );
        }
        halos.push([
            read(&pixels, column(2), over),
            read(&pixels, column(3), over),
        ]);
    }

    let [off, reduced, full] = [halos[0], halos[1], halos[2]];
    assert!(
        near(halos[3][0], full[0], 8),
        "Q12 glows as Q21 does: {halos:?}"
    );
    assert!(
        off[0][0] < 12 && off[1][2] < 30,
        "no glow at level 1: {off:?}"
    );
    // Studio (180, 98, 22) and (7, 30, 181).
    assert!(
        near(full[0], [180, 98, 22], 30),
        "Q21 orange glow {:?}",
        full[0]
    );
    assert!(
        near(full[1], [7, 30, 181], 30),
        "Q21 blue glow {:?}",
        full[1]
    );
    // Studio (107, 75, 6) and (2, 15, 102): fainter than the full glow, and
    // more yellow. Its strength follows the frame more than the full glow
    // does, so at this small size it lands brighter than Studio's.
    assert!(
        reduced[0][0] > 90 && reduced[0][0] < full[0][0],
        "Q6 orange glow {:?}",
        reduced[0]
    );
    assert!(
        reduced[1][2] > 90 && reduced[1][2] < full[1][2],
        "Q6 blue glow {:?}",
        reduced[1]
    );
    let yellow = |[r, g, _]: [u8; 3]| f32::from(g) / f32::from(r);
    assert!(
        yellow(reduced[0]) > yellow(full[0]),
        "{reduced:?} against {full:?}"
    );
}
