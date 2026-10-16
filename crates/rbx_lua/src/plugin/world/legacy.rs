//! Members Studio still answers to that rbx-native has nothing behind —
//! surface joints and assembly roots — and the camera's screen rays.

use mlua::{Lua, MultiValue, Result, Table, Value};
use rbx_dom::{CFrameData, Ref, Variant};

use crate::ctx::Ctx;
use crate::datatypes::cframe::LuaCFrame;
use crate::instance::LuaInstance;
use crate::plugin::env::env;

/// Studio's default `Camera.FieldOfView`, in degrees.
const DEFAULT_FOV: f32 = 70.0;

pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let db = ctx.database();
    let is = |base: &str| db.is_subclass_of(class, base);
    let f = match key {
        // Joints between touching surfaces are a physics concern the editor
        // never simulates; Studio's own "Join Surfaces" is off by default.
        "JoinToOutsiders" | "UnjoinFromOutsiders" if class == "Workspace" => {
            lua.create_function(|_, _: MultiValue| Ok(()))?
        }
        "BreakJoints" | "MakeJoints" if is("BasePart") || is("Model") => {
            lua.create_function(|_, _: MultiValue| Ok(()))?
        }
        // Without physics no joints or constraints are ever formed.
        "GetJoints" | "GetNoCollisionConstraints" | "GetConnectedParts" | "GetTouchingParts"
            if is("BasePart") =>
        {
            lua.create_function(|lua, _: MultiValue| lua.create_table())?
        }
        "CanCollideWith" if is("BasePart") => lua.create_function(|_, _: MultiValue| Ok(true))?,
        // No assemblies without physics: every part is its own root.
        "GetRootPart" if is("BasePart") => {
            let ctx = ctx.clone();
            lua.create_function(move |_, _: MultiValue| {
                Ok(LuaInstance::new(referent, ctx.clone()))
            })?
        }
        // Deprecated; Studio answers with the model's pivot.
        "GetModelCFrame" if is("Model") => {
            let ctx = ctx.clone();
            lua.create_function(move |_, _: MultiValue| {
                let frame = crate::pivot::pivot(&ctx.dom(), ctx.database(), referent);
                Ok(LuaCFrame(frame.unwrap_or(IDENTITY)))
            })?
        }
        "WorldToViewportPoint" | "WorldToScreenPoint" if class == "Camera" => {
            let ctx = ctx.clone();
            lua.create_function(move |lua, (_this, point): (Value, Value)| {
                world_to_view(lua, &ctx, referent, &point)
            })?
        }
        "ScreenPointToRay" | "ViewportPointToRay" if class == "Camera" => {
            let ctx = ctx.clone();
            lua.create_function(
                move |lua, (_this, x, y, depth): (Value, f32, f32, Option<f32>)| {
                    screen_ray(lua, &ctx, referent, [x, y], depth.unwrap_or(0.0))
                },
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

const IDENTITY: CFrameData = CFrameData {
    position: rbx_dom::Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
};

/// `Camera:WorldToViewportPoint(p)`: `(Vector3(x, y, depth), onScreen)`,
/// x/y in the view's pixels and depth along the look direction.
fn world_to_view(lua: &Lua, ctx: &Ctx, camera: Ref, point: &Value) -> Result<(Value, bool)> {
    let p =
        crate::datatypes::from_userdata::<crate::datatypes::vector3::LuaVector3>(point, "Vector3")?
            .0;
    let (frame, fov) = camera_pose(ctx, camera)?;
    let [width, height] = env(lua)
        .shared
        .borrow()
        .context
        .viewport_size
        .unwrap_or([1280.0, 720.0]);
    let d = [
        p.x - frame.position.x,
        p.y - frame.position.y,
        p.z - frame.position.z,
    ];
    // Into camera space: the transpose of the rotation, the camera looking
    // down its own -Z.
    let r = &frame.rotation;
    let local = [
        r[0] * d[0] + r[3] * d[1] + r[6] * d[2],
        r[1] * d[0] + r[4] * d[1] + r[7] * d[2],
        r[2] * d[0] + r[5] * d[1] + r[8] * d[2],
    ];
    let depth = -local[2];
    let tan = (fov.to_radians() / 2.0).tan();
    let aspect = width / height.max(1.0);
    let safe = if depth.abs() < 1e-6 { 1e-6 } else { depth };
    let ndc_x = local[0] / (safe * tan * aspect);
    let ndc_y = local[1] / (safe * tan);
    let x = (ndc_x + 1.0) / 2.0 * width;
    let y = (1.0 - ndc_y) / 2.0 * height;
    let on_screen = depth > 0.0 && (0.0..=width).contains(&x) && (0.0..=height).contains(&y);
    let vector3: Table = lua.globals().get("Vector3")?;
    let new_vector: mlua::Function = vector3.get("new")?;
    Ok((new_vector.call((x, y, depth))?, on_screen))
}

fn camera_pose(ctx: &Ctx, camera: Ref) -> Result<(CFrameData, f32)> {
    let dom = ctx.dom();
    let instance = dom
        .get(camera)
        .ok_or_else(crate::property::missing_instance)?;
    let frame = match instance.properties().get("CFrame") {
        Some(Variant::CFrame(frame)) => *frame,
        _ => IDENTITY,
    };
    let fov = match instance.properties().get("FieldOfView") {
        Some(Variant::Float32(degrees)) => *degrees,
        _ => DEFAULT_FOV,
    };
    Ok((frame, fov))
}

/// A unit `Ray` from the camera through pixel `point` of the 3D view,
/// starting `depth` studs out. Studio's edit view has no top bar, so the
/// screen and viewport variants agree.
fn screen_ray(lua: &Lua, ctx: &Ctx, camera: Ref, point: [f32; 2], depth: f32) -> Result<Value> {
    let (frame, fov) = {
        let dom = ctx.dom();
        let instance = dom
            .get(camera)
            .ok_or_else(crate::property::missing_instance)?;
        let frame = match instance.properties().get("CFrame") {
            Some(Variant::CFrame(frame)) => *frame,
            _ => IDENTITY,
        };
        let fov = match instance.properties().get("FieldOfView") {
            Some(Variant::Float32(degrees)) => *degrees,
            _ => DEFAULT_FOV,
        };
        (frame, fov)
    };
    let [width, height] = env(lua)
        .shared
        .borrow()
        .context
        .viewport_size
        .unwrap_or([1280.0, 720.0]);
    let tan = (fov.to_radians() / 2.0).tan();
    let local = [
        (2.0 * point[0] / width.max(1.0) - 1.0) * tan * width / height.max(1.0),
        (1.0 - 2.0 * point[1] / height.max(1.0)) * tan,
        -1.0,
    ];
    let length = local.iter().map(|c| c * c).sum::<f32>().sqrt();
    let local = local.map(|c| c / length);
    let r = &frame.rotation;
    let direction = [
        r[0] * local[0] + r[1] * local[1] + r[2] * local[2],
        r[3] * local[0] + r[4] * local[1] + r[5] * local[2],
        r[6] * local[0] + r[7] * local[1] + r[8] * local[2],
    ];
    let p = frame.position;
    let origin = [
        p.x + direction[0] * depth,
        p.y + direction[1] * depth,
        p.z + direction[2] * depth,
    ];
    let vector3: Table = lua.globals().get("Vector3")?;
    let new_vector: mlua::Function = vector3.get("new")?;
    let ray: Table = lua.globals().get("Ray")?;
    let new_ray: mlua::Function = ray.get("new")?;
    new_ray.call((
        new_vector.call::<Value>((origin[0], origin[1], origin[2]))?,
        new_vector.call::<Value>((direction[0], direction[1], direction[2]))?,
    ))
}
