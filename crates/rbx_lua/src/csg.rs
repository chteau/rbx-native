//! Solid modeling over a DOM: unions, negations and intersections of
//! parts, carved with the shared [`rbx_csg`] kernel, made into the
//! `UnionOperation` / `IntersectOperation` Studio would make. Scripts reach
//! it through `BasePart:UnionAsync` and `GeometryService` (see
//! `server::csg`), plugins through `plugin:Union` and its siblings, and the
//! editor's Model tab directly.
//!
//! An operation saves what Studio saves: `ChildData2`, a nested `.rbxm` of
//! the parts it was made from in its own space (a subtracted part wrapped
//! in a `NegateOperation`), so it can be separated again, and `MeshData2`,
//! the carved mesh in its own space at `InitialSize`, which Studio and the
//! viewer draw and which every later boolean, the physics and the mass read
//! back (see [`local_mesh`]).

use rbx_csg::glam::{DMat3, DMat4, DVec3};
use rbx_csg::{csgmdl, shapes, MeshData, Solid};
use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// `SharedString`, the wire type `ChildData2` and `MeshData2` are saved as.
const SHARED_STRING: u8 = 0x1c;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Union,
    Subtract,
    Intersect,
}

/// One operation made: its referent, and its mesh in its own space (the
/// geometry the physics collides with).
pub struct Made {
    pub referent: Ref,
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

fn read<'a>(
    db: &'a ReflectionDatabase,
    instance: &'a Instance,
    name: &'a str,
) -> Option<&'a Variant> {
    db.stored_or_default(instance, name).map(|(_, v)| v)
}

fn vec3(v: Vector3Data) -> DVec3 {
    DVec3::new(v.x.into(), v.y.into(), v.z.into())
}

fn vector3(v: DVec3) -> Vector3Data {
    Vector3Data {
        x: v.x as f32,
        y: v.y as f32,
        z: v.z as f32,
    }
}

/// A `CFrame` as a matrix (rows of `rotation` are the matrix's rows).
pub fn frame(cf: &CFrameData) -> DMat4 {
    let r = cf.rotation.map(f64::from);
    DMat4::from_mat3_translation(
        DMat3::from_cols(
            DVec3::new(r[0], r[3], r[6]),
            DVec3::new(r[1], r[4], r[7]),
            DVec3::new(r[2], r[5], r[8]),
        ),
        vec3(cf.position),
    )
}

fn cframe(m: DMat4) -> CFrameData {
    let (x, y, z) = (m.x_axis, m.y_axis, m.z_axis);
    CFrameData {
        position: vector3(m.w_axis.truncate()),
        rotation: [x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z].map(|v| v as f32),
    }
}

fn size_of(db: &ReflectionDatabase, instance: &Instance) -> DVec3 {
    match read(db, instance, "Size") {
        Some(Variant::Vector3(v)) => vec3(*v),
        _ => DVec3::ONE,
    }
}

fn cframe_of(db: &ReflectionDatabase, instance: &Instance) -> CFrameData {
    match read(db, instance, "CFrame") {
        Some(Variant::CFrame(cf)) => *cf,
        _ => CFrameData {
            position: vector3(DVec3::ZERO),
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        },
    }
}

fn shared_bytes<'a>(instance: &'a Instance, name: &str) -> Option<&'a [u8]> {
    match instance.properties().get(name) {
        Some(Variant::Unknown { raw, .. }) if !raw.is_empty() => Some(raw),
        _ => None,
    }
}

/// An operation's carved mesh in its own space at its `InitialSize`, read
/// back from its `MeshData2`: `None` for one whose mesh is not a version 5
/// blob (a legacy union), which then counts as its box.
pub fn local_mesh(instance: &Instance) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    csgmdl::decode(shared_bytes(instance, "MeshData2")?)
}

/// The unit mesh (fitting `[-0.5, 0.5]³`) and the scale a primitive part
/// draws with: a ball takes its smallest side, a cylinder its smaller
/// cross-section side.
fn primitive(db: &ReflectionDatabase, instance: &Instance, size: DVec3) -> (MeshData, DVec3) {
    let class = instance.class();
    let shape = match read(db, instance, "Shape") {
        Some(Variant::Enum(n)) if db.is_subclass_of(class, "Part") => *n,
        _ => 1,
    };
    match (class, shape) {
        ("WedgePart", _) | (_, 3) if class != "CornerWedgePart" => (shapes::wedge(), size),
        ("CornerWedgePart", _) | (_, 4) => (shapes::corner_wedge(), size),
        (_, 0) => (shapes::sphere(), DVec3::splat(size.min_element())),
        (_, 2 | 5) => {
            let d = size.y.min(size.z);
            (shapes::cylinder_x(), DVec3::new(size.x, d, d))
        }
        _ => (shapes::block(), size),
    }
}

/// `part`'s solid in world space. `Err` names why it can't take part.
pub fn world_solid(dom: &WeakDom, db: &ReflectionDatabase, part: Ref) -> Result<Solid, String> {
    let instance = dom.get(part).ok_or("The part no longer exists")?;
    let class = instance.class();
    if !db.is_subclass_of(class, "BasePart") || class == "Terrain" {
        return Err(format!("{} is not a part", instance.name()));
    }
    let size = size_of(db, instance);
    let at = frame(&cframe_of(db, instance));
    if db.is_subclass_of(class, "PartOperation") {
        let initial = match read(db, instance, "InitialSize") {
            Some(Variant::Vector3(v)) => vec3(*v),
            _ => size,
        };
        if let Some((positions, indices)) = local_mesh(instance) {
            let scale = size / initial.max(DVec3::splat(1e-6));
            return Ok(Solid::from_triangles(
                &positions,
                &indices,
                at * DMat4::from_scale(scale),
            ));
        }
    }
    let (mesh, scale) = primitive(db, instance, size);
    Ok(Solid::from_triangles(
        &mesh.positions,
        &mesh.indices,
        at * DMat4::from_scale(scale),
    ))
}

fn is_negate(dom: &WeakDom, part: Ref) -> bool {
    dom.get(part)
        .is_some_and(|i| i.class() == "NegateOperation")
}

/// Whether `other` carves rather than adds: everything after the base of a
/// subtraction, and a `NegateOperation` in a union.
fn carves(dom: &WeakDom, operation: Operation, other: Ref) -> bool {
    match operation {
        Operation::Subtract => true,
        Operation::Union => is_negate(dom, other),
        Operation::Intersect => false,
    }
}

/// The boolean of `base` with `others` in world space, every subtraction
/// after the unions and intersections. `Err` is the kernel's failure, by
/// name.
pub fn combine(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    operation: Operation,
    base: Ref,
    others: &[Ref],
) -> Result<Solid, String> {
    let fail = |f: rbx_csg::Failure| format!("{f:?}");
    let mut result = world_solid(dom, db, base)?;
    let mut cutters = Vec::new();
    for &other in others {
        let solid = world_solid(dom, db, other)?;
        if carves(dom, operation, other) {
            cutters.push(solid);
            continue;
        }
        result = match operation {
            Operation::Intersect => result.intersect(solid),
            _ => result.union(solid),
        }
        .map_err(fail)?;
    }
    for cutter in cutters {
        result = result.subtract(cutter).map_err(fail)?;
    }
    result.finish(None).map_err(fail)
}

/// The frame an operation made from `solid` takes: `base`'s rotation,
/// centred on the solid's bounds in that rotation, and the size of those
/// bounds.
fn fit(solid: &Solid, rotation: DMat4) -> (DMat4, DVec3) {
    let local = solid.transformed(rotation.inverse());
    let (lo, hi) = local.bounds().unwrap_or((DVec3::ZERO, DVec3::ZERO));
    (rotation * DMat4::from_translation((lo + hi) / 2.0), hi - lo)
}

fn color_of(db: &ReflectionDatabase, instance: &Instance) -> [u8; 3] {
    match read(db, instance, "Color") {
        Some(Variant::Color3uint8 { r, g, b }) => [*r, *g, *b],
        Some(Variant::Color3(c)) => [c.r, c.g, c.b].map(|v| (v * 255.0).round() as u8),
        _ => [163, 162, 165],
    }
}

/// Sets `name` under the spelling `class` saves it as.
fn set(dom: &mut WeakDom, db: &ReflectionDatabase, referent: Ref, name: &str, value: Variant) {
    let Some(class) = dom.get(referent).map(|i| i.class().to_owned()) else {
        return;
    };
    let key = db.stored_names(&class, name)[0].to_owned();
    let _ = dom.set_property(referent, &key, value);
}

fn shared(raw: Vec<u8>) -> Variant {
    Variant::Unknown {
        type_id: SHARED_STRING,
        raw,
    }
}

/// `source`'s instance copied into `into` under `parent` as `class`,
/// placed at `cframe`, children left behind.
fn copy_into(
    into: &mut WeakDom,
    source: &Instance,
    class: &str,
    parent: Option<Ref>,
    cframe: CFrameData,
) -> Ref {
    let referent = into.new_instance(class, source.name(), parent);
    if let Some(copy) = into.get_mut(referent) {
        *copy.properties_mut() = source.properties().clone();
        copy.properties_mut()
            .insert("CFrame".to_owned(), Variant::CFrame(cframe));
    }
    referent
}

/// The `ChildData2` document: every source part in the operation's own
/// space, a subtracted one inside a `NegateOperation` as Studio nests it.
fn child_data(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    operation: Operation,
    parts: &[Ref],
    inverse: DMat4,
) -> Option<Vec<u8>> {
    let mut nested = WeakDom::new();
    for (i, &part) in parts.iter().enumerate() {
        let instance = dom.get(part)?;
        let local = cframe(inverse * frame(&cframe_of(db, instance)));
        if i > 0 && carves(dom, operation, part) && instance.class() != "NegateOperation" {
            let solid = world_solid(dom, db, part).ok()?;
            negated(&mut nested, db, instance, local, &solid)?;
        } else {
            copy_into(&mut nested, instance, instance.class(), None, local);
        }
    }
    rbx_binary::serialize(&nested).ok()
}

/// A `NegateOperation` of `part` (whose world solid is `solid`) made in
/// `into` at `at`, holding the part as Studio nests it: its own `CFrame` an
/// echo of the operation's rotation, and the part's mesh as its bake.
fn negated(
    into: &mut WeakDom,
    db: &ReflectionDatabase,
    part: &Instance,
    at: CFrameData,
    solid: &Solid,
) -> Option<Ref> {
    const CLASS: &str = "NegateOperation";
    let negate = copy_into(into, part, CLASS, None, at);
    let size = Variant::Vector3(vector3(size_of(db, part)));
    let world = frame(&cframe_of(db, part));
    let mut inner = WeakDom::new();
    let mut echo = at;
    echo.position = vector3(DVec3::ZERO);
    copy_into(&mut inner, part, part.class(), None, echo);
    let raw = rbx_binary::serialize(&inner).ok()?;
    let props = into.get_mut(negate)?.properties_mut();
    props.retain(|k, _| {
        db.resolve_property(CLASS, db.canonical_name(CLASS, k))
            .is_some()
    });
    props.insert("InitialSize".to_owned(), size);
    props.insert("ChildData2".to_owned(), shared(raw));
    if let Some(blob) = csgmdl::encode(&solid.transformed(world.inverse()), color_of(db, part)) {
        props.insert("MeshData2".to_owned(), shared(blob));
    }
    Some(negate)
}

/// The parts among `objects` an operation is made of, the first one that
/// adds first: what Studio's Union, Intersect and Negate act on.
fn parts_of(dom: &WeakDom, db: &ReflectionDatabase, objects: &[Ref]) -> Vec<Ref> {
    let mut parts: Vec<Ref> = objects
        .iter()
        .copied()
        .filter(|&r| {
            dom.get(r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart") && i.class() != "Terrain")
        })
        .collect();
    if let Some(first) = parts.iter().position(|&p| !is_negate(dom, p)) {
        let base = parts.remove(first);
        parts.insert(0, base);
    }
    parts
}

/// Studio's Union and Intersect (`plugin:Union`, `plugin:Intersect`, the
/// Model tab): the parts among `objects` become one operation, parented
/// where the first part was, and leave the place. `NegateOperation`s in a
/// union are carved out.
pub fn operate_in_place(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    operation: Operation,
    objects: &[Ref],
) -> Result<Ref, String> {
    let parts = parts_of(dom, db, objects);
    let (&base, others) = parts.split_first().ok_or("No parts were selected")?;
    if is_negate(dom, base) {
        return Err("Cannot union only negated parts".to_owned());
    }
    let solid = combine(dom, db, operation, base, others)?;
    let parent = dom.parent(base);
    let made = build(dom, db, operation, Flavor::Part, base, others, solid, false);
    let referent = made.first().ok_or("The operation made nothing")?.referent;
    dom.set_parent(referent, parent);
    for part in parts {
        dom.remove(part);
    }
    Ok(referent)
}

/// Studio's Negate: each part becomes a `NegateOperation` in its place, and
/// a `NegateOperation` becomes its part again.
pub fn negate_in_place(dom: &mut WeakDom, db: &ReflectionDatabase, objects: &[Ref]) -> Vec<Ref> {
    let mut out = Vec::new();
    for part in parts_of(dom, db, objects) {
        if is_negate(dom, part) {
            out.extend(separate_in_place(dom, db, &[part]));
            continue;
        }
        let Ok(solid) = world_solid(dom, db, part) else {
            continue;
        };
        let Some(instance) = dom.get(part).cloned() else {
            continue;
        };
        let parent = dom.parent(part);
        let mut staged = WeakDom::new();
        let Some(negate) = negated(&mut staged, db, &instance, cframe_of(db, &instance), &solid)
        else {
            continue;
        };
        let Some(made) = staged.get(negate) else {
            continue;
        };
        let referent = dom.new_instance(made.class(), made.name(), parent);
        for (key, value) in made.properties() {
            let _ = dom.set_property(referent, key, value.clone());
        }
        dom.remove(part);
        out.push(referent);
    }
    out
}

/// Studio's Separate: each operation among `objects` gives back the parts
/// (and `NegateOperation`s) its `ChildData2` holds, placed in the world as
/// the operation stands now, and leaves the place.
pub fn separate_in_place(dom: &mut WeakDom, db: &ReflectionDatabase, objects: &[Ref]) -> Vec<Ref> {
    let mut out = Vec::new();
    for &op in objects {
        let Some(instance) = dom.get(op) else {
            continue;
        };
        if !db.is_subclass_of(instance.class(), "PartOperation") {
            continue;
        }
        let Some(nested) =
            shared_bytes(instance, "ChildData2").and_then(|raw| rbx_binary::deserialize(raw).ok())
        else {
            continue;
        };
        let size = size_of(db, instance);
        let initial = match read(db, instance, "InitialSize") {
            Some(Variant::Vector3(v)) => vec3(*v),
            _ => size,
        };
        let ratio = size / initial.max(DVec3::splat(1e-6));
        let at = frame(&cframe_of(db, instance));
        let echo = instance.class() == "NegateOperation";
        let parent = dom.parent(op);
        for &root in nested.root_refs() {
            let Some(child) = nested.get(root) else {
                continue;
            };
            let local = frame(&cframe_of(db, child));
            let placed = if echo {
                at
            } else {
                let mut local = local;
                local.w_axis = (local.w_axis.truncate() * ratio).extend(1.0);
                at * local
            };
            let referent = dom.new_instance(child.class(), child.name(), parent);
            for (key, value) in child.properties() {
                let _ = dom.set_property(referent, key, value.clone());
            }
            set(dom, db, referent, "CFrame", Variant::CFrame(cframe(placed)));
            if ratio != DVec3::ONE {
                let rotation = DMat3::from_mat4(local);
                let scale = (rotation.transpose() * ratio).abs();
                let size = size_of(db, child) * scale;
                set(dom, db, referent, "Size", Variant::Vector3(vector3(size)));
            }
            out.push(referent);
        }
        dom.remove(op);
    }
    out
}

/// Who asked for an operation, which decides what it is called and where
/// its frame sits (the CsgApi golden).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// `BasePart:UnionAsync` and its siblings, and Studio's own commands: a
    /// `UnionOperation` "Union" (an `IntersectOperation` "Intersection"),
    /// centred on its bounds in the base part's rotation.
    Part,
    /// `GeometryService`: a `PartOperation` "PartOperation" in the base
    /// part's own frame, its mesh wherever it lies in it.
    Service,
}

/// Makes the operation(s) `solid` (world space) becomes, unparented, from
/// `base` and `others`: one per piece when `split` asks for it. Each keeps
/// `base`'s properties where its class has them. Only a whole, centred
/// operation saves the parts' tree: a piece of a split would carve every
/// piece back, and the viewer checks a tree against a centred bake.
#[allow(clippy::too_many_arguments)]
pub fn build(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    operation: Operation,
    flavor: Flavor,
    base: Ref,
    others: &[Ref],
    solid: Solid,
    split: bool,
) -> Vec<Made> {
    let (class, name) = match (flavor, operation) {
        (Flavor::Service, _) => ("PartOperation", "PartOperation"),
        (Flavor::Part, Operation::Intersect) => ("IntersectOperation", "Intersection"),
        (Flavor::Part, _) => ("UnionOperation", "Union"),
    };
    let Some(base_instance) = dom.get(base) else {
        return Vec::new();
    };
    let base_props = base_instance.properties().clone();
    let base_frame = frame(&cframe_of(db, base_instance));
    let color = color_of(db, base_instance);
    let rotation = DMat4::from_mat3(DMat3::from_mat4(base_frame));
    let pieces = if split {
        solid.split_apart()
    } else {
        vec![solid]
    };
    let tree = pieces.len() == 1 && flavor == Flavor::Part;
    let mut parts = vec![base];
    parts.extend_from_slice(others);
    let mut made = Vec::new();
    for piece in pieces {
        let (at, size) = match flavor {
            Flavor::Part => fit(&piece, rotation),
            Flavor::Service => {
                let (lo, hi) = piece
                    .transformed(base_frame.inverse())
                    .bounds()
                    .unwrap_or_default();
                (base_frame, hi - lo)
            }
        };
        let local = piece.transformed(at.inverse());
        let referent = dom.new_instance(class, name, None);
        if let Some(instance) = dom.get_mut(referent) {
            *instance.properties_mut() = base_props
                .iter()
                .filter(|(k, _)| {
                    db.resolve_property(class, db.canonical_name(class, k))
                        .is_some()
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
        }
        set(dom, db, referent, "CFrame", Variant::CFrame(cframe(at)));
        set(dom, db, referent, "Size", Variant::Vector3(vector3(size)));
        set(
            dom,
            db,
            referent,
            "InitialSize",
            Variant::Vector3(vector3(size)),
        );
        // A fresh operation's own settings, whatever the base had (CsgApi).
        set(dom, db, referent, "CollisionFidelity", Variant::Enum(0));
        set(dom, db, referent, "RenderFidelity", Variant::Enum(0));
        set(dom, db, referent, "SmoothingAngle", Variant::Float32(0.0));
        set(dom, db, referent, "UsePartColor", Variant::Bool(false));
        if let Some(blob) = csgmdl::encode(&local, color) {
            set(dom, db, referent, "MeshData2", shared(blob));
        }
        if let Some(raw) = tree
            .then(|| child_data(dom, db, operation, &parts, at.inverse()))
            .flatten()
        {
            set(dom, db, referent, "ChildData2", shared(raw));
        }
        let mesh = local.to_mesh();
        made.push(Made {
            referent,
            vertices: mesh.vertices.iter().map(|v| v.position).collect(),
            triangles: mesh.indices.as_chunks::<3>().0.to_vec(),
        });
    }
    made
}

#[cfg(test)]
mod tests;
