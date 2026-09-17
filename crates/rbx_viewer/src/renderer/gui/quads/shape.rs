//! The per-element geometry a quad carries beyond its four corners: the
//! rotation every corner goes through, the rounded box the fragment shader
//! keeps its pixels inside, and the paint (colour, band, gradient) it fills
//! them with.

use super::super::pipeline::{mode, VertexRaw};
use crate::scene::{GuiElement, GuiGradient, GuiGradientKind, GuiJoin, GuiRect, GuiTile};

/// `GuiObject.Rotation` about one fixed pivot, shared by every quad an
/// element contributes (background, border bands, image) so they turn
/// together as a rigid box — Roblox gives no way to rotate about anything but
/// the element's own centre, so that is the only pivot this ever takes.
pub(super) struct Spin {
    sin: f32,
    cos: f32,
    pivot: [f32; 2],
}

impl Spin {
    pub(super) fn new(degrees: f32, pivot: [f32; 2]) -> Self {
        // Positive `Rotation` turns clockwise on screen: Roblox's own style
        // docs describe a transition *to* a negative rotation as turning a
        // button counterclockwise (content/en-us/ui/styling/editor.md), and
        // this coordinate space already has y increasing downward, so the
        // ordinary (cos, sin; -sin, cos) rotation matrix needs no extra flip.
        let radians = degrees.to_radians();
        Spin {
            sin: radians.sin(),
            cos: radians.cos(),
            pivot,
        }
    }

    pub(super) fn apply(&self, point: [f32; 2]) -> [f32; 2] {
        let dx = point[0] - self.pivot[0];
        let dy = point[1] - self.pivot[1];
        [
            self.pivot[0] + dx * self.cos - dy * self.sin,
            self.pivot[1] + dx * self.sin + dy * self.cos,
        ]
    }
}

pub(super) fn center(rect: &GuiRect) -> [f32; 2] {
    [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5]
}

/// A half-size so large the box's outline is nowhere near any pixel: what a
/// plain quad passes so the fragment's signed distance is always deep inside.
/// Modest, so that `band`'s own far edge (`FILL`) keeps a comfortable margin
/// within `f32` precision.
const UNBOUNDED: f32 = 1.0e4;

/// The band of a fill: everything inside the outline, nothing outside.
pub(super) const FILL: [f32; 2] = [-1.0e5, 0.0];

/// The rounded box an element's quads are shaped by, in the element's own
/// unrotated frame: `center` is where `local` is measured from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Shape {
    pub(super) center: [f32; 2],
    pub(super) half: [f32; 2],
    pub(super) radii: [f32; 4],
    pub(super) join: GuiJoin,
}

impl Shape {
    /// The element's own outline, which its stroke always follows.
    pub(super) fn of(element: &GuiElement) -> Self {
        Shape {
            center: center(&element.rect),
            half: [element.rect.width * 0.5, element.rect.height * 0.5],
            radii: element.corner_radii,
            join: element.stroke.map_or(GuiJoin::Round, |stroke| stroke.join),
        }
    }

    /// What the element's fills (background, border, image) are shaped by:
    /// the outline where a `UICorner` rounds it, otherwise nothing at all —
    /// a sharp box is left exactly as the rasterizer cuts it.
    pub(super) fn fill_of(element: &GuiElement) -> Self {
        let shape = Shape::of(element);
        match element.corner_radii.iter().any(|&radius| radius > 0.0) {
            true => shape,
            false => Shape {
                half: [UNBOUNDED, UNBOUNDED],
                radii: [0.0; 4],
                ..shape
            },
        }
    }
}

/// What a quad is filled with.
#[derive(Debug, Clone, Copy)]
pub(super) struct Paint<'a> {
    pub(super) color: [f32; 3],
    pub(super) alpha: f32,
    pub(super) band: [f32; 2],
    /// The element's gradient and the texture row it was baked to.
    pub(super) gradient: Option<(usize, &'a GuiGradient)>,
}

/// Two triangles covering `rect`, the image repeating `repeat` times across
/// it and every corner turned by `spin` — an identity `Spin` (zero rotation)
/// leaves them exactly where `rect` puts them.
pub(super) fn quad(
    rect: &GuiRect,
    repeat: [f32; 2],
    paint: &Paint,
    shape: &Shape,
    spin: &Spin,
    into: &mut Vec<VertexRaw>,
) {
    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    let (gradient, gradient_row, kind, tile) = match paint.gradient {
        Some((row, gradient)) => (
            [
                gradient.origin[0],
                gradient.origin[1],
                gradient.axis[0],
                gradient.axis[1],
            ],
            row as f32,
            gradient.kind as u32,
            gradient.tile as u32,
        ),
        None => ([0.0; 4], -1.0, 0, 0),
    };
    let mode = mode(shape.join as u32, kind, tile);
    let corner = |position: [f32; 2], uv: [f32; 2]| VertexRaw {
        position: spin.apply(position),
        uv,
        color: paint.color,
        alpha: paint.alpha,
        local: [position[0] - shape.center[0], position[1] - shape.center[1]],
        half: shape.half,
        radii: shape.radii,
        band: paint.band,
        gradient,
        gradient_row,
        mode,
    };

    let top_left = corner([left, top], [0.0, 0.0]);
    let top_right = corner([right, top], [repeat[0], 0.0]);
    let bottom_left = corner([left, bottom], [0.0, repeat[1]]);
    let bottom_right = corner([right, bottom], repeat);
    into.extend([
        top_left,
        top_right,
        bottom_left,
        top_right,
        bottom_right,
        bottom_left,
    ]);
}

/// The four bands of a `BorderMode.Outline` border, which sits just outside
/// the element rather than eating into it. The two horizontal bands run the
/// full outer width so the corners are covered exactly once.
pub(super) fn outline(rect: &GuiRect, width: f32) -> [GuiRect; 4] {
    [
        GuiRect {
            x: rect.x - width,
            y: rect.y - width,
            width: rect.width + 2.0 * width,
            height: width,
        },
        GuiRect {
            x: rect.x - width,
            y: rect.y + rect.height,
            width: rect.width + 2.0 * width,
            height: width,
        },
        GuiRect {
            x: rect.x - width,
            y: rect.y,
            width,
            height: rect.height,
        },
        GuiRect {
            x: rect.x + rect.width,
            y: rect.y,
            width,
            height: rect.height,
        },
    ]
}

/// The quad a stroke band needs: `rect` grown by the band's outer edge plus
/// a pixel for the anti-aliasing ramp, or `rect` itself for a band that
/// never leaves the box.
pub(super) fn grown(rect: &GuiRect, band: [f32; 2]) -> GuiRect {
    let margin = band[1].max(0.0) + 1.0;
    GuiRect {
        x: rect.x - margin,
        y: rect.y - margin,
        width: rect.width + 2.0 * margin,
        height: rect.height + 2.0 * margin,
    }
}

// The `as u32` casts above rely on the enums' discriminants being the Roblox
// ordinals the shader unpacks; these pin that down.
const _: () = assert!(GuiJoin::Miter as u32 == 2);
const _: () = assert!(GuiGradientKind::Conical as u32 == 2);
const _: () = assert!(GuiTile::Mirror as u32 == 2);
