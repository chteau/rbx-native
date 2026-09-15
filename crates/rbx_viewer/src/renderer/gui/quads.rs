//! Turns the resolved rectangles of `crate::scene::gui` into vertices and the
//! draw runs they are issued in, for a target of any pixel size: the screen
//! for a `ScreenGui`, an offscreen canvas for a `BillboardGui`/`SurfaceGui`.
//!
//! Runs are merged only between *consecutive* elements sharing a texture and a
//! scissor: the pass is a painter's algorithm with no depth buffer, so
//! regrouping by texture the way the ribbon passes do would reorder the paint
//! and is not available here.

use std::collections::HashMap;
use std::ops::Range;

use rbx_assets::AssetRef;

use super::pipeline::VertexRaw;
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

        let start = vertices.len();
        if element.background_alpha > 0.0 {
            quad(
                &element.rect,
                [1.0, 1.0],
                element.background,
                element.background_alpha,
                &mut vertices,
            );
            // Roblox ties the outline to `BackgroundTransparency`: a frame
            // with no background shows no border either.
            if let Some((width, color)) = element.border {
                for side in outline(&element.rect, width) {
                    quad(
                        &side,
                        [1.0, 1.0],
                        color,
                        element.background_alpha,
                        &mut vertices,
                    );
                }
            }
        }
        extend(&mut runs, WHITE, scissor, start..vertices.len());

        let Some(image) = &element.image else {
            continue;
        };
        let Some(&texture) = textures.get(&image.asset) else {
            // Never downloaded, or the fetch failed. Roblox draws nothing at
            // all for an image it cannot load, so neither does this.
            continue;
        };
        if image.alpha <= 0.0 {
            continue;
        }
        let start = vertices.len();
        quad(
            &element.rect,
            image.repeat,
            image.tint,
            image.alpha,
            &mut vertices,
        );
        extend(&mut runs, texture, scissor, start..vertices.len());
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

/// Two triangles covering `rect`, the image repeating `repeat` times across it.
fn quad(rect: &GuiRect, repeat: [f32; 2], color: [f32; 3], alpha: f32, into: &mut Vec<VertexRaw>) {
    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    let corner = |position: [f32; 2], uv: [f32; 2]| VertexRaw {
        position,
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
