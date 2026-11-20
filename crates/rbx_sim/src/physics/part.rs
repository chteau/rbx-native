//! A part or a joint as physics reads it from the DOM.

use std::f64::consts::PI;
use std::sync::Arc;

use rapier3d::prelude::*;
use rbx_dom::{CFrameData, Instance, PhysicalProperties, Ref, Variant, Vector3Data};
use rbx_reflection::ReflectionDatabase;

/// `Enum.Material.Plastic`, what a part with no material reads as.
pub(crate) const PLASTIC: [f32; 5] = [0.7, 0.3, 0.5, 1.0, 1.0];

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Block,
    Ball,
    /// Along the part's X axis, as Roblox's cylinder is.
    Cylinder,
    Wedge,
    CornerWedge,
    /// Along X like the cylinder (an assumption: the docs don't say).
    Capsule,
    /// A `MeshPart`: collides per its `CollisionFidelity`, see `mesh.rs`.
    Mesh,
    /// A union's convex pieces, in its space as made, and the size it was
    /// made at (`union.rs`).
    Union(Arc<Vec<super::union::Hull>>, Vector3Data),
}

pub(crate) struct PartData {
    pub(crate) referent: Ref,
    pub(super) shape: Shape,
    pub(crate) size: Vector3Data,
    pub(crate) cframe: CFrameData,
    pub(super) anchored: bool,
    pub(crate) can_query: bool,
    pub(crate) can_collide: bool,
    pub(super) can_touch: bool,
    pub(super) massless: bool,
    pub(crate) collision_group: String,
    /// Density, friction, elasticity, friction weight, elasticity weight.
    pub(super) physics: [f32; 5],
    /// `MeshPart.MeshId` and the name of its `CollisionFidelity`.
    pub(super) mesh_id: String,
    pub(super) fidelity: String,
    pub(super) linear_velocity: Vector3Data,
    pub(super) angular_velocity: Vector3Data,
}

fn read<'a>(
    db: &'a ReflectionDatabase,
    instance: &'a Instance,
    name: &'a str,
) -> Option<&'a Variant> {
    db.stored_or_default(instance, name).map(|(_, v)| v)
}

fn flag(db: &ReflectionDatabase, instance: &Instance, name: &str, default: bool) -> bool {
    match read(db, instance, name) {
        Some(Variant::Bool(on)) => *on,
        _ => default,
    }
}

fn vector(db: &ReflectionDatabase, instance: &Instance, names: &[&str]) -> Vector3Data {
    names
        .iter()
        .find_map(|name| match read(db, instance, name) {
            Some(Variant::Vector3(v)) => Some(*v),
            _ => None,
        })
        .unwrap_or(Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        })
}

impl PartData {
    pub(crate) fn read(db: &ReflectionDatabase, instance: &Instance) -> Option<PartData> {
        let class = instance.class();
        // Terrain is a BasePart with voxels, not a shape: not simulated yet.
        if !db.is_subclass_of(class, "BasePart") || class == "Terrain" {
            return None;
        }
        let shape = match class {
            "WedgePart" => Shape::Wedge,
            "CornerWedgePart" => Shape::CornerWedge,
            "MeshPart" => Shape::Mesh,
            _ if db.is_subclass_of(class, "PartOperation") => {
                match (
                    read(db, instance, "PhysicalConfigData"),
                    read(db, instance, "InitialSize"),
                ) {
                    (Some(Variant::Unknown { raw, .. }), Some(Variant::Vector3(made))) => {
                        match super::union::hulls(raw) {
                            Some(hulls) => Shape::Union(hulls, *made),
                            None => Shape::Block,
                        }
                    }
                    _ => Shape::Block,
                }
            }
            _ if db.is_subclass_of(class, "Part") => match read(db, instance, "Shape") {
                Some(Variant::Enum(0)) => Shape::Ball,
                Some(Variant::Enum(2)) => Shape::Cylinder,
                Some(Variant::Enum(3)) => Shape::Wedge,
                Some(Variant::Enum(4)) => Shape::CornerWedge,
                Some(Variant::Enum(5)) => Shape::Capsule,
                _ => Shape::Block,
            },
            _ => Shape::Block,
        };
        let cframe = match read(db, instance, "CFrame") {
            Some(Variant::CFrame(cf)) => *cf,
            _ => return None,
        };
        let collision_group = match read(db, instance, "CollisionGroup") {
            Some(Variant::String(name)) if !name.is_empty() => name.clone(),
            _ => "Default".to_owned(),
        };
        Some(PartData {
            referent: instance.referent(),
            shape,
            size: vector(db, instance, &["Size"]),
            cframe,
            anchored: flag(db, instance, "Anchored", false),
            can_query: flag(db, instance, "CanQuery", true),
            can_collide: flag(db, instance, "CanCollide", true),
            can_touch: flag(db, instance, "CanTouch", true),
            massless: flag(db, instance, "Massless", false),
            collision_group,
            physics: physics(db, instance),
            mesh_id: mesh_id(db, instance),
            fidelity: match read(db, instance, "CollisionFidelity") {
                Some(Variant::Enum(raw)) => db
                    .enum_name("CollisionFidelity", *raw)
                    .map(|name| name.to_string()),
                _ => None,
            }
            .unwrap_or_else(|| "Default".to_owned()),
            linear_velocity: vector(db, instance, &["AssemblyLinearVelocity", "Velocity"]),
            angular_velocity: vector(db, instance, &["AssemblyAngularVelocity", "RotVelocity"]),
        })
    }

    /// The part's volume as Roblox counts it for `Mass`: exact for blocks,
    /// wedges, balls; the cylinder and capsule as their solid of revolution.
    /// A `MeshPart` counts as its `Size` box whatever its fidelity: the
    /// mesh's own volume is not computed.
    pub(super) fn volume(&self) -> f64 {
        let [x, y, z] = [self.size.x, self.size.y, self.size.z].map(f64::from);
        match self.shape {
            Shape::Block | Shape::Mesh | Shape::Union(..) => x * y * z,
            Shape::Wedge => x * y * z / 2.0,
            Shape::CornerWedge => x * y * z / 3.0,
            Shape::Ball => {
                let d = x.min(y).min(z);
                PI * d * d * d / 6.0
            }
            Shape::Cylinder => {
                let r = y.min(z) / 2.0;
                PI * r * r * x
            }
            Shape::Capsule => {
                let r = y.min(z) / 2.0;
                let body = (x - 2.0 * r).max(0.0);
                PI * r * r * body + 4.0 / 3.0 * PI * r * r * r
            }
        }
    }

    pub(crate) fn shape_collider(&self) -> ColliderBuilder {
        let half = |v: f32| Real::from(v) / 2.0;
        let (hx, hy, hz) = (half(self.size.x), half(self.size.y), half(self.size.z));
        let along_x = Pose::from_rotation(Rotation::from_rotation_z(std::f64::consts::FRAC_PI_2));
        match &self.shape {
            Shape::Block => ColliderBuilder::cuboid(hx, hy, hz),
            Shape::Union(hulls, made) => {
                let scale = [
                    self.size.x / made.x,
                    self.size.y / made.y,
                    self.size.z / made.z,
                ]
                .map(|s| {
                    if s.is_finite() && s > 0.0 {
                        Real::from(s)
                    } else {
                        1.0
                    }
                });
                let pieces: Vec<_> = hulls
                    .iter()
                    .filter_map(|piece| {
                        let points: Vec<Vector> = piece
                            .iter()
                            .map(|p| {
                                Vector::new(
                                    Real::from(p[0]) * scale[0],
                                    Real::from(p[1]) * scale[1],
                                    Real::from(p[2]) * scale[2],
                                )
                            })
                            .collect();
                        SharedShape::convex_hull(&points).map(|s| (Pose::IDENTITY, s))
                    })
                    .collect();
                if pieces.is_empty() {
                    ColliderBuilder::cuboid(hx, hy, hz)
                } else {
                    ColliderBuilder::compound(pieces)
                }
            }
            Shape::Mesh => super::mesh::collider(self),
            Shape::Ball => ColliderBuilder::ball(hx.min(hy).min(hz)),
            Shape::Cylinder => {
                ColliderBuilder::compound(vec![(along_x, SharedShape::cylinder(hx, hy.min(hz)))])
            }
            Shape::Capsule => {
                let r = hy.min(hz);
                ColliderBuilder::compound(vec![(
                    along_x,
                    SharedShape::capsule_y((hx - r).max(0.0), r),
                )])
            }
            Shape::Wedge => hull(&[
                [-hx, -hy, -hz],
                [hx, -hy, -hz],
                [-hx, -hy, hz],
                [hx, -hy, hz],
                [-hx, hy, hz],
                [hx, hy, hz],
            ])
            .unwrap_or_else(|| ColliderBuilder::cuboid(hx, hy, hz)),
            Shape::CornerWedge => hull(&[
                [-hx, -hy, -hz],
                [hx, -hy, -hz],
                [-hx, -hy, hz],
                [hx, -hy, hz],
                [hx, hy, -hz],
            ])
            .unwrap_or_else(|| ColliderBuilder::cuboid(hx, hy, hz)),
        }
    }
}

/// `MeshPart.MeshId`: a string, or a `Content` whose URI is the first
/// quoted text of its debug form.
fn mesh_id(db: &ReflectionDatabase, instance: &Instance) -> String {
    let legacy = read(db, instance, "MeshId").or_else(|| read(db, instance, "MeshID"));
    // Studio now saves only the `Content` twin.
    let set = |v: &Variant| !matches!(v, Variant::String(s) if s.is_empty());
    match legacy
        .filter(|v| set(v))
        .or_else(|| read(db, instance, "MeshContent"))
    {
        Some(Variant::String(uri)) => uri.clone(),
        Some(Variant::Content(content)) => {
            let text = format!("{content:?}");
            text.split('"').nth(1).unwrap_or_default().to_owned()
        }
        _ => String::new(),
    }
}

fn hull(points: &[[Real; 3]]) -> Option<ColliderBuilder> {
    let points: Vec<Vector> = points
        .iter()
        .map(|p| Vector::new(p[0], p[1], p[2]))
        .collect();
    ColliderBuilder::convex_hull(&points)
}

/// `CustomPhysicalProperties` when set, else the material's defaults
/// (`rbx_materials::physics`, from the creator-docs table).
fn physics(db: &ReflectionDatabase, instance: &Instance) -> [f32; 5] {
    if let Some(Variant::PhysicalProperties(PhysicalProperties::Custom {
        density,
        friction,
        elasticity,
        friction_weight,
        elasticity_weight,
    })) = read(db, instance, "CustomPhysicalProperties")
    {
        return [
            *density,
            *friction,
            *elasticity,
            *friction_weight,
            *elasticity_weight,
        ];
    }
    let material = match read(db, instance, "Material") {
        Some(Variant::Enum(raw)) => db.enum_name("Material", *raw),
        _ => None,
    };
    material
        .and_then(|name| {
            rbx_materials::physics::PHYSICS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, p)| *p)
        })
        .unwrap_or(PLASTIC)
}

/// A rigid joint between two parts.
pub(super) struct Joint {
    pub(super) referent: Ref,
    pub(super) part0: Ref,
    pub(super) part1: Ref,
}

impl Joint {
    pub(super) fn read(db: &ReflectionDatabase, instance: &Instance) -> Option<Joint> {
        let class = instance.class();
        if !(db.is_subclass_of(class, "JointInstance") || class == "WeldConstraint") {
            return None;
        }
        if !flag(db, instance, "Enabled", true) {
            return None;
        }
        let part = |names: &[&str]| {
            names
                .iter()
                .find_map(|name| match instance.properties().get(*name) {
                    Some(Variant::Ref(r)) => Some(*r),
                    _ => None,
                })
        };
        Some(Joint {
            referent: instance.referent(),
            part0: part(&["Part0", "Part0Internal"])?,
            part1: part(&["Part1", "Part1Internal"])?,
        })
    }
}
