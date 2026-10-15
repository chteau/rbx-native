//! `Color3` datatype.
//!
//! HSV values use Roblox's convention: hue, saturation and value are all in `0..=1`,
//! and hue wraps around so `fromHSV(1, s, v)` equals `fromHSV(0, s, v)`.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::Color3Data;

use super::from_userdata;

const HEX_ERROR: &str = "Unable to convert characters to hex value";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaColor3(pub(crate) Color3Data);

impl LuaColor3 {
    pub(crate) fn new(r: f32, g: f32, b: f32) -> Self {
        LuaColor3(Color3Data { r, g, b })
    }
}

impl UserData for LuaColor3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("R", |_, this| Ok(this.0.r));
        fields.add_field_method_get("G", |_, this| Ok(this.0.g));
        fields.add_field_method_get("B", |_, this| Ok(this.0.b));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("ToHex", |_, this, ()| {
            let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            Ok(format!(
                "{:02x}{:02x}{:02x}",
                channel(this.0.r),
                channel(this.0.g),
                channel(this.0.b)
            ))
        });
        methods.add_method("ToHSV", |_, this, ()| Ok(rgb_to_hsv(this.0)));
        methods.add_method("Lerp", |_, this, (goal, alpha): (LuaColor3, f32)| {
            let lerp = |a: f32, b: f32| a + (b - a) * alpha;
            Ok(LuaColor3::new(
                lerp(this.0.r, goal.0.r),
                lerp(this.0.g, goal.0.g),
                lerp(this.0.b, goal.0.b),
            ))
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaColor3| {
            Ok(*this == other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}, {}", this.0.r, this.0.g, this.0.b))
        });
    }
}

impl mlua::FromLua for LuaColor3 {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "Color3")
    }
}

/// Converts HSV to RGB. Saturation and value are clamped to `0..=1`; hue is taken modulo 1.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color3Data {
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    if s == 0.0 {
        return Color3Data { r: v, g: v, b: v };
    }
    let h6 = h.rem_euclid(1.0) * 6.0;
    let sector = h6.floor();
    let f = h6 - sector;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match sector as u32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Color3Data { r, g, b }
}

/// Converts RGB to `(hue, saturation, value)`, each in `0..=1`. Hue is 0 for greys.
fn rgb_to_hsv(c: Color3Data) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let delta = max - min;
    let s = if max <= 0.0 { 0.0 } else { delta / max };
    let hue = if delta == 0.0 {
        0.0
    } else if max == c.r {
        ((c.g - c.b) / delta).rem_euclid(6.0)
    } else if max == c.g {
        (c.b - c.r) / delta + 2.0
    } else {
        (c.r - c.g) / delta + 4.0
    };
    (hue.rem_euclid(6.0) / 6.0, s, max)
}

/// Parses `#rrggbb`, `rrggbb`, `#rgb` or `rgb`, with or without the leading `#`.
fn parse_hex(text: &str) -> Option<Color3Data> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    let values = digits
        .chars()
        .map(|c| c.to_digit(16))
        .collect::<Option<Vec<u32>>>()?;
    let (r, g, b) = match *values.as_slice() {
        [r, g, b] => (r * 17, g * 17, b * 17),
        [r1, r2, g1, g2, b1, b2] => (r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2),
        _ => return None,
    };
    Some(Color3Data {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
    })
}

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, (r, g, b): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaColor3::new(
                r.unwrap_or_default(),
                g.unwrap_or_default(),
                b.unwrap_or_default(),
            ))
        })?,
    )?;
    table.set(
        "fromRGB",
        lua.create_function(|_, (r, g, b): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaColor3::new(
                r.unwrap_or_default() / 255.0,
                g.unwrap_or_default() / 255.0,
                b.unwrap_or_default() / 255.0,
            ))
        })?,
    )?;
    table.set(
        "fromHSV",
        lua.create_function(|_, (h, s, v): (Option<f32>, Option<f32>, Option<f32>)| {
            let color = hsv_to_rgb(
                h.unwrap_or_default(),
                s.unwrap_or_default(),
                v.unwrap_or_default(),
            );
            Ok(LuaColor3(color))
        })?,
    )?;
    table.set(
        "fromHex",
        lua.create_function(|_, text: String| {
            parse_hex(&text)
                .map(LuaColor3)
                .ok_or_else(|| mlua::Error::runtime(HEX_ERROR))
        })?,
    )?;
    table.set(
        "toHSV",
        lua.create_function(|_, color: LuaColor3| Ok(rgb_to_hsv(color.0)))?,
    )?;
    Ok(table)
}
