//! `Workspace`'s spatial queries on a server: the Lua entry points that
//! hand a filter and a shape to [`Physics`] and wrap what comes back.

use std::collections::HashSet;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, Result, Value};
use rbx_dom::{CFrameData, Ref, Vector3Data};

use super::Physics;
use crate::ctx::Ctx;
use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::params::{LuaOverlapParams, LuaRaycastParams, LuaRaycastResult};
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// Which parts a query may return.
#[derive(Debug, Clone)]
pub struct QueryFilter {
    /// Every `BasePart` at or under the instances the params list.
    pub parts: HashSet<Ref>,
    /// `true` for an `Include` filter, `false` for `Exclude`.
    pub include: bool,
    pub respect_can_collide: bool,
    pub collision_group: String,
    /// 0 is unlimited.
    pub max_parts: usize,
    /// `RaycastParams.IgnoreWater`: rays go through terrain water.
    pub ignore_water: bool,
}

/// The shape a query sweeps or tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueryShape {
    /// Full size.
    Box([f64; 3]),
    /// Radius.
    Ball(f64),
    /// That part's own collision shape.
    Part(Ref),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueryHit {
    pub part: Ref,
    pub position: [f64; 3],
    pub normal: [f64; 3],
    pub distance: f64,
    /// What was hit is terrain of this material (`Enum.Material` name).
    pub material: Option<&'static str>,
}

fn array(v: Vector3Data) -> [f64; 3] {
    [f64::from(v.x), f64::from(v.y), f64::from(v.z)]
}

fn expand(ctx: &Ctx, roots: &[Ref]) -> HashSet<Ref> {
    let db = ctx.database();
    let dom = ctx.dom();
    let mut found = HashSet::new();
    let mut stack = roots.to_vec();
    while let Some(at) = stack.pop() {
        let Some(instance) = dom.get(at) else {
            continue;
        };
        if db.is_subclass_of(instance.class(), "BasePart") {
            found.insert(at);
        }
        stack.extend(instance.children().iter().copied());
    }
    found
}

fn make_filter(
    ctx: &Ctx,
    listed: &[Ref],
    include: bool,
    collision_group: String,
    respect_can_collide: bool,
    max_parts: usize,
) -> QueryFilter {
    QueryFilter {
        parts: expand(ctx, listed),
        include,
        respect_can_collide,
        collision_group,
        max_parts,
        ignore_water: false,
    }
}

fn raycast_filter(ctx: &Ctx, params: &Option<AnyUserData>) -> Result<QueryFilter> {
    match params {
        Some(data) => {
            let p = data.borrow::<LuaRaycastParams>()?;
            Ok(QueryFilter {
                ignore_water: p.ignore_water,
                ..make_filter(
                    ctx,
                    &p.filter,
                    p.include,
                    p.collision_group.clone(),
                    p.respect_can_collide,
                    0,
                )
            })
        }
        None => Ok(make_filter(ctx, &[], false, "Default".to_owned(), false, 0)),
    }
}

fn overlap_filter(ctx: &Ctx, params: &Option<AnyUserData>) -> Result<QueryFilter> {
    match params {
        Some(data) => {
            let p = data.borrow::<LuaOverlapParams>()?;
            Ok(make_filter(
                ctx,
                &p.filter,
                p.include,
                p.collision_group.clone(),
                p.respect_can_collide,
                p.max_parts as usize,
            ))
        }
        None => Ok(make_filter(ctx, &[], false, "Default".to_owned(), false, 0)),
    }
}

fn hit_value(lua: &Lua, ctx: &Ctx, hit: Option<QueryHit>) -> Result<Value> {
    let Some(hit) = hit else {
        return Ok(Value::Nil);
    };
    let material = match (
        hit.material,
        crate::property::get(lua, ctx, hit.part, "Material")?,
    ) {
        (Some(material), _) => material.to_owned(),
        (None, Some(Value::UserData(item))) => mlua::ObjectLike::get::<String>(&item, "Name")?,
        _ => "Plastic".to_owned(),
    };
    let vector = |v: [f64; 3]| Vector3Data {
        x: v[0] as f32,
        y: v[1] as f32,
        z: v[2] as f32,
    };
    let result = LuaRaycastResult::new(
        hit.part,
        vector(hit.position),
        vector(hit.normal),
        hit.distance as f32,
        material,
        ctx.clone(),
    );
    Ok(Value::UserData(lua.create_userdata(result)?))
}

fn instances(lua: &Lua, ctx: &Ctx, refs: Vec<Ref>) -> Result<mlua::Table> {
    lua.create_sequence_from(refs.into_iter().map(|r| LuaInstance::new(r, ctx.clone())))
}

fn cframe_at(position: Vector3Data) -> CFrameData {
    CFrameData {
        position,
        rotation: IDENTITY,
    }
}

fn part_ref(part: &AnyUserData) -> Result<Ref> {
    Ok(part.borrow::<LuaInstance>()?.referent())
}

fn cframe_of_part(lua: &Lua, ctx: &Ctx, part: Ref) -> Result<CFrameData> {
    if let Some(Value::UserData(data)) = crate::property::get(lua, ctx, part, "CFrame")? {
        if let Ok(cframe) = data.borrow::<LuaCFrame>() {
            return Ok(cframe.0);
        }
    }
    Err(mlua::Error::runtime("Part expected"))
}

/// The deprecated `FindPartOnRay*` family, on top of `Raycast`: the part,
/// the hit point, its normal and material, or nil and the ray's end. And
/// the `FindPartsInRegion3*` family on top of `GetPartBoundsInBox`: at most
/// `maxParts` (20 by default) parts whose bounds touch the region.
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
                    local function region(self, box, list, include, max)
                        local params = OverlapParams.new()
                        params.FilterDescendantsInstances = list or {}
                        params.FilterType = include and Enum.RaycastFilterType.Include
                            or Enum.RaycastFilterType.Exclude
                        params.MaxParts = max or 20
                        return self:GetPartBoundsInBox(box.CFrame, box.Size, params)
                    end
                    return {
                        FindPartsInRegion3 = function(self, box, ignore, max)
                            return region(self, box, ignore and { ignore } or {}, false, max)
                        end,
                        FindPartsInRegion3WithIgnoreList = function(self, box, list, max)
                            return region(self, box, list, false, max)
                        end,
                        FindPartsInRegion3WithWhiteList = function(self, box, list, max)
                            return region(self, box, list, true, max)
                        end,
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

pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    key: &str,
) -> Result<Option<Value>> {
    if key.starts_with("FindPartOnRay") || key.starts_with("FindPartsInRegion3") {
        return legacy_find(lua, key);
    }
    let ctx = ctx.clone();
    let physics = physics.clone();
    let function = match key {
        "Raycast" => lua.create_function(
            move |lua,
                  (_, origin, direction, params): (
                Value,
                LuaVector3,
                LuaVector3,
                Option<AnyUserData>,
            )| {
                let filter = raycast_filter(&ctx, &params)?;
                let hit = physics.raycast(array(origin.0), array(direction.0), &filter);
                hit_value(lua, &ctx, hit)
            },
        )?,
        "Blockcast" => lua.create_function(
            move |lua,
                  (_, cframe, size, direction, params): (
                Value,
                LuaCFrame,
                LuaVector3,
                LuaVector3,
                Option<AnyUserData>,
            )| {
                let filter = raycast_filter(&ctx, &params)?;
                let hit = physics.shapecast(
                    &QueryShape::Box(array(size.0)),
                    &cframe.0,
                    array(direction.0),
                    &filter,
                );
                hit_value(lua, &ctx, hit)
            },
        )?,
        "Spherecast" => lua.create_function(
            move |lua,
                  (_, position, radius, direction, params): (
                Value,
                LuaVector3,
                f64,
                LuaVector3,
                Option<AnyUserData>,
            )| {
                let filter = raycast_filter(&ctx, &params)?;
                let hit = physics.shapecast(
                    &QueryShape::Ball(radius),
                    &cframe_at(position.0),
                    array(direction.0),
                    &filter,
                );
                hit_value(lua, &ctx, hit)
            },
        )?,
        "Shapecast" => lua.create_function(
            move |lua,
                  (_, part, direction, params): (
                Value,
                AnyUserData,
                LuaVector3,
                Option<AnyUserData>,
            )| {
                let part = part_ref(&part)?;
                let from = cframe_of_part(lua, &ctx, part)?;
                let filter = raycast_filter(&ctx, &params)?;
                let hit =
                    physics.shapecast(&QueryShape::Part(part), &from, array(direction.0), &filter);
                hit_value(lua, &ctx, hit)
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
                let filter = overlap_filter(&ctx, &params)?;
                let found =
                    physics.overlap(&QueryShape::Box(array(size.0)), &cframe.0, false, &filter);
                instances(lua, &ctx, found)
            },
        )?,
        "GetPartBoundsInRadius" => {
            lua.create_function(
                move |lua,
                      (_, position, radius, params): (
                    Value,
                    LuaVector3,
                    f64,
                    Option<AnyUserData>,
                )| {
                    let filter = overlap_filter(&ctx, &params)?;
                    let found = physics.overlap(
                        &QueryShape::Ball(radius),
                        &cframe_at(position.0),
                        false,
                        &filter,
                    );
                    instances(lua, &ctx, found)
                },
            )?
        }
        "GetPartsInPart" => lua.create_function(
            move |lua, (_, part, params): (Value, AnyUserData, Option<AnyUserData>)| {
                let part = part_ref(&part)?;
                let at = cframe_of_part(lua, &ctx, part)?;
                let filter = overlap_filter(&ctx, &params)?;
                let found = physics.overlap(&QueryShape::Part(part), &at, true, &filter);
                instances(lua, &ctx, found)
            },
        )?,
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(function)))
}
