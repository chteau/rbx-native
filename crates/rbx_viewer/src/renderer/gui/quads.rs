//! Turns the resolved rectangles of `crate::scene::gui` into vertices and the
//! draw runs they are issued in, for a target of any pixel size: the screen
//! for a `ScreenGui`, an offscreen canvas for a `BillboardGui`/`SurfaceGui`.
//!
//! Runs are merged only between *consecutive* elements sharing a texture and a
//! scissor: the pass is a painter's algorithm with no depth buffer, so
//! regrouping by texture the way the ribbon passes do would reorder the paint
//! and is not available here.

mod text;

use std::collections::HashMap;
use std::ops::Range;

use rbx_assets::AssetRef;

use super::pipeline::VertexRaw;
use super::text::Typesetter;
use crate::scene::{GuiElement, GuiRect};

/// The slot every untextured rectangle — a background, a border — samples: a
/// single white texel, so one pipeline covers both.
pub(super) const WHITE: usize = 0;

/// A scissor rectangle in the integer pixels `wgpu` wants, y down from the
/// target's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Scissor {
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// One contiguous slice of the vertex buffer sharing a texture and a scissor.
pub(super) struct Run {
    pub(super) texture: usize,
    pub(super) scissor: Option<Scissor>,
    pub(super) range: Range<u32>,
}

/// Every rectangle of every element, in paint order.
pub(super) fn build(
    elements: &[GuiElement],
    textures: &HashMap<AssetRef, usize>,
    target: (u32, u32),
    fonts: &mut Typesetter,
) -> (Vec<VertexRaw>, Vec<Run>) {
    loop {
        let generation = fonts.atlas.generation();
        let built = build_once(elements, textures, target, fonts);
        // The glyph atlas grew part-way through and every UV handed out
        // before that names the old packing: once more over the same
        // elements, every glyph now cached. Growth is bounded, so this ends.
        if fonts.atlas.generation() == generation {
            return built;
        }
    }
}

fn build_once(
    elements: &[GuiElement],
    textures: &HashMap<AssetRef, usize>,
    target: (u32, u32),
    fonts: &mut Typesetter,
) -> (Vec<VertexRaw>, Vec<Run>) {
    let mut vertices = Vec::new();
    let mut runs: Vec<Run> = Vec::new();

    for element in elements {
        let scissor = match &element.clip {
            Some(clip) => match scissor(clip, target) {
                Some(scissor) => Some(scissor),
                // Clipped away to nothing: `wgpu` rejects a zero-sized scissor
                // anyway, and there would be nothing to see through it.
                None => continue,
            },
            None => None,
        };
        if element.rect.width <= 0.0 || element.rect.height <= 0.0 {
            continue;
        }

        let spin = Spin::new(element.rotation, center(&element.rect));

        let start = vertices.len();
        if element.background_alpha > 0.0 {
            quad(
                &element.rect,
                [1.0, 1.0],
                element.background,
                element.background_alpha,
                &spin,
                &mut vertices,
            );
            // Roblox ties the outline to `BackgroundTransparency`: a frame
            // with no background shows no border either.
            if let Some((width, color)) = element.border {
                // The bands are rotated about the element's own centre, same
                // as the background — not each band's own, or a rotated
                // border would fly apart from the box it outlines.
                for side in outline(&element.rect, width) {
                    quad(
                        &side,
                        [1.0, 1.0],
                        color,
                        element.background_alpha,
                        &spin,
                        &mut vertices,
                    );
                }
            }
        }
        extend(&mut runs, WHITE, scissor, start..vertices.len());

        // Never downloaded, or the fetch failed: Roblox draws nothing at all
        // for an image it cannot load, so neither does this.
        let image = element
            .image
            .as_ref()
            .filter(|image| image.alpha > 0.0)
            .and_then(|image| Some((image, *textures.get(&image.asset)?)));
        if let Some((image, texture)) = image {
            let start = vertices.len();
            quad(
                &element.rect,
                image.repeat,
                image.tint,
                image.alpha,
                &spin,
                &mut vertices,
            );
            extend(&mut runs, texture, scissor, start..vertices.len());
        }

        // Last, over the background and the image, as Roblox layers a text
        // object.
        if let Some(typeset) = &element.text {
            text::emit(
                &element.rect,
                typeset,
                &text::strokes(&typeset.text),
                &spin,
                scissor,
                fonts,
                &mut vertices,
                &mut runs,
            );
        }
    }

    (vertices, runs)
}

/// Appends to the last run where it shares this one's texture and scissor,
/// which is the common case: most elements are a plain background in a row.
fn extend(runs: &mut Vec<Run>, texture: usize, scissor: Option<Scissor>, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    let range = range.start as u32..range.end as u32;
    match runs.last_mut() {
        Some(last) if last.texture == texture && last.scissor == scissor => {
            last.range.end = range.end;
        }
        _ => runs.push(Run {
            texture,
            scissor,
            range,
        }),
    }
}

/// `GuiObject.Rotation` about one fixed pivot, shared by every quad an
/// element contributes (background, border bands, image) so they turn
/// together as a rigid box — Roblox gives no way to rotate about anything but
/// the element's own centre, so that is the only pivot this ever takes.
struct Spin {
    sin: f32,
    cos: f32,
    pivot: [f32; 2],
}

impl Spin {
    fn new(degrees: f32, pivot: [f32; 2]) -> Self {
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

    fn apply(&self, point: [f32; 2]) -> [f32; 2] {
        let dx = point[0] - self.pivot[0];
        let dy = point[1] - self.pivot[1];
        [
            self.pivot[0] + dx * self.cos - dy * self.sin,
            self.pivot[1] + dx * self.sin + dy * self.cos,
        ]
    }
}

fn center(rect: &GuiRect) -> [f32; 2] {
    [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5]
}

/// Two triangles covering `rect`, the image repeating `repeat` times across
/// it and every corner turned by `spin` — an identity `Spin` (zero rotation)
/// leaves them exactly where `rect` puts them.
fn quad(
    rect: &GuiRect,
    repeat: [f32; 2],
    color: [f32; 3],
    alpha: f32,
    spin: &Spin,
    into: &mut Vec<VertexRaw>,
) {
    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    let corner = |position: [f32; 2], uv: [f32; 2]| VertexRaw {
        position: spin.apply(position),
        uv,
        color,
        alpha,
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
fn outline(rect: &GuiRect, width: f32) -> [GuiRect; 4] {
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

/// A clip rectangle rounded out to whole pixels and clamped to the target,
/// `None` where nothing of it is left on screen.
///
/// Rounded *outwards* rather than to the nearest pixel: a scissor that cut
/// half a pixel short would leave a visible seam along a clipping frame's own
/// edge, while half a pixel of overdraw is invisible.
fn scissor(clip: &GuiRect, target: (u32, u32)) -> Option<Scissor> {
    let left = clip.x.floor().max(0.0);
    let top = clip.y.floor().max(0.0);
    let right = (clip.x + clip.width).ceil().min(target.0 as f32);
    let bottom = (clip.y + clip.height).ceil().min(target.1 as f32);

    if right <= left || bottom <= top {
        return None;
    }
    Some(Scissor {
        x: left as u32,
        y: top as u32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

#[cfg(test)]
mod tests;
