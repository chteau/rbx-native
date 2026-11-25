//! `EditableImage`: pixels read, written and drawn on. The pixels live in
//! `rbx_dom::editable`, where a renderer finds them through the
//! `Content::Object` an `ImageLabel.ImageContent` holds.

mod blend;
mod project;
mod transform;

use std::sync::{Arc, Mutex};

use mlua::{
    AnyUserData, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
};
use rbx_dom::editable::{self, lock, Editable, Pixels};
use rbx_dom::Ref;

use super::{budget, editable_ref, publish, Env};
use crate::datatypes::misc::LuaVector2;
use blend::{alpha_of, color, combine, Combine, Lock, Paint};

const DEFAULT_SIZE: u32 = 512;
const MAX_SIZE: u32 = 1024;

#[derive(Clone)]
pub(crate) struct LuaEditableImage {
    pub(crate) referent: Ref,
    pub(crate) pixels: Arc<Mutex<Pixels>>,
    /// See `LuaEditableMesh::token`.
    pub(crate) token: Arc<()>,
}

pub(crate) fn vector2(value: &Value, what: &str) -> Result<(f32, f32)> {
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

/// A new image of these pixels, or nothing when a client's budget has no
/// room for it.
pub(crate) fn from_rgba(
    lua: &Lua,
    env: &Env,
    (width, height, rgba): (u32, u32, Vec<u8>),
) -> Result<Option<LuaEditableImage>> {
    let cost = budget::image_cost(width, height);
    if !budget::admit(lua, env.client, false, cost) {
        return Ok(None);
    }
    let pixels = Arc::new(Mutex::new(Pixels {
        width,
        height,
        rgba,
        version: 0,
    }));
    let referent = publish(lua, Editable::Image(pixels.clone()));
    let image = LuaEditableImage {
        referent,
        pixels,
        token: Arc::default(),
    };
    budget::hold(lua, env.client, referent, false, cost, &image.token);
    Ok(Some(image))
}

/// `AssetService:CreateEditableImage`.
pub(crate) fn create(
    lua: &Lua,
    env: &Env,
    options: Option<Table>,
) -> Result<Option<LuaEditableImage>> {
    let size: Value = match &options {
        Some(options) => options.get("Size")?,
        None => Value::Nil,
    };
    let (w, h) = match size {
        Value::Nil => (DEFAULT_SIZE, DEFAULT_SIZE),
        size => checked_size("AssetService:CreateEditableImage", vector2(&size, "Size")?)?,
    };
    from_rgba(lua, env, (w, h, vec![0; (w * h * 4) as usize]))
}

impl LuaEditableImage {
    /// Edits the pixels and tells every reader they changed.
    pub(crate) fn edit<T>(&self, f: impl FnOnce(&mut Pixels) -> T) -> T {
        let mut pixels = lock(&self.pixels);
        let out = f(&mut pixels);
        pixels.version += 1;
        editable::touch();
        out
    }

    /// Paints every pixel `coverage` gives a share of, its integer
    /// coordinates within the box `x0..=x1`, `y0..=y1`.
    fn draw(
        &self,
        paint: Paint,
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
                        paint.apply(&mut pixels.rgba[i..i + 4], share);
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

fn paint(
    method: &str,
    color_value: &Value,
    transparency: f32,
    mode: &Value,
    alpha_type: &Value,
) -> Result<Paint> {
    Ok(Paint {
        rgb: color(color_value)?,
        alpha: alpha_of(transparency),
        mode: combine(mode, Combine::Overwrite, method)?,
        lock: blend::lock(alpha_type)?,
    })
}

fn anti_aliased(value: &Value) -> Result<bool> {
    Ok(match value {
        Value::UserData(data) if data.is::<crate::lua_enum::LuaEnumItem>() => {
            data.borrow::<crate::lua_enum::LuaEnumItem>()?.item_name() != "Disabled"
        }
        _ => true,
    })
}

/// The pixels a one-pixel line from `a` to `b` steps through.
fn bresenham((ax, ay): (f32, f32), (bx, by): (f32, f32)) -> Vec<(i64, i64)> {
    let (mut x, mut y, x1, y1) = (
        ax.round() as i64,
        ay.round() as i64,
        bx.round() as i64,
        by.round() as i64,
    );
    let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
    let (sx, sy) = ((x1 - x).signum(), (y1 - y).signum());
    let mut err = dx + dy;
    let mut out = vec![(x, y)];
    while (x, y) != (x1, y1) {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        out.push((x, y));
    }
    out
}

impl UserData for LuaEditableImage {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Object");
        fields.add_field_method_get("ClassName", |_, _| Ok("EditableImage"));
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
        methods.add_method("IsA", |_, _, class: String| {
            Ok(matches!(class.as_str(), "EditableImage" | "Object"))
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
            |_, this, (position, size, c, t, mode, alpha): (Value, Value, Value, f32, Value, Value)| {
                let (x, y) = vector2(&position, "position")?;
                let (w, h) = vector2(&size, "size")?;
                let (x0, y0, x1, y1) = (x.floor(), y.floor(), (x + w).floor(), (y + h).floor());
                let paint = paint("DrawRectangle", &c, t, &mode, &alpha)?;
                this.draw(paint, (x0, y0, x1 - 1.0, y1 - 1.0), |px, py| {
                    f32::from(px >= x0 && px < x1 && py >= y0 && py < y1)
                });
                Ok(())
            },
        );
        methods.add_method(
            "DrawCircle",
            |_,
             this,
             (center, radius, c, t, mode, aa, alpha): (
                Value,
                f32,
                Value,
                f32,
                Value,
                Value,
                Value,
            )| {
                let (cx, cy) = vector2(&center, "center")?;
                let paint = paint("DrawCircle", &c, t, &mode, &alpha)?;
                let smooth = anti_aliased(&aa)?;
                let reach = radius + 1.0;
                this.draw(
                    paint,
                    (cx - reach, cy - reach, cx + reach, cy + reach),
                    |px, py| {
                        let distance = (px - cx).hypot(py - cy);
                        if !smooth {
                            // The EditableImageDraw golden's 49 pixels for a radius of 4.
                            return f32::from(distance <= radius);
                        }
                        // Smoothed over the last pixel inside the rim.
                        let t = (radius - distance).clamp(0.0, 1.0);
                        t * t * (3.0 - 2.0 * t)
                    },
                );
                Ok(())
            },
        );
        methods.add_method(
            "DrawLine",
            |_, this, (p0, p1, c, t, mode, aa, alpha): (Value, Value, Value, f32, Value, Value, Value)| {
                let (a, b) = (vector2(&p0, "p1")?, vector2(&p1, "p2")?);
                let paint = paint("DrawLine", &c, t, &mode, &alpha)?;
                if !anti_aliased(&aa)? {
                    let steps = bresenham(a, b);
                    this.draw(paint, (a.0.min(b.0) - 1.0, a.1.min(b.1) - 1.0, a.0.max(b.0) + 1.0, a.1.max(b.1) + 1.0), |px, py| {
                        f32::from(steps.contains(&(px as i64, py as i64)))
                    });
                    return Ok(());
                }
                let ((ax, ay), (bx, by)) = (a, b);
                let (dx, dy) = (bx - ax, by - ay);
                let length = dx.hypot(dy);
                let bounds = (ax.min(bx) - 1.0, ay.min(by) - 1.0, ax.max(bx) + 1.0, ay.max(by) + 1.0);
                this.draw(paint, bounds, |px, py| {
                    let (rx, ry) = (px - ax, py - ay);
                    if length == 0.0 {
                        return (1.0 - rx.hypot(ry)).clamp(0.0, 1.0);
                    }
                    // One pixel wide, each end covering half of its own pixel.
                    let along = (rx * dx + ry * dy) / length;
                    let across = (rx * dy - ry * dx).abs() / length;
                    (1.0 - across).clamp(0.0, 1.0) * (along.min(length - along) + 0.5).clamp(0.0, 1.0)
                });
                Ok(())
            },
        );
        methods.add_method(
            "DrawTriangle",
            |_, this, (p1, p2, p3, c, t): (Value, Value, Value, Value, f32)| {
                let p = [
                    vector2(&p1, "p1")?,
                    vector2(&p2, "p2")?,
                    vector2(&p3, "p3")?,
                ];
                let paint = paint("DrawTriangle", &c, t, &Value::Nil, &Value::Nil)?;
                let edge = |(ax, ay): (f32, f32), (bx, by): (f32, f32), x: f32, y: f32| {
                    (bx - ax) * (y - ay) - (by - ay) * (x - ax)
                };
                let xs = p.map(|q| q.0);
                let ys = p.map(|q| q.1);
                let bounds = (
                    xs.iter().copied().fold(f32::INFINITY, f32::min),
                    ys.iter().copied().fold(f32::INFINITY, f32::min),
                    xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                    ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                );
                this.draw(paint, bounds, |px, py| {
                    let (x, y) = (px + 0.5, py + 0.5);
                    let e = [
                        edge(p[0], p[1], x, y),
                        edge(p[1], p[2], x, y),
                        edge(p[2], p[0], x, y),
                    ];
                    f32::from(e.iter().all(|&v| v >= 0.0) || e.iter().all(|&v| v <= 0.0))
                });
                Ok(())
            },
        );
        methods.add_method(
            "DrawImage",
            |_, this, (position, image, mode, alpha): (Value, AnyUserData, Value, Value)| {
                let (x, y) = vector2(&position, "position")?;
                let source = lock(&image.borrow::<LuaEditableImage>()?.pixels).clone();
                let mode = combine(&mode, Combine::AlphaBlend, "DrawImage")?;
                let lock_mode = blend::lock(&alpha)?;
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
                            if mode == Combine::Overwrite && lock_mode == Lock::None {
                                pixels.rgba[d..d + 4].copy_from_slice(texel);
                            } else {
                                Paint::texel(texel, mode, lock_mode)
                                    .apply(&mut pixels.rgba[d..d + 4], 1.0);
                            }
                        }
                    }
                });
                Ok(())
            },
        );
        transform::add(methods);
        project::add(methods);
        methods.add_method("Destroy", |lua, this, ()| {
            editable::withdraw(this.referent);
            budget::release(lua, this.referent);
            Ok(())
        });
    }
}
