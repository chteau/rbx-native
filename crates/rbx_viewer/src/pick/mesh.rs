//! A ray against the real triangles of a downloaded file mesh, and the handle
//! that lets a hit test read the geometry the render thread already parsed
//! to draw it.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_mesh::Mesh;

use super::shape::Local;
use super::Ray;

/// Every file mesh a loaded place resolved, shared with whoever hit-tests
/// against it.
///
/// Cloning is cheap: the map and every mesh in it sit behind `Arc`s, so the
/// render thread that parsed them and the thread that picks against them read
/// the very same vertices — the geometry that is actually on screen, without
/// a copy of it. Empty by default, which is also the right answer for a place
/// whose meshes have not downloaded: a file-mesh part then picks as the
/// fallback box it is drawn as.
#[derive(Clone, Default)]
pub struct Meshes(Arc<HashMap<AssetRef, Arc<Mesh>>>);

impl Meshes {
    pub(crate) fn new(meshes: HashMap<AssetRef, Arc<Mesh>>) -> Self {
        Meshes(Arc::new(meshes))
    }

    pub(super) fn get(&self, asset: &AssetRef) -> Option<&Arc<Mesh>> {
        self.0.get(asset)
    }
}

/// How far along `ray` it first meets any triangle of `mesh` carried through
/// `model`, or `None` when it threads between them all or the mesh lies
/// behind the ray's origin.
///
/// Back faces count: a ray starting inside a closed mesh meets its far side
/// from within, which keeps a part the camera sits in selectable. There is no
/// "inside" answer of 0 as the solids have, since an arbitrary mesh need not
/// even be closed.
pub(super) fn hit(mesh: &Mesh, model: Mat4, ray: Ray) -> Option<f32> {
    let local = Local::of(model, ray)?;
    let (distance, _) = nearest(mesh, &local)?;
    Some(distance / local.per_stud)
}

/// Where `ray` first meets `mesh` carried through `model`, and that
/// triangle's unit normal turned to face the ray, both in world space. Facing
/// the ray rather than trusting the winding, because a downloaded mesh's
/// winding is whatever its author exported.
pub(super) fn surface(mesh: &Mesh, model: Mat4, ray: Ray) -> Option<(Vec3, Vec3)> {
    let local = Local::of(model, ray)?;
    let (distance, normal) = nearest(mesh, &local)?;
    let normal = model.inverse().transpose().transform_vector3(normal);
    let normal = if normal.dot(ray.direction) > 0.0 {
        -normal
    } else {
        normal
    };
    Some((ray.at(distance / local.per_stud), normal.try_normalize()?))
}

/// The nearest triangle `local` crosses: how far along it, in the mesh's own
/// units, and the triangle's (unnormalized) plane normal.
fn nearest(mesh: &Mesh, local: &Local) -> Option<(f32, Vec3)> {
    let vertex = |index: u32| {
        mesh.vertices
            .get(index as usize)
            .map(|vertex| Vec3::from(vertex.position))
    };
    mesh.lod0()
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|triangle| {
            let (a, b, c) = (
                vertex(triangle[0])?,
                vertex(triangle[1])?,
                vertex(triangle[2])?,
            );
            let distance = triangle_hit(local.origin, local.direction, a, b, c)?;
            Some((distance, (b - a).cross(c - a)))
        })
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
}

/// Möller–Trumbore: where the ray crosses the plane of triangle `abc`,
/// expressed in the triangle's own barycentric coordinates so "inside" is
/// two comparisons rather than a separate point-in-triangle test.
fn triangle_hit(origin: Vec3, direction: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let edge_ab = b - a;
    let edge_ac = c - a;
    let normal_ish = direction.cross(edge_ac);
    let determinant = edge_ab.dot(normal_ish);
    // Relative to the triangle's own size, so a small but real triangle isn't
    // mistaken for one seen edge-on.
    if determinant.abs() <= f32::EPSILON * edge_ab.length() * edge_ac.length() {
        return None;
    }
    let inverse = 1.0 / determinant;
    let from_a = origin - a;
    let u = from_a.dot(normal_ish) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let across = from_a.cross(edge_ab);
    let v = direction.dot(across) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = edge_ac.dot(across) * inverse;
    (distance >= 0.0).then_some(distance)
}

#[cfg(test)]
#[path = "mesh/tests.rs"]
mod tests;
