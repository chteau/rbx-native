//! `AssetService:CreateEditableImage` and the `EditableImage` it makes:
//! pixels read, written and drawn with the `ImageCombineType` blends. The
//! pixels live in `rbx_dom::editable`, where a renderer finds them through
//! the `Content::Object` an `ImageLabel.ImageContent` holds.
//!
//! The arithmetic is single precision and truncates, which is what the
//! EditableApi golden's numbers come out of: half a pixel of coverage is
//! 127, and red under a half-transparent blue `BlendSourceOver` is 126.

use std::sync::{Arc, Mutex};

use mlua::{
    AnyUserData, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
};
use rbx_dom::editable::{self, lock, Editable, Pixels};
use rbx_dom::Ref;

use crate::datatypes::color3::LuaColor3;
use crate::datatypes::content::{LuaContent, Source};
use crate::datatypes::misc::LuaVector2;
use crate::lua_enum::LuaEnumItem;

const DEFAULT_SIZE: u32 = 512;
const MAX_SIZE: u32 = 1024;

#[derive(Clone)]
pub(crate) struct LuaEditableImage {
    referent: Ref,
    pixels: Arc<Mutex<Pixels>>,
}

/// Every editable a VM published, withdrawn when the VM goes: a run that
/// ends must not leave its canvases behind for the next one.
#[derive(Default)]
struct Owned(Vec<Ref>);

impl Drop for Owned {
    fn drop(&mut self) {
        for referent in &self.0 {
            editable::withdraw(*referent);
        }
    }
}

/// Publishes `editable` and ties its life to `lua`'s.
pub(super) fn publish(lua: &Lua, editable: Editable) -> Ref {
    let referent = editable::publish(editable);
    match lua.app_data_mut::<Owned>() {
        Some(mut owned) => owned.0.push(referent),
        None => {
            lua.set_app_data(Owned(vec![referent]));
        }
    }
    referent
}

/// The referent an `EditableImage`/`EditableMesh` handle is published under,
/// what a `Content` property stores.
pub(crate) fn editable_ref(data: &AnyUserData) -> Option<Ref> {
    if let Ok(image) = data.borrow::<LuaEditableImage>() {
        return Some(image.referent);
    }
    super::meshes::referent(data)
}

/// A `Content` holding a handle on the editable `referent` names, for a
/// `Content` property read back; `None` once it is gone.
pub(crate) fn editable_content(lua: &Lua, referent: Ref) -> Result<Option<Value>> {
    let object = match editable::get(referent) {
        Some(Editable::Image(pixels)) => {
            lua.create_userdata(LuaEditableImage { referent, pixels })?
        }
        Some(Editable::Mesh(mesh)) => super::meshes::handle(lua, referent, mesh)?,
        None => return Ok(None),
    };
    lua.create_userdata(LuaContent(Source::Object(Value::UserData(object))))
        .map(|content| Some(Value::UserData(content)))
}

fn vector2(value: &Value, what: &str) -> Result<(f32, f32)> {
    match value {
        Value::UserData(data) if data.is::<LuaVector2>() => {
            let v = data.borrow::<LuaVector2>()?.0;
            Ok((v.x, v.y))
        }
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to Vector2 ({what})",
            other.type_name()
        ))),
    }
}

fn checked_size(method: &str, (x, y): (f32, f32)) -> Result<(u32, u32)> {
    if x <= 0.0 || y <= 0.0 {
        return Err(mlua::Error::runtime(format!(
            "{method} expects size.X and size.Y to be positive"
        )));
    }
    if x > MAX_SIZE as f32 || y > MAX_SIZE as f32 {
        return Err(mlua::Error::runtime(format!(
            "{method} expects size.X and size.Y to be less than {MAX_SIZE}"
        )));
    }
    Ok((x as u32, y as u32))
}

fn new_image(lua: &Lua, (width, height): (u32, u32), rgba: Vec<u8>) -> LuaEditableImage {
    let pixels = Arc::new(Mutex::new(Pixels {
        width,
        height,
        rgba,
        version: 0,
    }));
    let referent = publish(lua, Editable::Image(pixels.clone()));
    LuaEditableImage { referent, pixels }
}

#[derive(Clone, Copy, PartialEq)]
enum Combine {
    SourceOver,
    Overwrite,
    AlphaBlend,
    Add,
    Multiply,
    Subtract,
}

fn combine(value: &Value) -> Result<Combine> {
    let name = match value {
        Value::Nil => return Ok(Combine::SourceOver),
        Value::UserData(data) if data.is::<LuaEnumItem>() => {
            data.borrow::<LuaEnumItem>()?.item_name().to_owned()
        }
        Value::String(name) => name.to_str()?.to_owned(),
        other => {
            return Err(mlua::Error::runtime(format!(
                "Unable to cast {} to ImageCombineType",
                other.type_name()
            )))
        }
    };
    Ok(match name.as_str() {
        "Overwrite" => Combine::Overwrite,
        "AlphaBlend" => Combine::AlphaBlend,
        "Add" => Combine::Add,
        "Multiply" => Combine::Multiply,
        "Subtract" => Combine::Subtract,
        _ => Combine::SourceOver,
    })
}

/// A colour on its way onto pixels: channels 0..255, alpha 0..1 already
/// quantized to a byte, the way Roblox stores a draw's transparency.
#[derive(Clone, Copy)]
struct Brush {
    rgb: [f32; 3],
    alpha: f32,
    mode: Combine,
}

impl Brush {
    fn new(color: &Value, transparency: f32, mode: &Value) -> Result<Brush> {
        let color = match color {
            Value::UserData(data) if data.is::<LuaColor3>() => data.borrow::<LuaColor3>()?.0,
            other => {
                return Err(mlua::Error::runtime(format!(
                    "Unable to cast {} to Color3",
                    other.type_name()
                )))
            }
        };
        Ok(Brush {
            rgb: [color.r, color.g, color.b].map(|c| c.clamp(0.0, 1.0) * 255.0),
            alpha: ((1.0 - transparency).clamp(0.0, 1.0) * 255.0).round() / 255.0,
            mode: combine(mode)?,
        })
    }

    /// Lays this brush onto one RGBA pixel, `coverage` of it.
    fn paint(&self, pixel: &mut [u8], coverage: f32) {
        let a = self.alpha * coverage;
        let d = [pixel[0], pixel[1], pixel[2]].map(f32::from);
        let d_alpha = f32::from(pixel[3]);
        let channel = |i: usize| -> f32 {
            let (s, d) = (self.rgb[i], d[i]);
            match self.mode {
                Combine::Overwrite => s * coverage,
                Combine::AlphaBlend => s * a + d * (1.0 - a),
                Combine::Add => d + s * a,
                Combine::Subtract => d - s * a,
                Combine::Multiply => d * (s / 255.0 * a + (1.0 - a)),
                Combine::SourceOver => {
                    let da = d_alpha / 255.0;
                    let out = a + da * (1.0 - a);
                    match out > 0.0 {
                        true => (s * a + d * da * (1.0 - a)) / out,
                        false => 0.0,
                    }
                }
            }
        };
        let rgb = [channel(0), channel(1), channel(2)];
        let alpha = match self.mode {
            Combine::Overwrite => 255.0 * a,
            Combine::AlphaBlend => 255.0 * self.alpha * a + d_alpha * (1.0 - a),
            Combine::SourceOver => 255.0 * (a + d_alpha / 255.0 * (1.0 - a)),
            Combine::Add => d_alpha + 255.0 * a,
            Combine::Multiply | Combine::Subtract => d_alpha,
        };
        for (byte, value) in pixel.iter_mut().zip(rgb.into_iter().chain([alpha])) {
            *byte = value.clamp(0.0, 255.0) as u8;
        }
    }
}

impl LuaEditableImage {
    /// Edits the pixels and tells every reader they changed.
    fn edit(&self, f: impl FnOnce(&mut Pixels)) {
        let mut pixels = lock(&self.pixels);
        f(&mut pixels);
        pixels.version += 1;
        editable::touch();
    }

    /// Paints every pixel whose integer centre `coverage` gives a share of,
    /// within the box `x0..=x1`, `y0..=y1`.
    fn draw(
        &self,
        brush: Brush,
        (x0, y0, x1, y1): (f32, f32, f32, f32),
        coverage: impl Fn(f32, f32) -> f32,
    ) {
        self.edit(|pixels| {
            let (w, h) = (pixels.width as i64, pixels.height as i64);
            for y in (y0.floor() as i64).max(0)..=(y1.ceil() as i64).min(h - 1) {
                for x in (x0.floor() as i64).max(0)..=(x1.ceil() as i64).min(w - 1) {
                    let share = coverage(x as f32, y as f32);
                    if share > 0.0 {
                        let i = ((y * w + x) * 4) as usize;
                        brush.paint(&mut pixels.rgba[i..i + 4], share);
                    }
                }
            }
        });
    }

    /// The pixel box `position`/`size` names, or the bounds error `method`
    /// reports.
    fn region(
        &self,
        method: &str,
        position: &Value,
        size: &Value,
    ) -> Result<(usize, usize, usize, usize)> {
        let (x, y) = vector2(position, "position")?;
        let (w, h) = vector2(size, "size")?;
        let pixels = lock(&self.pixels);
        let (x, y, w, h) = (x as i64, y as i64, w as i64, h as i64);
        if x < 0
            || y < 0
            || w < 0
            || h < 0
            || x + w > pixels.width as i64
            || y + h > pixels.height as i64
        {
            return Err(mlua::Error::runtime(format!(
                "EditableImage:{method} expects position + size to be within EditableImage bounds"
            )));
        }
        Ok((x as usize, y as usize, w as usize, h as usize))
    }
}

impl UserData for LuaEditableImage {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Object");
        fields.add_field_method_get("Size", |_, this| {
            let pixels = lock(&this.pixels);
            Ok(LuaVector2::new(pixels.width as f32, pixels.height as f32))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("EditableImage"));
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            Ok(editable_ref(&other) == Some(this.referent))
        });
        methods.add_method(
            "ReadPixelsBuffer",
            |lua, this, (position, size): (Value, Value)| {
                let (x, y, w, h) = this.region("ReadPixelsBuffer", &position, &size)?;
                let pixels = lock(&this.pixels);
                let stride = pixels.width as usize * 4;
                let mut out = Vec::with_capacity(w * h * 4);
                for row in y..y + h {
                    out.extend_from_slice(
                        &pixels.rgba[row * stride + x * 4..row * stride + (x + w) * 4],
                    );
                }
                lua.create_buffer(out)
            },
        );
        methods.add_method(
            "WritePixelsBuffer",
            |_, this, (position, size, buffer): (Value, Value, mlua::Buffer)| {
                let (x, y, w, h) = this.region("WritePixelsBuffer", &position, &size)?;
                let bytes = buffer.to_vec();
                if bytes.len() != w * h * 4 {
                    return Err(mlua::Error::runtime(
                        "EditableImage:WritePixelsBuffer expects buffer length to equal size.X * size.Y * 4 bytes",
                    ));
                }
                this.edit(|pixels| {
                    let stride = pixels.width as usize * 4;
                    for (row, line) in bytes.chunks(w * 4).enumerate() {
                        let at = (y + row) * stride + x * 4;
                        pixels.rgba[at..at + w * 4].copy_from_slice(line);
                    }
                });
                Ok(())
            },
        );
        methods.add_method(
            "DrawRectangle",
            |_, this, (position, size, color, transparency, mode): (Value, Value, Value, f32, Value)| {
                let (x, y) = vector2(&position, "position")?;
                let (w, h) = vector2(&size, "size")?;
                let (x0, y0, x1, y1) = (x.floor(), y.floor(), (x + w).floor(), (y + h).floor());
                let brush = Brush::new(&color, transparency, &mode)?;
                this.draw(brush, (x0, y0, x1 - 1.0, y1 - 1.0), |px, py| {
                    f32::from(px >= x0 && px < x1 && py >= y0 && py < y1)
                });
                Ok(())
            },
        );
        methods.add_method(
            "DrawCircle",
            |_, this, (center, radius, color, transparency, mode): (Value, f32, Value, f32, Value)| {
                let (cx, cy) = vector2(&center, "center")?;
                let brush = Brush::new(&color, transparency, &mode)?;
                let reach = radius + 1.0;
                this.draw(brush, (cx - reach, cy - reach, cx + reach, cy + reach), |px, py| {
                    // Smoothed over the last pixel inside the rim.
                    let t = (radius - (px - cx).hypot(py - cy)).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                });
                Ok(())
            },
        );
        methods.add_method(
            "DrawLine",
            |_, this, (p0, p1, color, transparency, mode): (Value, Value, Value, f32, Value)| {
                let (ax, ay) = vector2(&p0, "p1")?;
                let (bx, by) = vector2(&p1, "p2")?;
                let brush = Brush::new(&color, transparency, &mode)?;
                let (dx, dy) = (bx - ax, by - ay);
                let length = dx.hypot(dy);
                let bounds = (
                    ax.min(bx) - 1.0,
                    ay.min(by) - 1.0,
                    ax.max(bx) + 1.0,
                    ay.max(by) + 1.0,
                );
                this.draw(brush, bounds, |px, py| {
                    let (rx, ry) = (px - ax, py - ay);
                    if length == 0.0 {
                        return (1.0 - rx.hypot(ry)).clamp(0.0, 1.0);
                    }
                    // One pixel wide, each end covering half of its own pixel.
                    let along = (rx * dx + ry * dy) / length;
                    let across = (rx * dy - ry * dx).abs() / length;
                    (1.0 - across).clamp(0.0, 1.0)
                        * (along.min(length - along) + 0.5).clamp(0.0, 1.0)
                });
                Ok(())
            },
        );
        methods.add_method(
            "DrawImage",
            |_, this, (position, image, mode): (Value, AnyUserData, Value)| {
                let (x, y) = vector2(&position, "position")?;
                let source = image.borrow::<LuaEditableImage>()?.pixels.clone();
                let source = lock(&source).clone();
                let mode = combine(&mode)?;
                let (ox, oy) = (x.floor() as i64, y.floor() as i64);
                this.edit(|pixels| {
                    let (w, h) = (pixels.width as i64, pixels.height as i64);
                    for sy in 0..source.height as i64 {
                        for sx in 0..source.width as i64 {
                            let (dx, dy) = (ox + sx, oy + sy);
                            if dx < 0 || dy < 0 || dx >= w || dy >= h {
                                continue;
                            }
                            let s = ((sy * source.width as i64 + sx) * 4) as usize;
                            let d = ((dy * w + dx) * 4) as usize;
                            let texel = &source.rgba[s..s + 4];
                            if mode == Combine::Overwrite {
                                pixels.rgba[d..d + 4].copy_from_slice(texel);
                            } else {
                                let brush = Brush {
                                    rgb: [texel[0], texel[1], texel[2]].map(f32::from),
                                    alpha: f32::from(texel[3]) / 255.0,
                                    mode,
                                };
                                brush.paint(&mut pixels.rgba[d..d + 4], 1.0);
                            }
                        }
                    }
                });
                Ok(())
            },
        );
        methods.add_method("Destroy", |_, this, ()| {
            editable::withdraw(this.referent);
            Ok(())
        });
    }
}

fn options_size(options: &Option<Table>) -> Result<Option<(f32, f32)>> {
    let Some(options) = options else {
        return Ok(None);
    };
    let size: Value = options.get("Size")?;
    match size {
        Value::Nil => Ok(None),
        size => vector2(&size, "Size").map(Some),
    }
}

pub(super) fn member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    let f = match key {
        "CreateEditableImage" => {
            lua.create_function(|lua, (_this, options): (Value, Option<Table>)| {
                let size = match options_size(&options)? {
                    Some(size) => checked_size("AssetService:CreateEditableImage", size)?,
                    None => (DEFAULT_SIZE, DEFAULT_SIZE),
                };
                Ok(new_image(
                    lua,
                    size,
                    vec![0; (size.0 * size.1 * 4) as usize],
                ))
            })?
        }
        // From another editable's pixels; an asset would need a download
        // a running place has no business making.
        "CreateEditableImageAsync" => lua.create_function(
            |lua, (_this, content, _options): (Value, Value, Option<Table>)| {
                let pixels = match &content {
                    Value::UserData(data) => match &data.borrow::<LuaContent>()?.0 {
                        Source::Object(Value::UserData(object)) => object
                            .borrow::<LuaEditableImage>()
                            .map(|image| lock(&image.pixels).clone())
                            .ok(),
                        _ => None,
                    },
                    _ => None,
                };
                let pixels = pixels.ok_or_else(|| {
                    mlua::Error::runtime(
                        "AssetService:CreateEditableImageAsync failed to load the image",
                    )
                })?;
                Ok(new_image(lua, (pixels.width, pixels.height), pixels.rgba))
            },
        )?,
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}
