//! A part or a joint as physics reads it from the DOM.

use std::f64::consts::PI;

use rapier3d::prelude::*;
use rbx_dom::{CFrameData, Instance, PhysicalProperties, Ref, Variant, Vector3Data};
use rbx_reflection::ReflectionDatabase;

/// `Enum.Material.Plastic`, what a part with no material reads as.
const PLASTIC: [f32; 5] = [0.7, 0.3, 0.5, 1.0, 1.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Block,
    Ball,
    /// Along the part's X axis, as Roblox's cylinder is.
    Cylinder,
    Wedge,
    CornerWedge,
    /// Along X like the cylinder (an assumption: the docs don't say).
    Capsule,
}

pub(super) struct PartData {
    pub(super) referent: Ref,
    pub(super) shape: Shape,
    pub(super) size: Vector3Data,
    pub(super) cframe: CFrameData,
    pub(super) anchored: bool,
    pub(super) can_collide: bool,
    pub(super) can_touch: bool,
    pub(super) massless: bool,
    pub(super) collision_group: String,
    /// Density, friction, elasticity, friction weight, elasticity weight.
    pub(super) physics: [f32; 5],
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
    pub(super) fn read(db: &ReflectionDatabase, instance: &Instance) -> Option<PartData> {
        let class = instance.class();
        // Terrain is a BasePart with voxels, not a shape: not simulated yet.
        if !db.is_subclass_of(class, "BasePart") || class == "Terrain" {
            return None;
        }
        let shape = match class {
            "WedgePart" => Shape::Wedge,
            "CornerWedgePart" => Shape::CornerWedge,
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
            can_collide: flag(db, instance, "CanCollide", true),
            can_touch: flag(db, instance, "CanTouch", true),
            massless: flag(db, instance, "Massless", false),
            collision_group,
            physics: physics(db, instance),
            linear_velocity: vector(db, instance, &["AssemblyLinearVelocity", "Velocity"]),
            angular_velocity: vector(db, instance, &["AssemblyAngularVelocity", "RotVelocity"]),
        })
    }

    /// The part's volume as Roblox counts it for `Mass`: exact for blocks,
    /// wedges, balls; the cylinder and capsule as their solid of revolution.
    pub(super) fn volume(&self) -> f64 {
        let [x, y, z] = [self.size.x, self.size.y, self.size.z].map(f64::from);
        match self.shape {
            Shape::Block => x * y * z,
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

    pub(super) fn shape_collider(&self) -> ColliderBuilder {
        let (hx, hy, hz) = (self.size.x / 2.0, self.size.y / 2.0, self.size.z / 2.0);
        let along_x = Pose::from_rotation(Rotation::from_rotation_z(std::f32::consts::FRAC_PI_2));
        match self.shape {
            Shape::Block => ColliderBuilder::cuboid(hx, hy, hz),
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

fn hull(points: &[[f32; 3]]) -> Option<ColliderBuilder> {
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
