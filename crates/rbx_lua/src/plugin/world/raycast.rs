//! `Workspace` spatial queries over the parts' oriented boxes.

use mlua::{AnyUserData, Lua, ObjectLike, Result, Value};
use rbx_dom::{CFrameData, Ref, Vector3Data};

use crate::ctx::Ctx;
use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::params::{LuaOverlapParams, LuaRaycastParams, LuaRaycastResult};
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;

struct Part {
    referent: Ref,
    position: [f32; 3],
    rotation: [f32; 9],
    size: [f32; 3],
}

impl Part {
    /// World-space vector to the box's local axes (rotation transpose).
    fn to_local(&self, v: [f32; 3]) -> [f32; 3] {
        let r = &self.rotation;
        std::array::from_fn(|j| r[j] * v[0] + r[3 + j] * v[1] + r[6 + j] * v[2])
    }

    fn aabb(&self) -> ([f32; 3], [f32; 3]) {
        aabb(self.position, &self.rotation, self.size)
    }
}

/// World-axis bounds of an oriented box.
fn aabb(position: [f32; 3], rotation: &[f32; 9], size: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let half: [f32; 3] = std::array::from_fn(|i| {
        (0..3)
            .map(|j| rotation[3 * i + j].abs() * size[j] / 2.0)
            .sum()
    });
    (
        std::array::from_fn(|i| position[i] - half[i]),
        std::array::from_fn(|i| position[i] + half[i]),
    )
}

fn vec3(v: Vector3Data) -> [f32; 3] {
    [v.x, v.y, v.z]
}

fn read_part(lua: &Lua, ctx: &Ctx, referent: Ref) -> Result<Option<Part>> {
    let Some(Value::UserData(size)) = crate::property::get(lua, ctx, referent, "Size")? else {
        return Ok(None);
    };
    let Some(Value::UserData(cframe)) = crate::property::get(lua, ctx, referent, "CFrame")? else {
        return Ok(None);
    };
    let (Ok(size), Ok(cframe)) = (size.borrow::<LuaVector3>(), cframe.borrow::<LuaCFrame>()) else {
        return Ok(None);
    };
    let CFrameData { position, rotation } = cframe.0;
    Ok(Some(Part {
        referent,
        position: vec3(position),
        rotation,
        size: vec3(size.0),
    }))
}

/// Every part under `root` passing the filter: `Exclude` drops filtered
/// subtrees, `Include` keeps only them.
fn collect(lua: &Lua, ctx: &Ctx, root: Ref, filter: &[Ref], include: bool) -> Result<Vec<Part>> {
    let mut candidates = Vec::new();
    {
        let dom = ctx.dom();
        let mut stack = vec![(root, !include)];
        while let Some((referent, inside)) = stack.pop() {
            let Some(instance) = dom.get(referent) else {
                continue;
            };
            let listed = filter.contains(&referent);
            if listed && !include {
                continue;
            }
            let inside = inside || listed;
            if referent != root && inside {
                candidates.push(referent);
            }
            stack.extend(instance.children().iter().map(|child| (*child, inside)));
        }
    }
    let mut parts = Vec::new();
    for referent in candidates {
        if let Some(part) = read_part(lua, ctx, referent)? {
            parts.push(part);
        }
    }
    Ok(parts)
}

/// Entry parameter along the ray (`0..=1` over the direction vector) and the
/// world-space normal of the face entered.
fn ray_box(part: &Part, origin: [f32; 3], dir: [f32; 3]) -> Option<(f32, [f32; 3])> {
    let from = part.to_local(std::array::from_fn(|i| origin[i] - part.position[i]));
    let along = part.to_local(dir);
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    let mut axis = 0;
    let mut sign = 1.0;
    for a in 0..3 {
        let half = part.size[a] / 2.0;
        if along[a].abs() < 1e-9 {
            if from[a].abs() > half {
                return None;
            }
            continue;
        }
        let t1 = (-half - from[a]) / along[a];
        let t2 = (half - from[a]) / along[a];
        let (enter, leave) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if enter > near {
            near = enter;
            axis = a;
            sign = if along[a] > 0.0 { -1.0 } else { 1.0 };
        }
        far = far.min(leave);
        if near > far {
            return None;
        }
    }
    if !(0.0..=1.0).contains(&near) {
        return None;
    }
    Some((
        near,
        std::array::from_fn(|i| part.rotation[3 * i + axis] * sign),
    ))
}

fn raycast(
    lua: &Lua,
    ctx: &Ctx,
    root: Ref,
    origin: [f32; 3],
    dir: [f32; 3],
    params: Option<AnyUserData>,
) -> Result<Value> {
    let (filter, include) = match &params {
        Some(data) => {
            let p = data.borrow::<LuaRaycastParams>()?;
            (p.filter.clone(), p.include)
        }
        None => (Vec::new(), false),
    };
    let mut best: Option<(f32, [f32; 3], &Part)> = None;
    let parts = collect(lua, ctx, root, &filter, include)?;
    for part in &parts {
        if let Some((t, normal)) = ray_box(part, origin, dir) {
            if best.as_ref().is_none_or(|(b, _, _)| t < *b) {
                best = Some((t, normal, part));
            }
        }
    }
    let Some((t, normal, part)) = best else {
        return Ok(Value::Nil);
    };
    let length = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    let position = Vector3Data {
        x: origin[0] + dir[0] * t,
        y: origin[1] + dir[1] * t,
        z: origin[2] + dir[2] * t,
    };
    let material = match crate::property::get(lua, ctx, part.referent, "Material")? {
        Some(Value::UserData(item)) => item.get::<String>("Name")?,
        _ => "Plastic".to_string(),
    };
    let result = LuaRaycastResult::new(
        part.referent,
        position,
        Vector3Data {
            x: normal[0],
            y: normal[1],
            z: normal[2],
        },
        t * length,
        material,
        ctx.clone(),
    );
    Ok(Value::UserData(lua.create_userdata(result)?))
}

fn overlapping(parts: Vec<Part>, center: [f32; 3], rotation: [f32; 9], size: [f32; 3]) -> Vec<Ref> {
    let (qmin, qmax) = aabb(center, &rotation, size);
    parts
        .into_iter()
        .filter(|part| {
            let (min, max) = part.aabb();
            (0..3).all(|i| min[i] <= qmax[i] && max[i] >= qmin[i])
        })
        .map(|part| part.referent)
        .collect()
}

fn instances(lua: &Lua, ctx: &Ctx, refs: Vec<Ref>, max: usize) -> Result<mlua::Table> {
    let limit = if max == 0 { usize::MAX } else { max };
    lua.create_sequence_from(
        refs.into_iter()
            .take(limit)
            .map(|r| LuaInstance::new(r, ctx.clone())),
    )
}

/// The deprecated `FindPartOnRay*` family, on top of `Raycast`: the part,
/// the hit point, its normal and material, or nil and the ray's end.
fn legacy_find(lua: &Lua, key: &str) -> Result<Option<Value>> {
    const KEY: &str = "rbxn.find_part_on_ray";
    let all: mlua::Table = match lua.named_registry_value::<Option<mlua::Table>>(KEY)? {
        Some(found) => found,
        None => {
            let made: mlua::Table = lua
                .load(
                    r#"
                    local function find(self, ray, list, include)
                        local params = RaycastParams.new()
                        params.FilterDescendantsInstances = list or {}
                        params.FilterType = include and Enum.RaycastFilterType.Include
                            or Enum.RaycastFilterType.Exclude
                        local hit = self:Raycast(ray.Origin, ray.Direction, params)
                        if hit then
                            return hit.Instance, hit.Position, hit.Normal, hit.Material
                        end
                        return nil, ray.Origin + ray.Direction, Vector3.zero, Enum.Material.Air
                    end
                    return {
                        FindPartOnRay = function(self, ray, ignore)
                            return find(self, ray, ignore and { ignore } or {}, false)
                        end,
                        FindPartOnRayWithIgnoreList = function(self, ray, list)
                            return find(self, ray, list, false)
                        end,
                        FindPartOnRayWithWhitelist = function(self, ray, list)
                            return find(self, ray, list, true)
                        end,
                    }
                    "#,
                )
                .set_name("=Workspace")
                .eval()?;
            lua.set_named_registry_value(KEY, made.clone())?;
            made
        }
    };
    let found: Value = all.get(key)?;
    Ok((!found.is_nil()).then_some(found))
}

pub(super) fn member(lua: &Lua, ctx: &Ctx, root: Ref, key: &str) -> Result<Option<Value>> {
    if key.starts_with("FindPartOnRay") {
        return legacy_find(lua, key);
    }
    let ctx = ctx.clone();
    let function = match key {
        "Raycast" => lua.create_function(
            move |lua,
                  (_, origin, dir, params): (
                Value,
                LuaVector3,
                LuaVector3,
                Option<AnyUserData>,
            )| { raycast(lua, &ctx, root, vec3(origin.0), vec3(dir.0), params) },
        )?,
        "FindPartsInRegion3" => lua.create_function(
            move |lua,
                  (_, region, ignore, max): (
                Value,
                AnyUserData,
                Option<AnyUserData>,
                Option<usize>,
            )| {
                let cframe: LuaCFrame = region.get("CFrame")?;
                let size: LuaVector3 = region.get("Size")?;
                let ignored: Vec<Ref> = match ignore {
                    Some(data) => data
                        .borrow::<LuaInstance>()
                        .map(|i| vec![i.referent()])
                        .unwrap_or_default(),
                    None => Vec::new(),
                };
                let parts = collect(lua, &ctx, root, &ignored, false)?;
                let found = overlapping(
                    parts,
                    vec3(cframe.0.position),
                    cframe.0.rotation,
                    vec3(size.0),
                );
                instances(lua, &ctx, found, max.unwrap_or(20))
            },
        )?,
        "GetPartBoundsInBox" => lua.create_function(
            move |lua,
                  (_, cframe, size, params): (
                Value,
                LuaCFrame,
                LuaVector3,
                Option<AnyUserData>,
            )| {
                let (filter, include, max) = match &params {
                    Some(data) => {
                        let p = data.borrow::<LuaOverlapParams>()?;
                        (p.filter.clone(), p.include, p.max_parts as usize)
                    }
                    None => (Vec::new(), false, 0),
                };
                let parts = collect(lua, &ctx, root, &filter, include)?;
                let found = overlapping(
                    parts,
                    vec3(cframe.0.position),
                    cframe.0.rotation,
                    vec3(size.0),
                );
                instances(lua, &ctx, found, max)
            },
        )?,
        "GetPartsInPart" => lua.create_function(|lua, _: mlua::MultiValue| lua.create_table())?,
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(function)))
}
