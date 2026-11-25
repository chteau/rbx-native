//! Scripts' solid modeling over [`crate::csg`]: `BasePart:UnionAsync`,
//! `SubtractAsync` and `IntersectAsync`, `GeometryService`'s batch
//! versions with their options, `FragmentAsync` and
//! `GenerateFragmentSites`, `CalculateConstraintsToPreserve`, and
//! `PartOperation:SubstituteGeometry`. Results, error texts and which calls
//! yield follow the CsgApi golden: the part methods answer in the same
//! frame, `GeometryService` a frame later.

use std::rc::Rc;

use mlua::{IntoLua, Lua, ObjectLike, Result, Table, Value};
use rbx_csg::glam::{DMat3, DMat4, DVec3};
use rbx_csg::{shapes, Solid};
use rbx_dom::{Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::Physics;
use crate::csg::{self, Flavor, Made, Operation};
use crate::ctx::Ctx;
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;

/// What the engine says when something that is not a part takes part.
const UNKNOWN_PRIMITIVE: &str = "CSG: One of the primitives was of unknown type (error -4)";

/// Runs the computation, then yields two frames (a heartbeat handler the
/// caller connected must see one go by, as with the real service) before handing back its
/// result or error, as `GeometryService`'s calls do.
const YIELDING: &str = r#"
local compute = ...
return function(...)
	local result = table.pack(pcall(compute, ...))
	task.wait()
	task.wait()
	if not result[1] then
		error(result[2], 0)
	end
	return table.unpack(result, 2, result.n)
end
"#;

fn yielding(lua: &Lua, compute: mlua::Function) -> Result<Value> {
    let f: mlua::Function = lua
        .load(YIELDING)
        .set_name("=GeometryService")
        .call(compute)?;
    Ok(Value::Function(f))
}

fn error(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

fn instance_of(value: &Value) -> Option<Ref> {
    value
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
}

/// The instances of an array argument; `what` names the type Roblox
/// casts to in its error.
fn instances(value: &Value, what: &str) -> Result<Vec<Ref>> {
    let cast = || error(format!("Unable to cast value to {what}"));
    let Value::Table(table) = value else {
        return Err(cast());
    };
    table
        .sequence_values::<Value>()
        .map(|v| instance_of(&v?).ok_or_else(cast))
        .collect()
}

fn new_instance(lua: &Lua, ctx: &Ctx, referent: Ref) -> Result<Value> {
    LuaInstance::new(referent, ctx.clone()).into_lua(lua)
}

/// Every result as a script gets it: fidelities set through the property
/// setter (an enum item, its name or its number cast as an assignment
/// does; `Performance` leaves the render fidelity `Automatic`, as the
/// golden shows), the mesh handed to the physics.
fn finish(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    made: Vec<Made>,
    collision: &Value,
    render: &Value,
) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for piece in made {
        let value = new_instance(lua, ctx, piece.referent)?;
        if let Value::UserData(instance) = &value {
            let cast = |value: &Value, kind: &str| {
                let from = match value {
                    Value::String(_) => "string",
                    Value::Integer(_) | Value::Number(_) => "int",
                    _ => "value",
                };
                error(format!("Unable to cast {from} to {kind}"))
            };
            if !collision.is_nil() {
                instance
                    .set("CollisionFidelity", collision.clone())
                    .map_err(|_| cast(collision, "CollisionFidelity"))?;
            }
            if !render.is_nil() {
                instance
                    .set("RenderFidelity", render.clone())
                    .map_err(|_| cast(render, "RenderFidelity"))?;
                let item: Value = instance.get("RenderFidelity")?;
                let name: String = item
                    .as_userdata()
                    .map_or(Ok(String::new()), |u| u.get("Name"))?;
                if name == "Performance" {
                    instance.set("RenderFidelity", "Automatic")?;
                }
            }
        }
        physics.register_mesh(piece.referent, piece.vertices, piece.triangles);
        out.push(value);
    }
    Ok(out)
}

/// The boolean made into operations; `None` when nothing is left of it.
fn operate(
    ctx: &Ctx,
    operation: Operation,
    flavor: Flavor,
    base: Ref,
    others: &[Ref],
    split: bool,
) -> Result<Option<Vec<Made>>> {
    let db = ctx.database();
    let solid = {
        let dom = ctx.dom();
        for &part in std::iter::once(&base).chain(others) {
            let is_part = dom.get(part).is_some_and(|i| {
                db.is_subclass_of(i.class(), "BasePart") && i.class() != "Terrain"
            });
            if !is_part {
                return Err(error(UNKNOWN_PRIMITIVE));
            }
        }
        match csg::combine(&dom, db, operation, base, others) {
            Ok(solid) => solid,
            Err(why) if why == "Empty" => return Ok(None),
            Err(why) => return Err(error(format!("CSG: {why}"))),
        }
    };
    let mut dom = ctx.dom_mut();
    Ok(Some(csg::build(
        &mut dom, db, operation, flavor, base, others, solid, split,
    )))
}

fn part_method(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    operation: Operation,
) -> Result<Value> {
    let (ctx, physics) = (ctx.clone(), physics.clone());
    let f = lua.create_function(
        move |lua, (this, parts, collision, render): (Value, Value, Value, Value)| {
            let base = instance_of(&this)
                .ok_or_else(|| error("Expected ':' not '.' calling member function"))?;
            let others = instances(&parts, "Objects")?;
            match operate(&ctx, operation, Flavor::Part, base, &others, false)? {
                Some(made) => Ok(finish(lua, &ctx, &physics, made, &collision, &render)?.remove(0)),
                None => Ok(Value::Nil),
            }
        },
    )?;
    Ok(Value::Function(f))
}

/// Refuses an options key not in `allowed`, as the engine does.
fn check_keys(options: &Table, allowed: &[&str], kind: &str) -> Result<()> {
    for pair in options.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let name = key.to_string().unwrap_or_default();
        if !allowed.contains(&name.as_str()) {
            return Err(error(format!(
                "Key '{name}' is not a valid member of {kind}"
            )));
        }
    }
    Ok(())
}

/// `GeometryService` options: fidelities, and `SplitApart` (on unless
/// asked otherwise).
fn options(value: &Value) -> Result<(Value, Value, bool)> {
    let options = match value {
        Value::Nil => return Ok((Value::Nil, Value::Nil, true)),
        Value::Table(options) => options,
        _ => return Err(error("Unable to cast value to Dictionary")),
    };
    check_keys(
        options,
        &[
            "CollisionFidelity",
            "RenderFidelity",
            "FluidFidelity",
            "SplitApart",
        ],
        "CSGOptions",
    )?;
    let split = match options.get::<Value>("SplitApart")? {
        Value::Nil => true,
        Value::Boolean(on) => on,
        _ => return Err(error("argument for Key 'SplitApart' needs to be a boolean")),
    };
    Ok((
        options.get("CollisionFidelity")?,
        options.get("RenderFidelity")?,
        split,
    ))
}

fn service_method(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    operation: Operation,
) -> Result<Value> {
    let (ctx, physics) = (ctx.clone(), physics.clone());
    let compute = lua.create_function(
        move |lua, (_this, part, parts, opts): (Value, Value, Value, Value)| {
            let base = instance_of(&part).ok_or_else(|| error(UNKNOWN_PRIMITIVE))?;
            let others = instances(&parts, "Array")?;
            let (collision, render, split) = options(&opts)?;
            let made = operate(&ctx, operation, Flavor::Service, base, &others, split)?;
            let results = match made {
                Some(made) => finish(lua, &ctx, &physics, made, &collision, &render)?,
                None => Vec::new(),
            };
            lua.create_sequence_from(results)
        },
    )?;
    yielding(lua, compute)
}

/// A half-space as a solid: a cube `extent` on a side whose face lies on
/// the plane through `at` facing `normal`, the cube behind it.
fn half_space(at: DVec3, normal: DVec3, extent: f64) -> Solid {
    let x = normal.normalize();
    let y = x.any_orthonormal_vector();
    let z = x.cross(y);
    let frame = DMat4::from_mat3_translation(DMat3::from_cols(x, y, z), at - x * extent / 2.0);
    // In f64 all the way: a plane far from the origin must not drift.
    let block = shapes::block();
    Solid::from_triangles(
        &block.positions,
        &block.indices,
        frame * DMat4::from_scale(DVec3::splat(extent)),
    )
}

fn vector_of(value: &Value) -> Option<DVec3> {
    let v = value.as_userdata()?.borrow::<LuaVector3>().ok()?.0;
    Some(DVec3::new(v.x.into(), v.y.into(), v.z.into()))
}

fn vector3(v: DVec3) -> Vector3Data {
    Vector3Data {
        x: v.x as f32,
        y: v.y as f32,
        z: v.z as f32,
    }
}

/// A `MeshPart` showing `cell` (world space) in `part`'s frame, with
/// `part`'s properties where a `MeshPart` has them: one fragment.
fn fragment_part(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    part: Ref,
    cell: &Solid,
) -> Result<Value> {
    const CLASS: &str = "MeshPart";
    let db = ctx.database();
    let (referent, local) = {
        let mut dom = ctx.dom_mut();
        let source = dom.get(part).ok_or_else(|| error(UNKNOWN_PRIMITIVE))?;
        let at = match db.stored_or_default(source, "CFrame") {
            Some((_, Variant::CFrame(cf))) => csg::frame(cf),
            _ => DMat4::IDENTITY,
        };
        let props: Vec<(String, Variant)> = source
            .properties()
            .iter()
            .filter(|(k, _)| {
                db.resolve_property(CLASS, db.canonical_name(CLASS, k))
                    .is_some()
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let local = cell.transformed(at.inverse());
        let (lo, hi) = local.bounds().unwrap_or_default();
        let referent = dom.new_instance(CLASS, CLASS, None);
        for (key, value) in props {
            let _ = dom.set_property(referent, &key, value);
        }
        set(
            &mut dom,
            db,
            referent,
            "Size",
            Variant::Vector3(vector3(hi - lo)),
        );
        (referent, local)
    };
    let mesh = local.to_mesh();
    let positions: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
    let triangles = mesh.indices.as_chunks::<3>().0.to_vec();
    crate::editable::bind(lua, ctx, referent, &positions, &triangles)?;
    physics.register_mesh(referent, positions, triangles);
    new_instance(lua, ctx, referent)
}

fn set(dom: &mut WeakDom, db: &ReflectionDatabase, referent: Ref, name: &str, value: Variant) {
    let Some(class) = dom.get(referent).map(|i| i.class().to_owned()) else {
        return;
    };
    let key = db.stored_names(&class, name)[0].to_owned();
    let _ = dom.set_property(referent, &key, value);
}

/// `FragmentAsync`: the part cut into the Voronoi cells of `sites`, each
/// what of the part lies nearer its site than any other, as
/// `{Index = site, Instance = MeshPart}` entries.
fn fragment(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    part: Ref,
    sites: &[DVec3],
) -> Result<Table> {
    let db = ctx.database();
    let whole = csg::world_solid(&ctx.dom(), db, part).map_err(|_| error(UNKNOWN_PRIMITIVE))?;
    let (lo, hi) = whole.bounds().unwrap_or_default();
    let reach = sites
        .iter()
        .map(|s| (*s - lo).length().max((*s - hi).length()))
        .fold(0.0, f64::max);
    let extent = 4.0 * ((hi - lo).length() + reach);
    let out = lua.create_table()?;
    for (i, &site) in sites.iter().enumerate() {
        let mut cell = whole.clone();
        for (j, &other) in sites.iter().enumerate() {
            if i == j || (other - site).length_squared() < 1e-12 {
                continue;
            }
            let cut = half_space((site + other) / 2.0, other - site, extent);
            cell = cell
                .intersect(cut)
                .map_err(|f| error(format!("CSG: {f:?}")))?;
        }
        let Ok(cell) = cell.finish(None) else {
            continue;
        };
        let entry = lua.create_table()?;
        entry.set("Index", i + 1)?;
        entry.set("Instance", fragment_part(lua, ctx, physics, part, &cell)?)?;
        out.push(entry)?;
    }
    Ok(out)
}

/// How many sites `GenerateFragmentSites` hands back by default.
const FRAGMENT_SITES: usize = 8;

/// `GenerateFragmentSites`: points spread through the part's bounds, the
/// same ones for the same part every run.
fn generate_sites(lua: &Lua, ctx: &Ctx, part: &Value) -> Result<Table> {
    let part = instance_of(part).ok_or_else(|| error(UNKNOWN_PRIMITIVE))?;
    let db = ctx.database();
    let solid = csg::world_solid(&ctx.dom(), db, part).map_err(|_| error(UNKNOWN_PRIMITIVE))?;
    // Roblox's sites reach past the part (CsgApi: some lie outside a 4-stud
    // cube, all within 10 studs), so they spread over its bounds × 1.5.
    let (lo, hi) = solid.bounds().unwrap_or_default();
    let (centre, half) = ((lo + hi) / 2.0, (hi - lo) * 0.75);
    let (lo, hi) = (centre - half, centre + half);
    let mut seed = 0x9e37_79b9_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        f64::from(seed) / f64::from(u32::MAX)
    };
    let sites = lua.create_table()?;
    for n in 1..=FRAGMENT_SITES {
        let p = lo + (hi - lo) * DVec3::new(next(), next(), next());
        sites.raw_set(n, LuaVector3(vector3(p)))?;
    }
    Ok(sites)
}

fn ref_of(db: &ReflectionDatabase, instance: &rbx_dom::Instance, key: &str) -> Option<Ref> {
    match db.stored_or_default(instance, key) {
        Some((_, Variant::Ref(r))) => Some(*r),
        _ => None,
    }
}
fn refers_to(
    db: &ReflectionDatabase,
    instance: &rbx_dom::Instance,
    keys: &[&str],
    target: Ref,
) -> bool {
    keys.iter().any(|k| ref_of(db, instance, k) == Some(target))
}

/// `CalculateConstraintsToPreserve`: each of the source's attachments a
/// constraint holds, with that constraint, both to move under the first
/// destination; and each `WeldConstraint` on the source with the parts it
/// joins, unless `weldConstraintPreserve` is `None`.
fn constraints_to_preserve(
    lua: &Lua,
    ctx: &Ctx,
    source: &Value,
    destination: &Value,
    options: &Value,
) -> Result<Table> {
    let source = instance_of(source).ok_or_else(|| error(UNKNOWN_PRIMITIVE))?;
    let destination = instances(destination, "Array")?;
    let mut welds = true;
    if let Value::Table(options) = options {
        check_keys(
            options,
            &[
                "tolerance",
                "weldConstraintPreserve",
                "dropAttachmentsWithoutConstraints",
            ],
            "CSGConstraintOptions",
        )?;
        if let Value::UserData(item) = options.get::<Value>("weldConstraintPreserve")? {
            welds = item.get::<String>("Name")? != "None";
        }
    }
    let out = lua.create_table()?;
    let Some(&target) = destination.first() else {
        return Ok(out);
    };
    let db = ctx.database();
    let mut entries: Vec<Vec<(&str, Option<Ref>)>> = Vec::new();
    {
        let dom = ctx.dom();
        let Some(source_instance) = dom.get(source) else {
            return Ok(out);
        };
        let everything: Vec<Ref> = descendants(&dom, dom.root_refs());
        for &child in source_instance.children() {
            if dom.get(child).is_none_or(|i| i.class() != "Attachment") {
                continue;
            }
            for &c in &everything {
                let Some(instance) = dom.get(c) else {
                    continue;
                };
                if db.is_subclass_of(instance.class(), "Constraint")
                    && refers_to(db, instance, &["Attachment0", "Attachment1"], child)
                {
                    entries.push(vec![
                        ("Attachment", Some(child)),
                        ("AttachmentParent", Some(target)),
                        ("Constraint", Some(c)),
                        ("ConstraintParent", Some(target)),
                    ]);
                }
            }
        }
        if welds {
            for &c in &everything {
                let Some(instance) = dom.get(c) else {
                    continue;
                };
                if instance.class() == "WeldConstraint"
                    && refers_to(db, instance, &["Part0", "Part1"], source)
                {
                    let part = |k: &str| ref_of(db, instance, k);
                    entries.push(vec![
                        ("WeldConstraint", Some(c)),
                        ("WeldConstraintPart0", part("Part0")),
                        ("WeldConstraintPart1", part("Part1")),
                    ]);
                }
            }
        }
    }
    for fields in entries {
        let entry = lua.create_table()?;
        for (key, referent) in fields {
            if let Some(referent) = referent {
                entry.set(key, new_instance(lua, ctx, referent)?)?;
            }
        }
        out.push(entry)?;
    }
    Ok(out)
}

fn descendants(dom: &WeakDom, roots: &[Ref]) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = roots.to_vec();
    while let Some(r) = stack.pop() {
        if let Some(instance) = dom.get(r) {
            stack.extend_from_slice(instance.children());
            out.push(r);
        }
    }
    out
}

/// `SubstituteGeometry`: the target takes the source operation's carved
/// mesh, tree and size, keeping its own frame and look. A source that is
/// not an operation changes nothing, as in a live server.
fn substitute(ctx: &Ctx, physics: &Rc<dyn Physics>, target: Ref, source: Ref) -> Result<()> {
    let db = ctx.database();
    let (copied, mesh) = {
        let dom = ctx.dom();
        let Some(source) = dom
            .get(source)
            .filter(|i| db.is_subclass_of(i.class(), "PartOperation"))
        else {
            return Ok(());
        };
        let copied: Vec<(String, Variant)> = ["MeshData2", "ChildData2", "InitialSize", "size"]
            .iter()
            .filter_map(|k| Some((k.to_string(), source.properties().get(*k)?.clone())))
            .collect();
        (copied, csg::local_mesh(source))
    };
    let mut dom = ctx.dom_mut();
    for (key, value) in copied {
        let _ = dom.set_property(target, &key, value);
    }
    if let Some(instance) = dom.get_mut(target) {
        csg::clear_stale_geometry(instance);
    }
    drop(dom);
    if let Some((positions, indices)) = mesh {
        physics.register_mesh(target, positions, indices.as_chunks::<3>().0.to_vec());
    }
    Ok(())
}

pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let db = ctx.database();
    let operation = match key {
        "UnionAsync" => Some(Operation::Union),
        "SubtractAsync" => Some(Operation::Subtract),
        "IntersectAsync" => Some(Operation::Intersect),
        _ => None,
    };
    let value = match (class, operation) {
        ("GeometryService", Some(operation)) => service_method(lua, ctx, physics, operation)?,
        (_, Some(operation)) if db.is_subclass_of(class, "BasePart") && class != "Terrain" => {
            part_method(lua, ctx, physics, operation)?
        }
        ("GeometryService", None) => match key {
            "FragmentAsync" => {
                let (ctx, physics) = (ctx.clone(), physics.clone());
                let compute = lua.create_function(
                    move |lua, (_this, part, sites, _options): (Value, Value, Value, Value)| {
                        let part = instance_of(&part).ok_or_else(|| error(UNKNOWN_PRIMITIVE))?;
                        let Value::Table(sites) = sites else {
                            return Err(error("Unable to cast value to Array"));
                        };
                        let sites: Vec<DVec3> = sites
                            .sequence_values::<Value>()
                            .map(|v| {
                                vector_of(&v?)
                                    .ok_or_else(|| error("Unable to cast value to Vector3"))
                            })
                            .collect::<Result<_>>()?;
                        fragment(lua, &ctx, &physics, part, &sites)
                    },
                )?;
                yielding(lua, compute)?
            }
            "GenerateFragmentSites" => {
                let ctx = ctx.clone();
                Value::Function(lua.create_function(
                    move |lua, (_this, part, _options): (Value, Value, Value)| {
                        generate_sites(lua, &ctx, &part)
                    },
                )?)
            }
            "CalculateConstraintsToPreserve" => {
                let ctx = ctx.clone();
                Value::Function(
                    lua.create_function(
                        move |lua,
                              (_this, source, destination, options): (
                            Value,
                            Value,
                            Value,
                            Value,
                        )| {
                            constraints_to_preserve(lua, &ctx, &source, &destination, &options)
                        },
                    )?,
                )
            }
            _ => return Ok(None),
        },
        _ if key == "SubstituteGeometry" && db.is_subclass_of(class, "PartOperation") => {
            let (ctx, physics) = (ctx.clone(), physics.clone());
            Value::Function(
                lua.create_function(move |_, (this, source): (Value, Value)| {
                    let target = instance_of(&this)
                        .ok_or_else(|| error("Expected ':' not '.' calling member function"))?;
                    let source = instance_of(&source)
                        .ok_or_else(|| error("Unable to cast value to Object"))?;
                    substitute(&ctx, &physics, target, source)
                })?,
            )
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
