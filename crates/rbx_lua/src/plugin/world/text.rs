//! `TextService`: text measurement without a font renderer.

use mlua::{AnyUserData, Lua, ObjectLike, Result, Table, UserData, UserDataMethods, Value};

struct Metrics {
    width: f32,
    lines: u32,
}

/// Advance of one character in units of the text size. Roblox does not
/// publish its font metrics, so this is a coarse per-class estimate.
fn advance(c: char) -> f32 {
    if c == ' ' || "ilj.,!|'".contains(c) {
        0.3
    } else if c.is_ascii_uppercase() || c.is_ascii_digit() || c == 'W' || c == 'M' {
        0.6
    } else {
        0.5
    }
}

fn measure(text: &str, size: f32, limit: f32) -> Metrics {
    let mut lines = 1;
    let mut widest = 0.0f32;
    let mut current = 0.0f32;
    for c in text.chars() {
        if c == '\n' {
            lines += 1;
            widest = widest.max(current);
            current = 0.0;
            continue;
        }
        let step = advance(c) * size;
        if limit > 0.0 && current + step > limit && current > 0.0 {
            lines += 1;
            widest = widest.max(current);
            current = 0.0;
        }
        current += step;
    }
    Metrics {
        width: widest.max(current),
        lines,
    }
}

pub(super) struct TextService;

impl UserData for TextService {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "GetTextSize",
            |lua: &Lua, _, (text, size, _font, frame): (String, f32, Value, AnyUserData)| {
                let limit: f32 = frame.get("X")?;
                let metrics = measure(&text, size, limit);
                let vector2: Table = lua.globals().get("Vector2")?;
                let new: mlua::Function = vector2.get("new")?;
                new.call::<Value>((metrics.width, metrics.lines as f32 * size))
            },
        );
        methods.add_method(
            "GetTextBoundsAsync",
            |_, _, _: mlua::MultiValue| -> Result<()> {
                Err(mlua::Error::runtime(
                    "GetTextBoundsAsync is not available in rbx-native",
                ))
            },
        );
    }
}
