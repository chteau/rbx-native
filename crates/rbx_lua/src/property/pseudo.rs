//! Properties Luau scripts read/write directly but that the file never
//! stores under their own name.
//!
//! `Position`, `Orientation` and `Rotation` are all views onto `CFrame`
//! (Studio computes them, it never serializes them separately); `BrickColor`
//! is a legacy palette index that Studio actually stores as `Color3uint8`,
//! same as the `Color` alias already handles for a plain `Color3`.

use std::f32::consts::PI;

use mlua::{IntoLua, Lua, Result, Value};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data};

use super::missing_instance;
use crate::ctx::Ctx;
use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::misc::LuaBrickColor;
use crate::datatypes::vector3::LuaVector3;

fn to_deg(rad: f32) -> f32 {
    rad * 180.0 / PI
}

fn to_rad(deg: f32) -> f32 {
    deg * PI / 180.0
}

fn vector3_arg(name: &str, value: &Value) -> Result<Vector3Data> {
    if let Value::UserData(data) = value {
        if let Ok(v) = data.borrow::<LuaVector3>() {
            return Ok(v.0);
        }
    }
    Err(mlua::Error::runtime(format!(
        "Unable to assign property {name}. Vector3 expected, got {}",
        value.type_name()
    )))
}

/// The instance's stored `CFrame`, or `None` when the class has no such
/// property (Position/Orientation/Rotation only make sense on a `BasePart`).
/// A `BasePart` that has never had its `CFrame` written yet reads as the
/// identity frame, matching a fresh Part's default (see `crate::defaults`).
fn cframe(ctx: &Ctx, referent: Ref) -> Result<Option<CFrameData>> {
    let dom = ctx.dom();
    let instance = dom.get(referent).ok_or_else(missing_instance)?;
    if ctx
        .database()
        .resolve_property(instance.class(), "CFrame")
        .is_none()
    {
        return Ok(None);
    }
    Ok(Some(match instance.properties().get("CFrame") {
        Some(Variant::CFrame(frame)) => *frame,
        Some(Variant::OptionalCFrame(Some(frame))) => *frame,
        _ => CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        },
    }))
}

fn write_cframe(ctx: &Ctx, referent: Ref, frame: CFrameData) -> Result<()> {
    ctx.dom_mut()
        .set_property(referent, "CFrame", Variant::CFrame(frame))
        .map_err(|error| mlua::Error::runtime(error.to_string()))?;
    Ok(())
}

/// Reads a pseudo-property, returning `Ok(None)` for anything that isn't one so
/// the caller falls back to the generic reflected-property path.
pub(crate) fn get(lua: &Lua, ctx: &Ctx, referent: Ref, name: &str) -> Result<Option<Value>> {
    match name {
        "Position" => {
            let Some(frame) = cframe(ctx, referent)? else {
                return Ok(None);
            };
            LuaVector3(frame.position).into_lua(lua).map(Some)
        }
        "Orientation" | "Rotation" => {
            let Some(frame) = cframe(ctx, referent)? else {
                return Ok(None);
            };
            let (rx, ry, rz) = LuaCFrame(frame).to_euler_angles_yxz();
            LuaVector3::new(to_deg(rx), to_deg(ry), to_deg(rz))
                .into_lua(lua)
                .map(Some)
        }
        _ => Ok(None),
    }
}

/// Writes a pseudo-property, returning `Ok(false)` for anything that isn't one
/// so the caller falls back to the generic reflected-property path.
pub(crate) fn set(ctx: &Ctx, referent: Ref, name: &str, value: &Value) -> Result<bool> {
    match name {
        "Position" => {
            let Some(mut frame) = cframe(ctx, referent)? else {
                return Ok(false);
            };
            frame.position = vector3_arg(name, value)?;
            write_cframe(ctx, referent, frame)?;
            Ok(true)
        }
        "Orientation" | "Rotation" => {
            let Some(mut frame) = cframe(ctx, referent)? else {
                return Ok(false);
            };
            let degrees = vector3_arg(name, value)?;
            frame.rotation = LuaCFrame::from_euler_angles_yxz(
                to_rad(degrees.x),
                to_rad(degrees.y),
                to_rad(degrees.z),
            )
            .0
            .rotation;
            write_cframe(ctx, referent, frame)?;
            Ok(true)
        }
        "BrickColor" => {
            let dom = ctx.dom();
            let instance = dom.get(referent).ok_or_else(missing_instance)?;
            let has_brick_color = ctx
                .database()
                .resolve_property(instance.class(), "BrickColor")
                .is_some();
            drop(dom);
            if !has_brick_color {
                return Ok(false);
            }
            let Value::UserData(data) = value else {
                return Err(mlua::Error::runtime(format!(
                    "Unable to assign property BrickColor. BrickColor expected, got {}",
                    value.type_name()
                )));
            };
            let brick_color = data.borrow::<LuaBrickColor>().map_err(|_| {
                mlua::Error::runtime(format!(
                    "Unable to assign property BrickColor. BrickColor expected, got {}",
                    value.type_name()
                ))
            })?;
            let (r, g, b) = brick_color.color.ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "Unable to assign property BrickColor: number {} has no known color in \
                     this build's small BrickColor table",
                    brick_color.number
                ))
            })?;
            ctx.dom_mut()
                .set_property(referent, "Color3uint8", Variant::Color3uint8 { r, g, b })
                .map_err(|error| mlua::Error::runtime(error.to_string()))?;
            Ok(true)
        }
        _ => Ok(false),
    }
}
