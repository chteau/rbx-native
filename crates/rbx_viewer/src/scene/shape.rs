//! Works out which unit mesh a `BasePart` draws as, and the size/offset to
//! scale and place it with.
//!
//! Precedence, matching the engine: a legacy mesh child (`BlockMesh`,
//! `CylinderMesh`, `SpecialMesh`) replaces the part's own geometry outright;
//! otherwise `WedgePart`/`CornerWedgePart`'s class decides it; otherwise a
//! `Part`'s own `shape` property (note the lowercase wire name — `Shape` with a
//! capital belongs to `ParticleEmitter`, a different property entirely);
//! otherwise it is a plain box.

use glam::{Mat4, Vec3};
use rbx_dom::{Instance, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::shapes::{TrussAxis, TrussStyle};

const WEDGE_PART: &str = "WedgePart";
const CORNER_WEDGE_PART: &str = "CornerWedgePart";
const TRUSS_PART: &str = "TrussPart";
// Roblox restricts a real TrussPart.Size to 2*2*n studs, n a multiple of 2, up
// to 512 — n/2 (this cap) is the largest legitimate number of 2-stud repeats.
const MAX_TRUSS_SEGMENTS: u32 = 256;

/// Which unit mesh (from [`crate::shapes`]) a part instances, before its
/// transform is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeKind {
    Box,
    Ball,
    CylinderX,
    CylinderY,
    Wedge,
    CornerWedge,
    Truss {
        axis: TrussAxis,
        segments: u32,
        style: TrussStyle,
    },
}

/// A part's resolved shape: which mesh, and the size/offset to place it with.
///
/// `size` already folds in a mesh child's `Scale` (or is clamped to a uniform
/// diameter for `Part.shape == Ball`, which — unlike `SpecialMesh.MeshType ==
/// Sphere` — never stretches into an ellipsoid); `offset` is zero unless a mesh
/// child carries one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Geometry {
    pub(crate) kind: ShapeKind,
    pub(crate) size: Vec3,
    pub(crate) offset: Vec3,
}

impl Geometry {
    /// The matrix the unit mesh is drawn through: the part's CFrame, then the
    /// shape's offset in the part's own frame, then its size as a scale.
    pub(crate) fn model(&self, cframe: Mat4) -> Mat4 {
        cframe * Mat4::from_translation(self.offset) * Mat4::from_scale(self.size)
    }
}

/// Resolves one part's geometry from its mesh children, class, and `shape`
/// property, in that order.
pub(crate) fn resolve(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    instance: &Instance,
    size: Vec3,
) -> Geometry {
    if let Some(geometry) = mesh_child(dom, instance, size) {
        return geometry;
    }

    let class = instance.class();
    if database.is_subclass_of(class, WEDGE_PART) {
        return Geometry {
            kind: ShapeKind::Wedge,
            size,
            offset: Vec3::ZERO,
        };
    }
    if database.is_subclass_of(class, CORNER_WEDGE_PART) {
        return Geometry {
            kind: ShapeKind::CornerWedge,
            size,
            offset: Vec3::ZERO,
        };
    }
    if database.is_subclass_of(class, TRUSS_PART) {
        return Geometry {
            kind: truss_kind(instance, size),
            size,
            offset: Vec3::ZERO,
        };
    }

    let kind = match instance.properties().get("shape") {
        Some(&Variant::Enum(raw)) => part_type(raw),
        // UnionOperation, MeshPart and anything else with no `shape` property:
        // a plain box until CSG/mesh geometry lands (see rbx_mesh).
        _ => ShapeKind::Box,
    };
    let size = if kind == ShapeKind::Ball {
        // Enum.PartType.Ball is always a true sphere, clamped to the smallest
        // dimension rather than stretched into an ellipsoid.
        Vec3::splat(size.x.min(size.y).min(size.z))
    } else {
        size
    };
    Geometry {
        kind,
        size,
        offset: Vec3::ZERO,
    }
}

/// `Enum.PartType`: Ball=0, Block=1, Cylinder=2, Wedge=3, CornerWedge=4.
fn part_type(raw: u32) -> ShapeKind {
    match raw {
        0 => ShapeKind::Ball,
        2 => ShapeKind::CylinderX,
        3 => ShapeKind::Wedge,
        4 => ShapeKind::CornerWedge,
        _ => ShapeKind::Box,
    }
}

/// A `TrussPart`'s shape: its long axis, how many 2-stud lattice repeats fit
/// along it, and its cross-bracing pattern.
fn truss_kind(instance: &Instance, size: Vec3) -> ShapeKind {
    let axis = truss_axis(size);
    let length = match axis {
        TrussAxis::X => size.x,
        TrussAxis::Y => size.y,
        TrussAxis::Z => size.z,
    };
    let style = match instance.properties().get("Style") {
        Some(&Variant::Enum(raw)) => truss_style(raw),
        _ => TrussStyle::default(),
    };
    ShapeKind::Truss {
        axis,
        segments: truss_segments(length),
        style,
    }
}

/// The part's long axis: whichever `size` component is largest. Roblox always
/// forces the other two to 2 studs for a real `TrussPart`, so this is
/// unambiguous; a tie (only possible from a non-conforming scripted size)
/// resolves to X.
fn truss_axis(size: Vec3) -> TrussAxis {
    if size.y > size.x && size.y >= size.z {
        TrussAxis::Y
    } else if size.z > size.x && size.z > size.y {
        TrussAxis::Z
    } else {
        TrussAxis::X
    }
}

/// One lattice repeat per real 2-stud length, matching how Roblox documents
/// `TrussPart.Size` (`2*2*n`, `n` a multiple of 2): `n / 2` is always a whole
/// number for a real truss. Clamped in case a script sets a non-conforming
/// size.
fn truss_segments(long_axis_length: f32) -> u32 {
    if !long_axis_length.is_finite() {
        return 1;
    }
    ((long_axis_length / 2.0).round() as i64).clamp(1, MAX_TRUSS_SEGMENTS as i64) as u32
}

/// `Enum.Style`: AlternatingSupports=0, BridgeStyleSupports=1, NoSupports=2.
fn truss_style(raw: u32) -> TrussStyle {
    match raw {
        1 => TrussStyle::BridgeStyleSupports,
        2 => TrussStyle::NoSupports,
        _ => TrussStyle::AlternatingSupports,
    }
}

/// Looks for the first legacy mesh child that replaces the part's own shape.
fn mesh_child(dom: &WeakDom, instance: &Instance, size: Vec3) -> Option<Geometry> {
    instance.children().iter().find_map(|&child_ref| {
        let child = dom.get(child_ref)?;
        let (scale, offset) = mesh_scale_offset(child);

        match child.class() {
            "BlockMesh" => Some(Geometry {
                kind: ShapeKind::Box,
                size: size * scale,
                offset,
            }),
            "CylinderMesh" => Some(Geometry {
                kind: ShapeKind::CylinderY,
                size: size * scale,
                offset,
            }),
            "SpecialMesh" => special_mesh(child, size * scale, offset),
            // A plain `FileMesh` child (no MeshType) has no procedural stand-in
            // yet — TODO: rbx_mesh will draw its actual geometry.
            _ => None,
        }
    })
}

/// `Enum.MeshType`, mapped onto the meshes this viewer can actually draw.
///
/// TODO: Head and Torso are rough stand-ins (ellipsoid / box) rather than their
/// true rounded shapes; Prism/ParallelRamp/RightAngleRamp reuse Wedge as a
/// reasonable approximation; Pyramid has no dedicated mesh yet and falls back
/// to a box. `VertexColor` is read nowhere yet — every shaped part keeps the
/// part's own `Color3uint8`.
fn special_mesh(mesh: &Instance, size: Vec3, offset: Vec3) -> Option<Geometry> {
    let Some(&Variant::Enum(mesh_type)) = mesh.properties().get("MeshType") else {
        return Some(Geometry {
            kind: ShapeKind::Box,
            size,
            offset,
        });
    };

    let kind = match mesh_type {
        0 | 3 => ShapeKind::Ball,           // Head, Sphere
        4 => ShapeKind::CylinderY,          // Cylinder
        2 | 7 | 9 | 10 => ShapeKind::Wedge, // Wedge, Prism, ParallelRamp, RightAngleRamp
        11 => ShapeKind::CornerWedge,
        5 => return None,    // FileMesh: no procedural geometry yet
        _ => ShapeKind::Box, // Torso, Brick, Pyramid, unknown
    };
    Some(Geometry { kind, size, offset })
}

fn mesh_scale_offset(mesh: &Instance) -> (Vec3, Vec3) {
    let vector3 = |name: &str, default: Vec3| match mesh.properties().get(name) {
        Some(&Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
        _ => default,
    };
    (vector3("Scale", Vec3::ONE), vector3("Offset", Vec3::ZERO))
}

#[cfg(test)]
#[path = "shape/tests.rs"]
mod tests;
