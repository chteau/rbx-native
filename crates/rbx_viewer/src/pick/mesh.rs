//! A ray against the real triangles of a downloaded file mesh, and the handle
//! that lets a hit test read the geometry the render thread already parsed
//! to draw it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};
use rbx_mesh::Mesh;

use crate::assets::Image;
use crate::scene::{AlphaMode, Kind, Scene, Slot};

use super::shape::Local;
use super::Ray;

mod face;
pub(super) use face::flat_face;
pub use face::FlatFace;

/// Every file mesh a loaded place resolved, shared with whoever hit-tests
/// against it.
///
/// Cloning is cheap: the map and every mesh in it sit behind `Arc`s, so the
/// render thread that parsed them and the thread that picks against them read
/// the very same vertices — the geometry that is actually on screen, without
/// a copy of it. Empty by default, which is also the right answer for a place
/// whose meshes have not downloaded: a file-mesh part then picks as the
/// fallback box it is drawn as.
///
/// Also carries the decoded image each textured mesh instance is drawn with,
/// the `SurfaceAppearance` maps of each one wearing a set, and the texture
/// pack of each part whose `Material` has one, by the part it stands for,
/// which nothing picks against but an export writes out beside the triangles
/// (see `crate::export`).
#[derive(Clone, Default)]
pub struct Meshes {
    meshes: Arc<HashMap<AssetRef, Arc<Mesh>>>,
    textures: Arc<HashMap<Ref, Arc<Image>>>,
    surfaces: Arc<HashMap<Ref, Surface>>,
    materials: Arc<HashMap<Ref, Arc<Pack>>>,
    /// Every part drawn as one of the materials no texture captures (Neon,
    /// Glass, ForceField); the rest are plain.
    kinds: Arc<HashMap<Ref, Kind>>,
    /// The additive pieces each union whose boolean could not be run is
    /// drawn as instead (see `scene::union`).
    pieces: Arc<HashMap<Ref, Vec<Piece>>>,
    /// The roots an isolated view draws (see `Headless::set_isolation`), or
    /// `None` while the whole place does. Carried with the meshes because a
    /// click has to reach exactly what is on screen, and this handle is the
    /// one thing the host's hit test already takes from the renderer.
    isolation: Option<Arc<HashSet<Ref>>>,
}

/// One recovered piece of a union, as the scene draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Piece {
    pub(crate) kind: crate::scene::ShapeKind,
    /// Unit shape to world.
    pub(crate) transform: Mat4,
    /// Linear.
    pub(crate) color: [f32; 3],
    pub(crate) alpha: f32,
}

/// A textured `Material`'s maps as the renderer projects them (see
/// `renderer/material.wgsl`): [`rbx_materials::MapKind::ALL`] order, tiled
/// every `studs_per_tile` along the part.
#[derive(Debug, Clone)]
pub(crate) struct Pack {
    pub(crate) maps: [Option<Arc<Image>>; 4],
    pub(crate) studs_per_tile: f32,
}

/// A `SurfaceAppearance` as the renderer binds it: the maps that decoded, in
/// [`rbx_materials::MapKind::ALL`] order, and the tint and alpha mode the
/// shader reads beside them (see `renderer/appearance.wgsl`).
#[derive(Debug, Clone)]
pub(crate) struct Surface {
    pub(crate) maps: [Option<Arc<Image>>; 4],
    pub(crate) tint: [f32; 3],
    pub(crate) alpha_mode: AlphaMode,
}

impl Meshes {
    pub(crate) fn new(
        meshes: HashMap<AssetRef, Arc<Mesh>>,
        textures: HashMap<Ref, Arc<Image>>,
    ) -> Self {
        Meshes {
            meshes: Arc::new(meshes),
            textures: Arc::new(textures),
            surfaces: Arc::default(),
            materials: Arc::default(),
            kinds: Arc::default(),
            pieces: Arc::default(),
            isolation: None,
        }
    }

    pub(crate) fn with_isolation(mut self, roots: Option<HashSet<Ref>>) -> Self {
        self.isolation = roots.map(Arc::new);
        self
    }

    /// Whether `referent` is drawn as far as an isolation goes: always while
    /// nothing is isolated, and otherwise only when it is one of the roots
    /// or sits beneath one in `dom`. Asked of the host's own DOM rather than
    /// of the scene the renderer built, so a part the host has just moved
    /// under a root is reachable on the very next click.
    pub fn shows(&self, dom: &WeakDom, referent: Ref) -> bool {
        self.isolation.as_ref().is_none_or(|roots| {
            crate::scene::isolation_within(roots, referent, |at| dom.parent(at))
        })
    }

    pub(crate) fn with_materials(mut self, materials: HashMap<Ref, Arc<Pack>>) -> Self {
        self.materials = Arc::new(materials);
        self
    }

    pub(crate) fn with_kinds(mut self, kinds: HashMap<Ref, Kind>) -> Self {
        self.kinds = Arc::new(kinds);
        self
    }

    pub(crate) fn with_pieces(mut self, pieces: HashMap<Ref, Vec<Piece>>) -> Self {
        self.pieces = Arc::new(pieces);
        self
    }

    pub(crate) fn with_surfaces(mut self, surfaces: HashMap<Ref, Surface>) -> Self {
        self.surfaces = Arc::new(surfaces);
        self
    }

    /// What the scene resolved, textures as the renderer binds them: the
    /// `TextureID`/`TextureId` that downloaded, and none under a
    /// `SurfaceAppearance`, whose own maps come instead; and every part's
    /// material pack where it shades as a textured one right now.
    pub(crate) fn of(scene: &Scene) -> Self {
        let resolved = scene.resolved_file_meshes();
        let mut textures = HashMap::new();
        let mut surfaces = HashMap::new();
        for instance in &resolved.instances {
            if let Some(appearance) = instance
                .appearance
                .and_then(|index| resolved.appearances.get(index))
            {
                let maps = appearance
                    .maps
                    .clone()
                    .map(|map| map.and_then(|map| resolved.images.get(&map).cloned()));
                surfaces.entry(instance.referent).or_insert(Surface {
                    maps,
                    tint: appearance.tint,
                    alpha_mode: appearance.alpha_mode,
                });
            }
            let image = instance
                .texture
                .as_ref()
                .and_then(|t| resolved.images.get(t));
            if let Some(image) = image {
                textures
                    .entry(instance.referent)
                    .or_insert_with(|| Arc::clone(image));
            }
        }
        // One pack per layer, however many parts are made of it.
        let catalog = scene.materials();
        let mut packs: HashMap<u32, Option<Arc<Pack>>> = HashMap::new();
        let mut materials = HashMap::new();
        let mut kinds = HashMap::new();
        let mut pieces: HashMap<Ref, Vec<Piece>> = HashMap::new();
        for part in scene.parts() {
            if part.id.piece_index().is_some() {
                pieces.entry(part.id.referent()).or_default().push(Piece {
                    kind: part.kind,
                    transform: part.transform,
                    color: part.color,
                    alpha: part.alpha,
                });
            }
        }
        let slots = scene
            .parts()
            .iter()
            .map(|part| (part.id.referent(), part.material))
            .chain(resolved.instances.iter().map(|i| (i.referent, i.material)));
        for (referent, Slot { layer, .. }) in slots {
            // Re-read rather than trusted: the slot a part was built with
            // is plastic until its pack lands (see `Catalog::slot`).
            let slot = catalog.slot(layer);
            if matches!(slot.kind, Kind::Neon | Kind::Glass | Kind::ForceField) {
                kinds.entry(referent).or_insert(slot.kind);
            }
            let pack = packs.entry(layer).or_insert_with(|| {
                // Glass reads its pack too (`mapped_shade`); only Neon and
                // ForceField shade procedurally.
                matches!(slot.kind, Kind::Textured | Kind::Glass).then(|| {
                    Arc::new(Pack {
                        maps: rbx_materials::MapKind::ALL
                            .map(|kind| catalog.shared_image(layer as usize, kind).cloned()),
                        studs_per_tile: slot.studs_per_tile,
                    })
                })
            });
            if let Some(pack) = pack {
                materials
                    .entry(referent)
                    .or_insert_with(|| Arc::clone(pack));
            }
        }
        Meshes::new(resolved.meshes.clone(), textures)
            .with_surfaces(surfaces)
            .with_materials(materials)
            .with_kinds(kinds)
            .with_pieces(pieces)
            .with_isolation(scene.isolation().map(|isolation| isolation.roots().clone()))
    }

    pub fn get(&self, asset: &AssetRef) -> Option<&Arc<Mesh>> {
        self.meshes.get(asset)
    }

    pub(crate) fn texture(&self, referent: Ref) -> Option<&Arc<Image>> {
        self.textures.get(&referent)
    }

    pub(crate) fn surface(&self, referent: Ref) -> Option<&Surface> {
        self.surfaces.get(&referent)
    }

    pub(crate) fn material(&self, referent: Ref) -> Option<&Pack> {
        self.materials.get(&referent).map(Arc::as_ref)
    }

    pub(crate) fn kind(&self, referent: Ref) -> Kind {
        self.kinds.get(&referent).copied().unwrap_or(Kind::Plastic)
    }

    pub(crate) fn pieces(&self, referent: Ref) -> &[Piece] {
        self.pieces.get(&referent).map_or(&[], Vec::as_slice)
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
    let (distance, ..) = nearest(mesh, &local)?;
    Some(distance / local.per_stud)
}

/// Where `ray` first meets `mesh` carried through `model`, and that
/// triangle's unit normal turned to face the ray, both in world space. Facing
/// the ray rather than trusting the winding, because a downloaded mesh's
/// winding is whatever its author exported.
pub(super) fn surface(mesh: &Mesh, model: Mat4, ray: Ray) -> Option<(Vec3, Vec3)> {
    let local = Local::of(model, ray)?;
    let (distance, normal, _) = nearest(mesh, &local)?;
    let normal = model.inverse().transpose().transform_vector3(normal);
    let normal = if normal.dot(ray.direction) > 0.0 {
        -normal
    } else {
        normal
    };
    Some((ray.at(distance / local.per_stud), normal.try_normalize()?))
}

/// The nearest triangle `local` crosses: how far along it, in the mesh's own
/// units, the triangle's (unnormalized) plane normal, and which triangle of
/// [`Mesh::lod0`] it is.
fn nearest(mesh: &Mesh, local: &Local) -> Option<(f32, Vec3, usize)> {
    let vertex = |index: u32| {
        mesh.vertices
            .get(index as usize)
            .map(|vertex| Vec3::from(vertex.position))
    };
    mesh.lod0()
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .filter_map(|(index, triangle)| {
            let (a, b, c) = (
                vertex(triangle[0])?,
                vertex(triangle[1])?,
                vertex(triangle[2])?,
            );
            let distance = triangle_hit(local.origin, local.direction, a, b, c)?;
            Some((distance, (b - a).cross(c - a), index))
        })
        .min_by(|(a, ..), (b, ..)| a.total_cmp(b))
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
