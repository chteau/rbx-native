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

use super::atlas::Slot;
use super::gradient::Rows;
use super::pipeline::VertexRaw;
use super::text::Typesetter;
use crate::scene::{GuiElement, GuiRect};

mod image;
mod shape;

use shape::{center, grown, outline, quad, Paint, Shape, Spin, FILL, UV_WHOLE};

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

/// Every rectangle of every element, in paint order, and the gradient ramps
/// their vertices refer to by row.
pub(super) fn build(
    elements: &[GuiElement],
    textures: &HashMap<AssetRef, Slot>,
    target: (u32, u32),
    fonts: &mut Typesetter,
) -> (Vec<VertexRaw>, Vec<Run>, Rows) {
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
    textures: &HashMap<AssetRef, Slot>,
    target: (u32, u32),
    fonts: &mut Typesetter,
) -> (Vec<VertexRaw>, Vec<Run>, Rows) {
    let mut vertices = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut rows = Rows::default();

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
        let fill = Shape::fill_of(element);
        let gradient = element
            .gradient
            .as_ref()
            .map(|gradient| (rows.row(gradient), gradient));

        let start = vertices.len();
        if element.background_alpha > 0.0 {
            let paint = Paint {
                color: element.background,
                alpha: element.background_alpha,
                band: FILL,
                gradient,
            };
            quad(&element.rect, UV_WHOLE, &paint, &fill, &spin, &mut vertices);
            // Roblox ties the outline to `BackgroundTransparency`: a frame
            // with no background shows no border either.
            if let Some((width, color)) = element.border {
                let paint = Paint { color, ..paint };
                // The bands are rotated about the element's own centre, same
                // as the background — not each band's own, or a rotated
                // border would fly apart from the box it outlines.
                for side in outline(&inset(&element.rect, element.border_inset), width) {
                    quad(&side, UV_WHOLE, &paint, &fill, &spin, &mut vertices);
                }
            }
        }
        extend(&mut runs, WHITE, scissor, start..vertices.len());

        if let Some(image) = &element.image {
            // Never downloaded, or the fetch failed: Roblox draws nothing at
            // all for an image it cannot load, so neither does this.
            if let Some(slot) = textures.get(&image.asset) {
                if image.alpha > 0.0 {
                    let texture = match image.pixelated {
                        true => slot.nearest,
                        false => slot.linear,
                    };
                    let start = vertices.len();
                    let paint = Paint {
                        color: image.tint,
                        alpha: image.alpha,
                        band: FILL,
                        gradient,
                    };
                    image::build(
                        &element.rect,
                        image,
                        slot.size,
                        &paint,
                        &fill,
                        &spin,
                        &mut vertices,
                    );
                    extend(&mut runs, texture, scissor, start..vertices.len());
                }
            }
        }

        // Over the image, and — unlike the border — independent of the
        // background: the docs give `UIStroke.Transparency` its own life so
        // a box can be "hollow", an outline and nothing else.
        if let Some(stroke) = &element.stroke {
            if !stroke.on_text && stroke.alpha > 0.0 {
                let start = vertices.len();
                let paint = Paint {
                    color: stroke.color,
                    alpha: stroke.alpha,
                    band: stroke.band,
                    gradient: None,
                };
                let shape = Shape::of(element);
                let rect = grown(&element.rect, stroke.band);
                quad(&rect, UV_WHOLE, &paint, &shape, &spin, &mut vertices);
                extend(&mut runs, WHITE, scissor, start..vertices.len());
            }
        }

        // Last, over the background and the image, as Roblox layers a text
        // object. A `UIStroke` in `ApplyStrokeMode.Contextual` on a text
        // object outlines the glyphs like `TextStrokeColor3` does, at its
        // own `Thickness`.
        if let Some(typeset) = &element.text {
            let mut strokes = text::strokes(&typeset.text);
            if let Some(stroke) = &element.stroke {
                if stroke.on_text && stroke.alpha > 0.0 {
                    strokes.push(text::Stroke {
                        color: stroke.color,
                        alpha: stroke.alpha,
                        thickness: stroke.band[1] - stroke.band[0],
                    });
                }
            }
            let paint = Paint {
                color: typeset.text.color,
                alpha: typeset.text.alpha,
                band: FILL,
                gradient,
            };
            text::emit(
                &element.rect,
                typeset,
                &strokes,
                (&paint, &fill, &spin),
                scissor,
                fonts,
                &mut vertices,
                &mut runs,
            );
        }
    }

    (vertices, runs, rows)
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

/// The box a border's bands are grown outward from: the element's own rect
/// for `BorderMode.Outline`, pulled in half a width for `Middle` and a full
/// width for `Inset`, so the bands straddle or sit inside the edge instead.
fn inset(rect: &GuiRect, by: f32) -> GuiRect {
    GuiRect {
        x: rect.x + by,
        y: rect.y + by,
        width: (rect.width - 2.0 * by).max(0.0),
        height: (rect.height - 2.0 * by).max(0.0),
    }
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
