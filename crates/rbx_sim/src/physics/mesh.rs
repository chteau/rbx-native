//! `MeshPart` collision: the mesh's geometry fitted to the part's `Size`,
//! shaped per `CollisionFidelity`.
//!
//! Geometry comes from meshes scripts made at runtime (`register`) or from
//! the part's `MeshId`, read from the local asset cache only: building the
//! world never waits on the network. A part whose geometry isn't available
//! collides as its `Size` box.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rapier3d::parry::transformation::vhacd::VHACDParameters;
use rapier3d::prelude::*;
use rbx_assets::{AssetCache, AssetRef};
use rbx_dom::Ref;

use super::part::PartData;

/// Triangles of one mesh, vertices in the mesh's own space.
pub(crate) struct Geometry {
    id: u64,
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Fidelity {
    Default,
    Hull,
    Box,
    Precise,
}

impl Fidelity {
    pub(crate) fn from_name(name: &str) -> Fidelity {
        match name {
            "Hull" => Fidelity::Hull,
            "Box" => Fidelity::Box,
            "PreciseConvexDecomposition" => Fidelity::Precise,
            _ => Fidelity::Default,
        }
    }
}

type ShapeKey = (u64, [u32; 3], Fidelity);

thread_local! {
    static REGISTERED: RefCell<HashMap<Ref, Arc<Geometry>>> = RefCell::default();
    static ASSETS: RefCell<HashMap<String, Option<Arc<Geometry>>>> = RefCell::default();
    static SHAPES: RefCell<HashMap<ShapeKey, SharedShape>> = RefCell::default();
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

/// Remembers the geometry of a mesh a script made for `part`.
pub(crate) fn register(part: Ref, vertices: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) {
    let id = NEXT_ID.with(|next| next.replace(next.get() + 1));
    let geometry = Arc::new(Geometry {
        id,
        vertices,
        triangles,
    });
    REGISTERED.with(|all| all.borrow_mut().insert(part, geometry));
}

fn asset_geometry(mesh_id: &str) -> Option<Arc<Geometry>> {
    if let Some(known) = ASSETS.with(|all| all.borrow().get(mesh_id).cloned()) {
        return known;
    }
    let loaded = load_asset(mesh_id);
    ASSETS.with(|all| all.borrow_mut().insert(mesh_id.to_owned(), loaded.clone()));
    loaded
}

fn load_asset(mesh_id: &str) -> Option<Arc<Geometry>> {
    let cache = AssetCache::new(None).ok()?;
    let bytes = match AssetRef::parse(mesh_id).ok()? {
        AssetRef::Id(id) => cache.get_id(id)?,
        AssetRef::Native(path) => cache.get_native(&path)?,
        AssetRef::Thumb(_) | AssetRef::Empty => return None,
    };
    let mesh = rbx_mesh::parse(&bytes).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    mesh_id.hash(&mut hasher);
    Some(Arc::new(Geometry {
        id: hasher.finish() | (1 << 63),
        vertices: mesh.vertices.iter().map(|v| v.position).collect(),
        triangles: mesh.indices.as_chunks::<3>().0.to_vec(),
    }))
}

fn geometry_of(part: &PartData) -> Option<Arc<Geometry>> {
    REGISTERED
        .with(|all| all.borrow().get(&part.referent).cloned())
        .or_else(|| asset_geometry(&part.mesh_id))
}

fn bounds(geometry: &Geometry) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for v in &geometry.vertices {
        for axis in 0..3 {
            min[axis] = min[axis].min(v[axis]);
            max[axis] = max[axis].max(v[axis]);
        }
    }
    (min, max)
}

fn scale_of(geometry: &Geometry, size: [f32; 3]) -> [f32; 3] {
    let (min, max) = bounds(geometry);
    std::array::from_fn(|i| {
        let extent = max[i] - min[i];
        if extent > 0.0 {
            size[i] / extent
        } else {
            1.0
        }
    })
}

/// The mesh's vertices scaled from their own bounding box to `size`, about
/// the mesh's own origin, which stays the part's origin.
fn fitted(geometry: &Geometry, size: [f32; 3]) -> Vec<Vector> {
    let scale = scale_of(geometry, size);
    geometry
        .vertices
        .iter()
        .map(|v| {
            Vector::new(
                (v[0] * scale[0]).into(),
                (v[1] * scale[1]).into(),
                (v[2] * scale[2]).into(),
            )
        })
        .collect()
}

/// `Size` as a box around where the mesh's bounding box sits.
fn bounding_box(geometry: &Geometry, size: [f32; 3]) -> SharedShape {
    let (min, max) = bounds(geometry);
    let scale = scale_of(geometry, size);
    let centre = Vector::new(
        ((min[0] + max[0]) / 2.0 * scale[0]).into(),
        ((min[1] + max[1]) / 2.0 * scale[1]).into(),
        ((min[2] + max[2]) / 2.0 * scale[2]).into(),
    );
    SharedShape::compound(vec![(Pose::from_translation(centre), cuboid(size))])
}

fn cuboid(size: [f32; 3]) -> SharedShape {
    let half = |v: f32| Real::from(v) / 2.0;
    SharedShape::cuboid(half(size[0]), half(size[1]), half(size[2]))
}

fn build(geometry: &Geometry, size: [f32; 3], fidelity: Fidelity) -> SharedShape {
    if geometry.vertices.is_empty() {
        return cuboid(size);
    }
    if fidelity == Fidelity::Box {
        return bounding_box(geometry, size);
    }
    let points = fitted(geometry, size);
    let shape = match fidelity {
        Fidelity::Box => None,
        Fidelity::Hull => SharedShape::convex_hull(&points),
        Fidelity::Default | Fidelity::Precise => {
            if geometry.triangles.is_empty() {
                None
            } else {
                let params = if fidelity == Fidelity::Default {
                    VHACDParameters {
                        resolution: 32,
                        max_convex_hulls: 16,
                        ..Default::default()
                    }
                } else {
                    VHACDParameters {
                        resolution: 64,
                        concavity: 0.0025,
                        max_convex_hulls: 64,
                        ..Default::default()
                    }
                };
                // parry's own `convex_decomposition_with_params`, minus its
                // panic when no piece survives (a degenerate mesh).
                let decomposed = rapier3d::parry::transformation::vhacd::VHACD::decompose(
                    &params,
                    &points,
                    &geometry.triangles,
                    true,
                );
                let pieces: Vec<_> = decomposed
                    .compute_exact_convex_hulls(&points, &geometry.triangles)
                    .into_iter()
                    .filter_map(|(vertices, indices)| SharedShape::convex_mesh(vertices, &indices))
                    .map(|piece| (Pose::IDENTITY, piece))
                    .collect();
                (!pieces.is_empty()).then(|| SharedShape::compound(pieces))
            }
        }
    };
    shape.unwrap_or_else(|| cuboid(size))
}

fn shape(geometry: &Geometry, size: [f32; 3], fidelity: Fidelity) -> SharedShape {
    let key = (geometry.id, size.map(f32::to_bits), fidelity);
    if let Some(found) = SHAPES.with(|all| all.borrow().get(&key).cloned()) {
        return found;
    }
    let built = build(geometry, size, fidelity);
    SHAPES.with(|all| all.borrow_mut().insert(key, built.clone()));
    built
}

/// The collider of a `MeshPart`; physics and spatial queries both build
/// through this, so a ray sees what the solver collides with.
pub(crate) fn collider(part: &PartData) -> ColliderBuilder {
    let size = [part.size.x, part.size.y, part.size.z];
    let shape = match geometry_of(part) {
        Some(geometry) => shape(&geometry, size, Fidelity::from_name(&part.fidelity)),
        None => cuboid(size),
    };
    ColliderBuilder::new(shape)
}

#[cfg(test)]
mod tests {
    use rapier3d::parry::query::Ray;

    use super::*;

    fn cube(min: [f32; 3], max: [f32; 3], vertices: &mut Vec<[f32; 3]>, tris: &mut Vec<[u32; 3]>) {
        let base = vertices.len() as u32;
        for i in 0..8 {
            vertices.push([
                if i & 4 != 0 { max[0] } else { min[0] },
                if i & 2 != 0 { max[1] } else { min[1] },
                if i & 1 != 0 { max[2] } else { min[2] },
            ]);
        }
        for q in [
            [0, 1, 3, 2],
            [4, 6, 7, 5],
            [0, 4, 5, 1],
            [2, 3, 7, 6],
            [0, 2, 6, 4],
            [1, 5, 7, 3],
        ] {
            tris.push([base + q[0], base + q[1], base + q[2]]);
            tris.push([base + q[0], base + q[2], base + q[3]]);
        }
    }

    fn two_cubes() -> Geometry {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        cube(
            [-3.0, -1.0, -1.0],
            [-1.0, 1.0, 1.0],
            &mut vertices,
            &mut triangles,
        );
        cube(
            [1.0, 1.0, -1.0],
            [3.0, 3.0, 1.0],
            &mut vertices,
            &mut triangles,
        );
        Geometry {
            id: u64::MAX,
            vertices,
            triangles,
        }
    }

    fn hits(shape: &SharedShape, x: Real) -> bool {
        let ray = Ray::new(Vector::new(x, 10.0, 0.0), Vector::new(0.0, -20.0, 0.0));
        shape
            .cast_ray_and_get_normal(&Pose::IDENTITY, &ray, 1.0, true)
            .is_some()
    }

    #[test]
    fn hull_fills_the_gap() {
        let shape = build(&two_cubes(), [6.0, 4.0, 2.0], Fidelity::Hull);
        assert!(hits(&shape, 0.0));
        assert!(hits(&shape, 2.0));
    }

    #[test]
    fn box_fills_the_gap() {
        let shape = build(&two_cubes(), [6.0, 4.0, 2.0], Fidelity::Box);
        assert!(hits(&shape, 0.0));
    }

    #[test]
    fn decomposition_keeps_the_gap() {
        for fidelity in [Fidelity::Default, Fidelity::Precise] {
            let shape = build(&two_cubes(), [6.0, 4.0, 2.0], fidelity);
            assert!(!hits(&shape, 0.0), "{fidelity:?} filled the gap");
            assert!(hits(&shape, 2.0), "{fidelity:?} lost a cube");
        }
    }

    #[test]
    fn a_flat_mesh_that_decomposes_to_nothing_falls_back_to_its_box() {
        let geometry = Geometry {
            id: 0,
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            triangles: vec![[0, 1, 2]],
        };
        let shape = build(&geometry, [2.0, 1.0, 1.0], Fidelity::Default);
        assert!(shape.as_cuboid().is_some() || shape.as_compound().is_some());
    }
}
