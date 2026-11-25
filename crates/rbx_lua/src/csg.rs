//! Solid modeling over a DOM: unions, negations and intersections of
//! parts, carved with the shared [`rbx_csg`] kernel, made into the
//! `UnionOperation` / `IntersectOperation` Studio would make. Scripts reach
//! it through `BasePart:UnionAsync` and `GeometryService` (see
//! `server::csg`), plugins through `plugin:Union` and its siblings, and the
//! editor's Model tab directly.
//!
//! An operation saves what Studio saves: `ChildData2`, a nested `.rbxm` of
//! the parts it was made from in its own space (a subtracted part wrapped
//! in a `NegateOperation`), so it can be separated again, and
//! `SolidMeshHolder`, the carved mesh in its own space at `InitialSize`,
//! each face in its source part's colour, which Studio draws and which
//! every later boolean, the physics and the mass read back (see
//! [`local_mesh`]); `PhysicalConfigData`, the convex pieces Studio collides
//! it as at its `CollisionFidelity` (see [`refresh_collision`]); `MeshData2`
//! stays empty, as in Studio's own saves, and the mass properties Studio
//! saves beside the mesh are filled in.

use rbx_csg::csgphs::{self, Fidelity};
use rbx_csg::glam::{DMat3, DMat4, DVec3};
use rbx_csg::{csgmdl, shapes, solidmesh, MeshData, Solid};
use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// `SharedString`, the wire type `ChildData2` and `SolidMeshHolder` are
/// saved as.
const SHARED_STRING: u8 = 0x1c;

/// Where real Studio caches an operation's geometry: the solid in
/// `SolidMeshHolder` (`MeshData2` in older saves), the collision pieces in
/// `PhysicalConfigData`. An operation carved again here must lose them.
const STALE_GEOMETRY: [&str; 3] = ["SolidMeshHolder", "MeshData2", "PhysicalConfigData"];

/// Drops the cached geometry ([`STALE_GEOMETRY`]) an operation whose mesh
/// was just (re)computed still carries from before.
pub fn clear_stale_geometry(instance: &mut Instance) {
    instance
        .properties_mut()
        .retain(|k, _| !STALE_GEOMETRY.contains(&k.as_str()));
}

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

fn solid_mesh(instance: &Instance) -> Option<solidmesh::Decoded> {
    solidmesh::decode(shared_bytes(instance, "SolidMeshHolder")?)
}

/// An operation's carved mesh in its own space at its `InitialSize`, read
/// back from its `SolidMeshHolder`, or a version 5 `MeshData2`: `None` for
/// one with neither (a legacy union), which then counts as its box.
pub fn local_mesh(instance: &Instance) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    if let Some(mesh) = solid_mesh(instance) {
        return Some((mesh.positions, mesh.triangles.concat()));
    }
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
    let color = color_of(db, instance);
    if db.is_subclass_of(class, "PartOperation") {
        let initial = match read(db, instance, "InitialSize") {
            Some(Variant::Vector3(v)) => vec3(*v),
            _ => size,
        };
        let model = at * DMat4::from_scale(size / initial.max(DVec3::splat(1e-6)));
        let part_color = matches!(
            read(db, instance, "UsePartColor"),
            Some(Variant::Bool(true))
        );
        if let Some(mesh) = solid_mesh(instance) {
            let solid = mesh.solid(model);
            return Ok(if part_color {
                solid.painted(color)
            } else {
                solid
            });
        }
        if let Some((positions, indices)) = local_mesh(instance) {
            return Ok(Solid::from_triangles(&positions, &indices, model).painted(color));
        }
    }
    if let Some(solid) = mesh::world_solid(db, instance, size, at) {
        return solid.map(|solid| solid.painted(color));
    }
    let (mesh, scale) = primitive(db, instance, size);
    Ok(Solid::from_triangles(
        &mesh.positions,
        &mesh.indices,
        at * DMat4::from_scale(scale),
    )
    .painted(color))
}

/// The tag Studio's Negate puts on a part (with `Transparency` 0.5): a
/// union carves the part out.
pub const NEGATED_TAG: &str = "rbxNegated";

fn has_negated_tag(instance: &Instance) -> bool {
    rbx_dom::attributes::tags(instance.properties().get("Tags")).contains(&NEGATED_TAG)
}

/// A negated part: tagged by Studio's Negate, or a `NegateOperation` (older
/// Studio's Negate, and `plugin:Negate`).
fn is_negate(dom: &WeakDom, part: Ref) -> bool {
    dom.get(part)
        .is_some_and(|i| i.class() == "NegateOperation" || has_negated_tag(i))
}

/// Whether `other` carves rather than adds: everything after the base of a
/// subtraction, and a negated part in a union.
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

fn triangle_count(solid: &Solid) -> i32 {
    solid
        .polygons
        .iter()
        .map(|p| p.vertices.len().saturating_sub(2) as i32)
        .sum()
}

fn shared(raw: Vec<u8>) -> Variant {
    Variant::Unknown {
        type_id: SHARED_STRING,
        raw,
    }
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
            clear_stale_geometry(instance);
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
        set(
            dom,
            db,
            referent,
            "SolidMeshHolder",
            shared(solidmesh::encode(&local)),
        );
        set(
            dom,
            db,
            referent,
            "PhysicalConfigData",
            shared(csgphs::encode(&local, Fidelity::Default)),
        );
        set(
            dom,
            db,
            referent,
            "TriangleCount",
            Variant::Int32(triangle_count(&local)),
        );
        if let Some(mass) = local.mass_properties() {
            for (name, value) in [
                ("UnscaledCofm", mass.centroid),
                ("UnscaledVolInertiaDiags", mass.inertia_diagonal),
                ("UnscaledVolInertiaOffDiags", mass.inertia_off_diagonal),
            ] {
                set(dom, db, referent, name, Variant::Vector3(vector3(value)));
            }
            set(
                dom,
                db,
                referent,
                "UnscaledVolume",
                Variant::Float32(mass.volume as f32),
            );
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

/// Rewrites `operation`'s `PhysicalConfigData` for the `CollisionFidelity`
/// it was just given, as Studio does when the property changes: Studio
/// never saves the property and reads it back from the blob, so a stale
/// blob would undo the change on the next load. `false` when nothing
/// needed doing (no such operation, no fidelity set, already matching).
pub fn refresh_collision(dom: &mut WeakDom, db: &ReflectionDatabase, operation: Ref) -> bool {
    let Some(instance) = dom.get(operation) else {
        return false;
    };
    if !db.is_subclass_of(instance.class(), "PartOperation")
        || instance.class() == "NegateOperation"
    {
        return false;
    }
    let Some(Variant::Enum(value)) = instance.properties().get("CollisionFidelity") else {
        return false;
    };
    let wanted = Fidelity::from_value(*value);
    if shared_bytes(instance, "PhysicalConfigData").and_then(csgphs::fidelity) == Some(wanted) {
        return false;
    }
    let Some(mesh) = solid_mesh(instance) else {
        return false;
    };
    let blob = csgphs::encode(&mesh.solid(DMat4::IDENTITY), wanted);
    set(dom, db, operation, "PhysicalConfigData", shared(blob));
    true
}

mod mesh;
mod place;
pub(crate) mod script;
mod tree;

pub use mesh::{set_mesh_source, MeshSource, NON_MANIFOLD};

pub use place::{negate_in_place, negate_operations_in_place, operate_in_place, separate_in_place};
use tree::child_data;

#[cfg(test)]
mod tests;
