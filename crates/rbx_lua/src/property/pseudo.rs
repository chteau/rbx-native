//! Properties Luau scripts read/write directly but that the file never
//! stores under their own name.
//!
//! `Position`, `Orientation` and `Rotation` are all views onto `CFrame`
//! (Studio computes them, it never serializes them separately); `BrickColor`
//! is a view onto `Color`, which is all Studio saves. A script's `Enabled`
//! is `Disabled` turned round (only `Disabled` is saved).

use std::f32::consts::PI;

use mlua::{IntoLua, Lua, Result, Value};
use rbx_dom::{BrickColor, CFrameData, Color3Data, Ref, Variant, Vector3Data};

use super::missing_instance;
use crate::ctx::Ctx;
use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::misc::LuaBrickColor;
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;

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

type V3 = [f32; 3];
type Rot = [f32; 9];

const IDENTITY_ROT: Rot = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn col(r: &Rot, i: usize) -> V3 {
    [r[i], r[3 + i], r[6 + i]]
}

fn from_cols(x: V3, y: V3, z: V3) -> Rot {
    [x[0], y[0], z[0], x[1], y[1], z[1], x[2], y[2], z[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: V3) -> V3 {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        v
    }
}

fn rot_apply(r: &Rot, v: V3) -> V3 {
    [
        r[0] * v[0] + r[1] * v[1] + r[2] * v[2],
        r[3] * v[0] + r[4] * v[1] + r[5] * v[2],
        r[6] * v[0] + r[7] * v[1] + r[8] * v[2],
    ]
}

fn transpose(r: &Rot) -> Rot {
    [r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]]
}

fn rot_mul(a: &Rot, b: &Rot) -> Rot {
    let (bx, by, bz) = (col(b, 0), col(b, 1), col(b, 2));
    from_cols(rot_apply(a, bx), rot_apply(a, by), rot_apply(a, bz))
}

fn vec_data(v: V3) -> Vector3Data {
    Vector3Data {
        x: v[0],
        y: v[1],
        z: v[2],
    }
}

fn is_attachment(ctx: &Ctx, referent: Ref) -> Result<bool> {
    let dom = ctx.dom();
    let instance = dom.get(referent).ok_or_else(missing_instance)?;
    Ok(ctx
        .database()
        .is_subclass_of(instance.class(), "Attachment"))
}

/// The parent BasePart's frame (identity otherwise) and the attachment's own.
fn attachment_frames(ctx: &Ctx, referent: Ref) -> Result<Option<(CFrameData, CFrameData)>> {
    if !is_attachment(ctx, referent)? {
        return Ok(None);
    }
    let Some(local) = cframe(ctx, referent)? else {
        return Ok(None);
    };
    let parent = ctx.dom().parent(referent);
    let parent_part = parent.filter(|&p| {
        ctx.dom()
            .get(p)
            .is_some_and(|i| ctx.database().is_subclass_of(i.class(), "BasePart"))
    });
    let parent_frame = match parent_part {
        Some(p) => cframe(ctx, p)?,
        None => None,
    }
    .unwrap_or(CFrameData {
        position: vec_data([0.0; 3]),
        rotation: IDENTITY_ROT,
    });
    Ok(Some((parent_frame, local)))
}

/// An attachment's world position, `None` when it isn't on a part.
pub(crate) fn attachment_world_position(ctx: &Ctx, referent: Ref) -> Option<[f64; 3]> {
    let (parent, local) = attachment_frames(ctx, referent).ok()??;
    let p = world_frame(&parent, &local).position;
    Some([f64::from(p.x), f64::from(p.y), f64::from(p.z)])
}

fn world_frame(parent: &CFrameData, local: &CFrameData) -> CFrameData {
    let p = rot_apply(
        &parent.rotation,
        [local.position.x, local.position.y, local.position.z],
    );
    CFrameData {
        position: Vector3Data {
            x: p[0] + parent.position.x,
            y: p[1] + parent.position.y,
            z: p[2] + parent.position.z,
        },
        rotation: rot_mul(&parent.rotation, &local.rotation),
    }
}

fn attachment_get(lua: &Lua, ctx: &Ctx, referent: Ref, name: &str) -> Result<Option<Value>> {
    let Some((parent, local)) = attachment_frames(ctx, referent)? else {
        return Ok(None);
    };
    let world = world_frame(&parent, &local);
    let value = match name {
        "Axis" => LuaVector3(vec_data(col(&local.rotation, 0))),
        "SecondaryAxis" => LuaVector3(vec_data(col(&local.rotation, 1))),
        "WorldAxis" => LuaVector3(vec_data(col(&world.rotation, 0))),
        "WorldSecondaryAxis" => LuaVector3(vec_data(col(&world.rotation, 1))),
        "WorldPosition" => LuaVector3(world.position),
        "WorldOrientation" => {
            let (rx, ry, rz) = LuaCFrame(world).to_euler_angles_yxz();
            LuaVector3::new(to_deg(rx), to_deg(ry), to_deg(rz))
        }
        "WorldCFrame" => return LuaCFrame(world).into_lua(lua).map(Some),
        _ => return Ok(None),
    };
    value.into_lua(lua).map(Some)
}

fn attachment_set(ctx: &Ctx, referent: Ref, name: &str, value: &Value) -> Result<bool> {
    let Some((parent, mut local)) = attachment_frames(ctx, referent)? else {
        return Ok(false);
    };
    let inverse = transpose(&parent.rotation);
    let x = col(&local.rotation, 0);
    let y = col(&local.rotation, 1);
    let rebuild_x = |axis: V3, secondary: V3| {
        let x = normalize(axis);
        let z = normalize(cross(x, secondary));
        from_cols(x, cross(z, x), z)
    };
    let rebuild_y = |axis: V3, secondary: V3| {
        let y = normalize(secondary);
        let z = normalize(cross(axis, y));
        from_cols(cross(y, z), y, z)
    };
    match name {
        "Axis" | "WorldAxis" | "SecondaryAxis" | "WorldSecondaryAxis" => {
            let v = vector3_arg(name, value)?;
            let mut v = [v.x, v.y, v.z];
            if name.starts_with("World") {
                v = rot_apply(&inverse, v);
            }
            local.rotation = if name.ends_with("SecondaryAxis") {
                rebuild_y(x, v)
            } else {
                rebuild_x(v, y)
            };
        }
        "WorldPosition" => {
            let w = vector3_arg(name, value)?;
            let rel = rot_apply(
                &inverse,
                [
                    w.x - parent.position.x,
                    w.y - parent.position.y,
                    w.z - parent.position.z,
                ],
            );
            local.position = vec_data(rel);
        }
        "WorldOrientation" => {
            let d = vector3_arg(name, value)?;
            let world = LuaCFrame::from_euler_angles_yxz(to_rad(d.x), to_rad(d.y), to_rad(d.z))
                .0
                .rotation;
            local.rotation = rot_mul(&inverse, &world);
        }
        "WorldCFrame" => {
            let Value::UserData(data) = value else {
                return Err(mlua::Error::runtime(
                    "Unable to assign property WorldCFrame. CFrame expected",
                ));
            };
            let world = data
                .borrow::<LuaCFrame>()
                .map_err(|_| {
                    mlua::Error::runtime(format!(
                        "Unable to assign property WorldCFrame. CFrame expected, got {}",
                        value.type_name()
                    ))
                })?
                .0;
            let rel = rot_apply(
                &inverse,
                [
                    world.position.x - parent.position.x,
                    world.position.y - parent.position.y,
                    world.position.z - parent.position.z,
                ],
            );
            local = CFrameData {
                position: vec_data(rel),
                rotation: rot_mul(&inverse, &world.rotation),
            };
        }
        _ => return Ok(false),
    }
    write_cframe(ctx, referent, local)?;
    Ok(true)
}

fn is_script(ctx: &Ctx, referent: Ref) -> Result<bool> {
    let dom = ctx.dom();
    let instance = dom.get(referent).ok_or_else(missing_instance)?;
    Ok(ctx
        .database()
        .is_subclass_of(instance.class(), "BaseScript"))
}

/// Reads a pseudo-property, returning `Ok(None)` for anything that isn't one so
/// the caller falls back to the generic reflected-property path.
pub(crate) fn get(lua: &Lua, ctx: &Ctx, referent: Ref, name: &str) -> Result<Option<Value>> {
    match name {
        "Axis" | "SecondaryAxis" | "WorldAxis" | "WorldSecondaryAxis" | "WorldPosition"
        | "WorldOrientation" | "WorldCFrame" => attachment_get(lua, ctx, referent, name),
        "ClockTime" => super::clock::get(ctx, referent),
        // Plugins and Open Cloud only (NewApis `next_gen_solver`).
        "NextGenerationPhysicsSolver" => Err(mlua::Error::runtime(
            "The current thread cannot read 'NextGenerationPhysicsSolver' (lacking capability PluginOrOpenCloud)",
        )),
        "Enabled" if is_script(ctx, referent)? => {
            let disabled = super::get(lua, ctx, referent, "Disabled")?;
            Ok(Some(Value::Boolean(!matches!(
                disabled,
                Some(Value::Boolean(true))
            ))))
        }
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
        "BrickColor" => {
            let Some((_, color)) = brick_color_target(ctx, referent)? else {
                return Ok(None);
            };
            let rgb = match color {
                Variant::Color3uint8 { r, g, b } => [r, g, b],
                Variant::Color3(color) => [color.r, color.g, color.b]
                    .map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8),
                _ => return Ok(Some(Value::Nil)),
            };
            LuaBrickColor(BrickColor::nearest(rgb))
                .into_lua(lua)
                .map(Some)
        }
        // A place file keeps `Terrain` only as a child of the Workspace; the
        // reflected reference property is filled in by the running engine.
        "Terrain" => {
            let dom = ctx.dom();
            let is_workspace =
                dom.get(referent).ok_or_else(missing_instance)?.class() == "Workspace";
            let found = is_workspace
                .then(|| {
                    dom.get(referent)?
                        .children()
                        .iter()
                        .copied()
                        .find(|child| dom.get(*child).is_some_and(|i| i.class() == "Terrain"))
                })
                .flatten();
            drop(dom);
            match found {
                Some(terrain) => LuaInstance::new(terrain, ctx.clone())
                    .into_lua(lua)
                    .map(Some),
                None => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

/// Where a part's `BrickColor` really lives: `Color`, under whichever name
/// the instance stores it (or its default), since Roblox derives the one
/// from the other, the closest colour in the table, and saves only
/// `Color`. `None` for a class with no `BrickColor`.
fn brick_color_target(ctx: &Ctx, referent: Ref) -> Result<Option<(String, Variant)>> {
    let dom = ctx.dom();
    let instance = dom.get(referent).ok_or_else(missing_instance)?;
    let database = ctx.database();
    if database
        .resolve_property(instance.class(), "BrickColor")
        .is_none()
    {
        return Ok(None);
    }
    Ok(database
        .stored_or_default(instance, "Color")
        .map(|(key, value)| (key.to_owned(), value.clone())))
}

fn brick_color_expected(value: &Value) -> mlua::Error {
    mlua::Error::runtime(format!(
        "Unable to assign property BrickColor. BrickColor expected, got {}",
        value.type_name()
    ))
}

/// A text object's `TextScaled` and `TextWrapped`, which the real client
/// keeps together: turning `TextScaled` on turns `TextWrapped` on, turning
/// `TextWrapped` off turns `TextScaled` off, and the other two writes touch
/// only themselves (recorded live with the `gui_text_scaled` fidelity scene).
/// False for every other write, which takes the generic path.
fn text_fit_set(ctx: &Ctx, referent: Ref, name: &str, value: &Value) -> Result<bool> {
    let (coupled, on) = match (name, value) {
        ("TextScaled", Value::Boolean(true)) => ("TextWrapped", true),
        ("TextWrapped", Value::Boolean(false)) => ("TextScaled", false),
        _ => return Ok(false),
    };
    let has_both = {
        let dom = ctx.dom();
        let instance = dom.get(referent).ok_or_else(missing_instance)?;
        ctx.database()
            .resolve_property(instance.class(), coupled)
            .is_some()
    };
    if !has_both {
        return Ok(false);
    }
    let mut dom = ctx.dom_mut();
    for key in [name, coupled] {
        dom.set_property(referent, key, Variant::Bool(on))
            .map_err(|error| mlua::Error::runtime(error.to_string()))?;
    }
    Ok(true)
}

/// Writes a pseudo-property, returning `Ok(false)` for anything that isn't one
/// so the caller falls back to the generic reflected-property path.
pub(crate) fn set(ctx: &Ctx, referent: Ref, name: &str, value: &Value) -> Result<bool> {
    match name {
        "Axis" | "SecondaryAxis" | "WorldAxis" | "WorldSecondaryAxis" | "WorldPosition"
        | "WorldOrientation" | "WorldCFrame" => attachment_set(ctx, referent, name, value),
        "ClockTime" | "TimeOfDay" => super::clock::set(ctx, referent, name, value),
        "TextScaled" | "TextWrapped" => text_fit_set(ctx, referent, name, value),
        "Enabled" if is_script(ctx, referent)? => {
            let Value::Boolean(enabled) = value else {
                return Err(mlua::Error::runtime(format!(
                    "Unable to assign property Enabled. bool expected, got {}",
                    value.type_name()
                )));
            };
            super::set(ctx, referent, "Disabled", &Value::Boolean(!enabled))
        }
        // Unassigning a model's `PrimaryPart` resets its pivot.
        "PrimaryPart" if value.is_nil() => {
            let mut dom = ctx.dom_mut();
            let model = dom.get(referent).ok_or_else(super::missing_instance)?;
            if !ctx.database().is_subclass_of(model.class(), "Model") {
                return Ok(false);
            }
            crate::pivot::clear_primary_part(&mut dom, ctx.database(), referent)
                .map_err(mlua::Error::runtime)?;
            Ok(true)
        }
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
            let Some((key, color)) = brick_color_target(ctx, referent)? else {
                return Ok(false);
            };
            let Value::UserData(data) = value else {
                return Err(brick_color_expected(value));
            };
            let brick_color = data
                .borrow::<LuaBrickColor>()
                .map_err(|_| brick_color_expected(value))?;
            let [r, g, b] = brick_color.0.rgb;
            // In the representation `Color` already holds there: the saved
            // `Color3uint8`, or a `Color3` a hand-written file kept.
            let written = match color {
                Variant::Color3(_) => Variant::Color3(Color3Data {
                    r: f32::from(r) / 255.0,
                    g: f32::from(g) / 255.0,
                    b: f32::from(b) / 255.0,
                }),
                _ => Variant::Color3uint8 { r, g, b },
            };
            ctx.dom_mut()
                .set_property(referent, &key, written)
                .map_err(|error| mlua::Error::runtime(error.to_string()))?;
            Ok(true)
        }
        _ => Ok(false),
    }
}
