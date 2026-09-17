//! Turns one `Painted` image into the quads its own `ScaleType` calls for: a
//! single quad for `Stretch`/`Tile`/`Fit`/`Crop`, nine for `Slice`.
//!
//! This is the one place the source image's own pixel size is known (`size`,
//! read off the atlas's `Slot`) — `crate::scene::gui::plan` and `::layout`
//! only ever see a `UDim2` or a raw pixel count off `ImageRectOffset`/
//! `ImageRectSize`, never the texture itself, so `Fit`/`Crop`'s letterbox and
//! crop math, and the UV side of a sub-rect, all have to live here.
//!
//! Roblox's docs give no rule for combining `Slice` or `Tile` with
//! `ImageRectOffset`/`ImageRectSize`, so both approximate: `Slice` always
//! slices the whole texture, and `Tile` always tiles the whole texture too —
//! a wrapping sampler cannot repeat a sub-rect, only the whole image it
//! addresses (see the comment on the `Tile` arm below).

use super::{quad, Spin};
use crate::scene::{GuiImageScale, GuiPixelRect, GuiRect, Painted};

/// Appends every vertex `image` contributes, already turned by `spin`.
pub(super) fn build(
    rect: &GuiRect,
    image: &Painted,
    texture_size: [f32; 2],
    spin: &Spin,
    into: &mut Vec<super::VertexRaw>,
) {
    match &image.scale {
        GuiImageScale::Stretch => {
            let (uv0, uv1) = sub_rect(image, texture_size);
            quad(rect, uv0, uv1, image.tint, image.alpha, spin, into);
        }
        // ponytail: an exact tiled sub-rect needs a `fract()` over the
        // sub-rect bounds in `gui.wgsl`, since `Repeat` addressing wraps the
        // whole texture, not an arbitrary window into it. Add that if this
        // combination turns out to matter — `Tile` is rare enough combined
        // with a sprite-sheet sub-rect that this viewer just tiles the whole
        // image instead, same as before `ImageRectOffset`/`Size` existed.
        GuiImageScale::Tile => {
            quad(
                rect,
                [0.0, 0.0],
                image.repeat,
                image.tint,
                image.alpha,
                spin,
                into,
            );
        }
        GuiImageScale::Fit => {
            let (drawn, uv0, uv1) = fit(rect, image, texture_size);
            quad(&drawn, uv0, uv1, image.tint, image.alpha, spin, into);
        }
        GuiImageScale::Crop => {
            let (uv0, uv1) = crop(rect, image, texture_size);
            quad(rect, uv0, uv1, image.tint, image.alpha, spin, into);
        }
        GuiImageScale::Slice { center, scale } => {
            slice(rect, image, texture_size, *center, *scale, spin, into);
        }
    }
}

/// The UV bounds `ImageRectOffset`/`ImageRectSize` carve out of the texture —
/// `(0, 0)`..`(1, 1)`, the whole image, where the property is unset (either
/// dimension of `rect_size` being `0`, per `ImageRectSize`'s own docs) or
/// `texture_size` is degenerate.
fn sub_rect(image: &Painted, texture_size: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    let [tex_w, tex_h] = texture_size;
    if image.rect_size[0] <= 0.0 || image.rect_size[1] <= 0.0 || tex_w <= 0.0 || tex_h <= 0.0 {
        return ([0.0, 0.0], [1.0, 1.0]);
    }
    let u0 = image.rect_offset[0] / tex_w;
    let v0 = image.rect_offset[1] / tex_h;
    let u1 = u0 + image.rect_size[0] / tex_w;
    let v1 = v0 + image.rect_size[1] / tex_h;
    ([u0, v0], [u1, v1])
}

/// `ScaleType.Fit`: the drawn quad shrinks to the sub-image's own aspect
/// ratio instead of the box being cropped, with letterbox bars left to
/// whatever `rect`'s own background already painted — Roblox draws no bars
/// of its own either.
fn fit(rect: &GuiRect, image: &Painted, texture_size: [f32; 2]) -> (GuiRect, [f32; 2], [f32; 2]) {
    let (uv0, uv1) = sub_rect(image, texture_size);
    let sub = [
        (uv1[0] - uv0[0]) * texture_size[0],
        (uv1[1] - uv0[1]) * texture_size[1],
    ];
    if sub[0] <= 0.0 || sub[1] <= 0.0 {
        return (*rect, uv0, uv1);
    }
    let scale = (rect.width / sub[0]).min(rect.height / sub[1]);
    let width = sub[0] * scale;
    let height = sub[1] * scale;
    let drawn = GuiRect {
        x: rect.x + (rect.width - width) * 0.5,
        y: rect.y + (rect.height - height) * 0.5,
        width,
        height,
    };
    (drawn, uv0, uv1)
}

/// `ScaleType.Crop`: the sub-image fills the box at its own aspect ratio,
/// centred, with whichever axis overflows cut from the UVs rather than drawn
/// past the edge.
fn crop(rect: &GuiRect, image: &Painted, texture_size: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    let (uv0, uv1) = sub_rect(image, texture_size);
    let sub = [
        (uv1[0] - uv0[0]) * texture_size[0],
        (uv1[1] - uv0[1]) * texture_size[1],
    ];
    if sub[0] <= 0.0 || sub[1] <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return (uv0, uv1);
    }
    let scale = (rect.width / sub[0]).max(rect.height / sub[1]);
    let visible_u = (rect.width / scale / sub[0]).min(1.0);
    let visible_v = (rect.height / scale / sub[1]).min(1.0);
    let cu0 = uv0[0] + (uv1[0] - uv0[0]) * (1.0 - visible_u) * 0.5;
    let cv0 = uv0[1] + (uv1[1] - uv0[1]) * (1.0 - visible_v) * 0.5;
    (
        [cu0, cv0],
        [
            cu0 + (uv1[0] - uv0[0]) * visible_u,
            cv0 + (uv1[1] - uv0[1]) * visible_v,
        ],
    )
}

/// `ScaleType.Slice`: nine quads — four fixed-size corners, four edges that
/// stretch along one axis, and a centre stretching along both — from
/// `SliceCenter`'s boundaries in the texture and `SliceScale`'s multiplier on
/// how big they draw.
fn slice(
    rect: &GuiRect,
    image: &Painted,
    texture_size: [f32; 2],
    center: Option<GuiPixelRect>,
    scale: f32,
    spin: &Spin,
    into: &mut Vec<super::VertexRaw>,
) {
    let [tex_w, tex_h] = texture_size;
    if tex_w <= 0.0 || tex_h <= 0.0 {
        quad(
            rect,
            [0.0, 0.0],
            [1.0, 1.0],
            image.tint,
            image.alpha,
            spin,
            into,
        );
        return;
    }
    // The docs never state Roblox's own default for an unset `SliceCenter`;
    // the whole image makes every border zero pixels wide, which is the same
    // as `Stretch` rather than a guessed border.
    let center = center.unwrap_or(GuiPixelRect {
        min: [0.0, 0.0],
        max: [tex_w, tex_h],
    });
    let cx0 = center.min[0].clamp(0.0, tex_w);
    let cy0 = center.min[1].clamp(0.0, tex_h);
    let cx1 = center.max[0].clamp(cx0, tex_w);
    let cy1 = center.max[1].clamp(cy0, tex_h);

    // Border thickness in screen pixels: `SliceScale` grows them the same way
    // a higher-resolution source texture would, per its own docs.
    let left = (cx0 * scale).max(0.0);
    let top = (cy0 * scale).max(0.0);
    let right = ((tex_w - cx1) * scale).max(0.0);
    let bottom = ((tex_h - cy1) * scale).max(0.0);
    // Roblox's docs don't say what happens once the borders overlap; shrunk
    // proportionally, they at least never invert the centre they surround.
    let (left, right) = shrink_to_fit(left, right, rect.width);
    let (top, bottom) = shrink_to_fit(top, bottom, rect.height);

    let u = [0.0, cx0 / tex_w, cx1 / tex_w, 1.0];
    let v = [0.0, cy0 / tex_h, cy1 / tex_h, 1.0];
    let x = [
        rect.x,
        rect.x + left,
        rect.x + rect.width - right,
        rect.x + rect.width,
    ];
    let y = [
        rect.y,
        rect.y + top,
        rect.y + rect.height - bottom,
        rect.y + rect.height,
    ];

    for row in 0..3 {
        for col in 0..3 {
            let piece = GuiRect {
                x: x[col],
                y: y[row],
                width: x[col + 1] - x[col],
                height: y[row + 1] - y[row],
            };
            if piece.width <= 0.0 || piece.height <= 0.0 {
                continue;
            }
            quad(
                &piece,
                [u[col], v[row]],
                [u[col + 1], v[row + 1]],
                image.tint,
                image.alpha,
                spin,
                into,
            );
        }
    }
}

/// Scales a pair of opposing borders down together, proportionally, so they
/// never claim more of `extent` than it has to give.
fn shrink_to_fit(a: f32, b: f32, extent: f32) -> (f32, f32) {
    let total = a + b;
    if total > extent && total > 0.0 {
        let factor = extent / total;
        (a * factor, b * factor)
    } else {
        (a, b)
    }
}

#[cfg(test)]
mod tests;
