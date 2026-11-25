//! How a colour lands on a pixel: the `ImageCombineType` blends and the
//! `ImageAlphaType` locks, with the arithmetic the EditableApi and
//! EditableImageDraw goldens come out of (single precision, mostly
//! truncating; `Multiply` rounds up; `NormalMapBlend` works on normals
//! decoded as `(c - 128) / 127`).

use mlua::{Result, Value};

use crate::datatypes::color3::LuaColor3;
use crate::lua_enum::LuaEnumItem;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Combine {
    SourceOver,
    Overwrite,
    AlphaBlend,
    Add,
    Multiply,
    Subtract,
    NormalMap,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Lock {
    None,
    Alpha,
    Color,
}

fn item_name(value: &Value) -> Result<Option<String>> {
    Ok(match value {
        Value::UserData(data) if data.is::<LuaEnumItem>() => {
            Some(data.borrow::<LuaEnumItem>()?.item_name().to_owned())
        }
        Value::String(name) => Some(name.to_str()?.to_owned()),
        _ => None,
    })
}

/// A combine type argument; `method` names the call for Roblox's error
/// about the types it doesn't draw with yet.
pub(crate) fn combine(value: &Value, default: Combine, method: &str) -> Result<Combine> {
    if value.is_nil() {
        return Ok(default);
    }
    let Some(name) = item_name(value)? else {
        return Err(mlua::Error::runtime(format!(
            "Unable to cast {} to ImageCombineType",
            value.type_name()
        )));
    };
    Ok(match name.as_str() {
        "BlendSourceOver" => Combine::SourceOver,
        "Overwrite" => Combine::Overwrite,
        "AlphaBlend" => Combine::AlphaBlend,
        "Add" => Combine::Add,
        "Multiply" => Combine::Multiply,
        "Subtract" => Combine::Subtract,
        "NormalMapBlend" => Combine::NormalMap,
        _ => {
            return Err(mlua::Error::runtime(format!(
                "EditableImage:{method}: the requested Enum.ImageCombineType is not yet supported"
            )))
        }
    })
}

pub(crate) fn lock(value: &Value) -> Result<Lock> {
    Ok(match item_name(value)?.as_deref() {
        Some("LockCanvasAlpha") => Lock::Alpha,
        Some("LockCanvasColor") => Lock::Color,
        _ => Lock::None,
    })
}

/// A draw's transparency as the alpha byte Roblox keeps.
pub(crate) fn alpha_of(transparency: f32) -> f32 {
    ((1.0 - transparency).clamp(0.0, 1.0) * 256.0)
        .floor()
        .min(255.0)
        / 255.0
}

pub(crate) fn color(value: &Value) -> Result<[f32; 3]> {
    match value {
        Value::UserData(data) if data.is::<LuaColor3>() => {
            let c = data.borrow::<LuaColor3>()?.0;
            Ok([c.r, c.g, c.b].map(|c| c.clamp(0.0, 1.0) * 255.0))
        }
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to Color3",
            other.type_name()
        ))),
    }
}

/// A colour on its way onto pixels: channels 0..255, alpha 0..1.
#[derive(Clone, Copy)]
pub(crate) struct Paint {
    pub(crate) rgb: [f32; 3],
    pub(crate) alpha: f32,
    pub(crate) mode: Combine,
    pub(crate) lock: Lock,
}

fn normal_in(c: f32) -> f32 {
    (c - 128.0) / 127.0
}

impl Paint {
    pub(crate) fn texel(texel: &[u8], mode: Combine, lock: Lock) -> Paint {
        Paint {
            rgb: [texel[0], texel[1], texel[2]].map(f32::from),
            alpha: f32::from(texel[3]) / 255.0,
            mode,
            lock,
        }
    }

    /// Lays this paint onto one RGBA pixel, `coverage` of it.
    pub(crate) fn apply(&self, pixel: &mut [u8], coverage: f32) {
        let a = self.alpha * coverage;
        let d = [pixel[0], pixel[1], pixel[2]].map(f32::from);
        let d_alpha = f32::from(pixel[3]);
        let s_alpha = self.alpha * 255.0;
        let out: [f32; 4] = match self.mode {
            // An anti-aliased edge fades toward the source (the
            // EditableImageDraw golden's `circle_soft_sum`).
            Combine::Overwrite => {
                let [r, g, b] = std::array::from_fn(|i| d[i] + (self.rgb[i] - d[i]) * coverage);
                [r, g, b, d_alpha + (s_alpha - d_alpha) * coverage]
            }
            Combine::AlphaBlend => {
                let [r, g, b] = std::array::from_fn(|i| self.rgb[i] * a + d[i] * (1.0 - a));
                [r, g, b, s_alpha * a + d_alpha * (1.0 - a)]
            }
            Combine::SourceOver => {
                let da = d_alpha / 255.0;
                let total = a + da * (1.0 - a);
                let [r, g, b] = std::array::from_fn(|i| match total > 0.0 {
                    true => (self.rgb[i] * a + d[i] * da * (1.0 - a)) / total,
                    false => 0.0,
                });
                [r, g, b, 255.0 * total]
            }
            Combine::Add => {
                let [r, g, b] = std::array::from_fn(|i| d[i] + (self.rgb[i] * a).floor());
                [r, g, b, d_alpha + 255.0 * a]
            }
            Combine::Subtract => {
                let [r, g, b] = std::array::from_fn(|i| d[i] - (self.rgb[i] * a).floor());
                [r, g, b, d_alpha - 255.0 * a]
            }
            Combine::Multiply => {
                let s = [self.rgb[0], self.rgb[1], self.rgb[2], s_alpha];
                let dd = [d[0], d[1], d[2], d_alpha];
                std::array::from_fn(|i| dd[i] + ((s[i] * dd[i] / 255.0).ceil() - dd[i]) * coverage)
            }
            Combine::NormalMap => {
                let v: [f32; 3] = std::array::from_fn(|i| {
                    let (dn, sn) = (normal_in(d[i]), normal_in(self.rgb[i]));
                    dn + (sn - dn) * a
                });
                let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                let [r, g, b] = v.map(|c| match length > 0.0 {
                    true => (c / length * 127.0 + 128.0).round(),
                    false => 128.0,
                });
                [r, g, b, d_alpha + (s_alpha - d_alpha) * a]
            }
        };
        let old = [pixel[0], pixel[1], pixel[2], pixel[3]];
        for (byte, value) in pixel.iter_mut().zip(out) {
            *byte = value.clamp(0.0, 255.0) as u8;
        }
        match self.lock {
            Lock::Alpha => pixel[3] = old[3],
            Lock::Color => pixel[..3].copy_from_slice(&old[..3]),
            Lock::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(dest: [u8; 4], src: [u8; 4], mode: Combine) -> [u8; 4] {
        let mut pixel = dest;
        Paint::texel(&src, mode, Lock::None).apply(&mut pixel, 1.0);
        pixel
    }

    #[test]
    fn image_blends_match_roblox() {
        // EditableImageDraw's image_* rows.
        let (dest, src) = ([100, 150, 200, 128], [200, 100, 50, 192]);
        assert_eq!(draw(dest, src, Combine::AlphaBlend), [175, 112, 87, 176]);
        assert_eq!(draw(dest, src, Combine::SourceOver), [185, 107, 71, 223]);
        assert_eq!(draw(dest, src, Combine::Add), [250, 225, 237, 255]);
        assert_eq!(draw(dest, src, Combine::Multiply), [79, 59, 40, 97]);
        assert_eq!(draw(dest, src, Combine::Subtract), [0, 75, 163, 0]);
    }

    #[test]
    fn normal_map_blend_renormalizes() {
        assert_eq!(
            draw(
                [180, 100, 240, 255],
                [90, 160, 220, 255],
                Combine::NormalMap
            ),
            [82, 167, 240, 255]
        );
        assert_eq!(
            draw(
                [128, 128, 255, 255],
                [200, 128, 230, 255],
                Combine::NormalMap
            ),
            [201, 128, 232, 255]
        );
        assert_eq!(
            draw(
                [180, 100, 240, 255],
                [90, 160, 220, 128],
                Combine::NormalMap
            ),
            [136, 131, 255, 191]
        );
    }
}
