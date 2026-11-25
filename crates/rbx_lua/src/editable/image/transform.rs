//! `EditableImage:DrawImageTransformed`: an image scaled and rotated
//! (degrees, clockwise on screen) about its pivot, which lands on
//! `position`. Each destination pixel's centre is mapped back into the
//! source and sampled there, nearest or bilinear (rounded, edges clamped);
//! the EditableImageDraw golden's `tx_*` rows. Options it doesn't know,
//! a combine type it can't read and a zero scale are not errors.

use mlua::{AnyUserData, Result, Table, UserDataMethods, Value};
use rbx_dom::editable::lock;

use super::blend::{combine, Combine, Lock, Paint};
use super::{vector2, LuaEditableImage};

fn sample(rgba: &[u8], (w, h): (usize, usize), (x, y): (f32, f32), smooth: bool) -> [u8; 4] {
    let texel = |x: i64, y: i64| -> [f32; 4] {
        let (x, y) = (
            x.clamp(0, w as i64 - 1) as usize,
            y.clamp(0, h as i64 - 1) as usize,
        );
        let i = (y * w + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]].map(f32::from)
    };
    if !smooth {
        return texel(x.floor() as i64, y.floor() as i64).map(|c| c as u8);
    }
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (
        texel(x0, y0),
        texel(x0 + 1, y0),
        texel(x0, y0 + 1),
        texel(x0 + 1, y0 + 1),
    );
    std::array::from_fn(|i| {
        let top = a[i] + (b[i] - a[i]) * tx;
        let bottom = c[i] + (d[i] - c[i]) * tx;
        (top + (bottom - top) * ty).round().clamp(0.0, 255.0) as u8
    })
}

pub(super) fn add<M: UserDataMethods<LuaEditableImage>>(methods: &mut M) {
    methods.add_method(
        "DrawImageTransformed",
        |_, this, (position, scale, rotation, image, options): (Value, Value, f32, AnyUserData, Option<Table>)| {
            let (px, py) = vector2(&position, "position")?;
            let (sx, sy) = vector2(&scale, "scale")?;
            let source = lock(&image.borrow::<LuaEditableImage>()?.pixels).clone();
            let (w, h) = (source.width as f32, source.height as f32);
            let get = |key: &str| -> Result<Value> {
                match &options {
                    Some(options) => options.get(key),
                    None => Ok(Value::Nil),
                }
            };
            let mode = combine(&get("CombineType")?, Combine::AlphaBlend, "DrawImageTransformed").unwrap_or(Combine::AlphaBlend);
            let smooth = !matches!(&get("SamplingMode")?, Value::UserData(d)
                if d.borrow::<crate::lua_enum::LuaEnumItem>().is_ok_and(|i| i.item_name() == "Pixelated"));
            let pivot = match get("PivotPoint")? {
                Value::Nil => (w / 2.0, h / 2.0),
                p => vector2(&p, "PivotPoint")?,
            };
            if sx == 0.0 || sy == 0.0 || source.width == 0 || source.height == 0 {
                return Ok(());
            }
            let (sin, cos) = rotation.to_radians().sin_cos();
            let forward = |(x, y): (f32, f32)| {
                let (x, y) = ((x - pivot.0) * sx, (y - pivot.1) * sy);
                (px + x * cos - y * sin, py + x * sin + y * cos)
            };
            let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(forward);
            let min = corners.iter().fold((f32::INFINITY, f32::INFINITY), |m, c| (m.0.min(c.0), m.1.min(c.1)));
            let max = corners.iter().fold((f32::NEG_INFINITY, f32::NEG_INFINITY), |m, c| (m.0.max(c.0), m.1.max(c.1)));
            this.edit(|pixels| {
                let (dw, dh) = (pixels.width as i64, pixels.height as i64);
                for y in (min.1.floor() as i64).max(0)..=(max.1.ceil() as i64).min(dh - 1) {
                    for x in (min.0.floor() as i64).max(0)..=(max.0.ceil() as i64).min(dw - 1) {
                        let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
                        let (ux, uy) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                        let (ux, uy) = (pivot.0 + ux / sx, pivot.1 + uy / sy);
                        if !(0.0..w).contains(&ux) || !(0.0..h).contains(&uy) {
                            continue;
                        }
                        let texel = sample(&source.rgba, (source.width as usize, source.height as usize), (ux, uy), smooth);
                        let i = ((y * dw + x) * 4) as usize;
                        match mode {
                            Combine::Overwrite => pixels.rgba[i..i + 4].copy_from_slice(&texel),
                            mode => Paint::texel(&texel, mode, Lock::None).apply(&mut pixels.rgba[i..i + 4], 1.0),
                        }
                    }
                }
            });
            Ok(())
        },
    );
}
