//! Instances as plain triangle meshes, written out as Wavefront `.obj`,
//! glTF 2.0 or binary FBX for another 3D tool to open.
//!
//! Every part is exported as the geometry the viewport draws it with, the
//! same `scene::shape::resolve` precedence and unit meshes for the procedural
//! solids, the same fitted `MeshId` triangles for a `MeshPart` or a
//! `SpecialMesh` FileMesh (see [`crate::pick`], which resolves a click the same
//! way), baked into world space. A `UnionOperation` exports the boolean
//! `scene::union` carved from its original parts, or Roblox's own baked
//! `MeshData` where no parts survive; where that boolean could not be run,
//! the additive pieces the viewport draws instead. A file mesh that has not downloaded exports as
//! the box it is drawn as.
//!
//! Each part is shaded as the viewport shades it (see [`part`]): its image or
//! `SurfaceAppearance`, its `Material`'s pack, tiled where one projection
//! covers a face and baked (see [`bake`]) where the shader blends three or
//! multiplies the pack under a mesh's image, and the materials no texture
//! captures (Neon, Glass, ForceField) as a [`Finish`] the writers turn into
//! what each format has closest. Images are pooled in [`Export::textures`]:
//! one the whole place shares is encoded and written once.
//!
//! The instance tree comes along as [`Export::nodes`], one per part and one
//! per container between it and the exported root, the way Studio's own
//! glTF export keeps the hierarchy. Units are studs, unscaled, in Roblox's
//! own right-handed Y-up frame, which is also glTF's and what Studio's
//! exporter writes.

mod bake;
mod fbx;
mod gltf;
mod obj;
mod packed;
mod part;
mod surface;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::assets::Image;
use crate::pick::Meshes;
use crate::scene::{descendants_of, is_drawable};

pub use fbx::fbx;
pub use gltf::{gltf, gltf_files};
pub use obj::{mtl, obj, obj_files};

/// Everything an export writes: each part's surfaces, the PNGs their
/// materials point into by index (each image once however many parts wear
/// it), and the instance tree they hang from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Export {
    pub meshes: Vec<ExportMesh>,
    pub textures: Vec<Vec<u8>>,
    pub nodes: Vec<ExportNode>,
}

/// One instance in the exported tree: a part, or a container (a `Model`, a
/// `Folder`, the exported root) between parts and the root.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportNode {
    pub name: String,
    pub class: String,
    pub parent: Option<usize>,
    /// A part's `CFrame` as rotation then position, world space; `None` for
    /// a container, which places nothing.
    pub placement: Option<([f32; 4], [f32; 3])>,
    /// The [`Export::meshes`] this part is drawn as, one per material it
    /// needs: usually one, two where a pack is tiled on some faces and baked
    /// on the rest, one per piece of a union drawn as its pieces.
    pub meshes: Vec<usize>,
    /// The part's `Material`, by name, as Studio's own export records it.
    pub material: Option<String>,
}

/// How a surface is lit, beyond its maps: the procedural materials the
/// viewport shades with no texture of their own (`renderer/material.wgsl`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Finish {
    #[default]
    Plain,
    /// Unlit and over-bright: its colour (times its image) is its light.
    Neon,
    /// See-through by `Transparency`, refracting what is behind it.
    Glass,
    /// A tinted shell, faint face-on and glowing at its rim.
    ForceField,
}

/// One surface of a part in world space, ready to write.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportMesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Where the normal map is baked in a frame of its own (see [`bake`]):
    /// the direction of increasing U, handedness in W as glTF has it. Empty
    /// otherwise, for a viewer to derive from the UVs.
    pub tangents: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Linear RGB, and `1 - Transparency` as alpha. Multiplies the colour
    /// map, as the viewport does.
    pub color: [f32; 4],
    /// One per position, top-left origin as Roblox and glTF have it; empty
    /// for a procedural solid, which has no image to map.
    pub uvs: Vec<[f32; 2]>,
    pub maps: Maps,
    /// Whether the surface is see-through anywhere: its `Transparency`, or
    /// a colour map whose alpha the viewport blends with.
    pub blend: bool,
    pub finish: Finish,
}

/// The images a part is drawn with, each an index into [`Export::textures`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Maps {
    pub color: Option<usize>,
    /// Tangent space, green up the image, as glTF and Blender read one.
    pub normal: Option<usize>,
    /// Greyscale in red, as `SurfaceAppearance` authors them; for OBJ's
    /// `map_Pm`/`map_Pr`.
    pub metalness: Option<usize>,
    pub roughness: Option<usize>,
    /// The two packed the way glTF's `metallicRoughnessTexture` wants them:
    /// roughness in green, metalness in blue.
    pub metallic_roughness: Option<usize>,
    /// A ForceField mesh's pattern as `filemesh.wgsl` shows it at the
    /// start of its cycle: greyscale, white where it shows.
    pub pattern: Option<usize>,
    /// The same pattern inverted, in red: how much of the shell is clear
    /// there, as glTF's transmission texture reads it.
    pub see_through: Option<usize>,
}

/// Every drawable part in `roots`' subtrees, each once even where one root
/// sits under another, depth first. Unlike [`crate::pick::parts_of`], a
/// part's child parts come along: an export is of the subtree, not of what
/// one click would move.
pub fn meshes_of(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    roots: &[Ref],
) -> Export {
    within(dom, database, meshes, roots, TEXEL_BUDGET)
}

/// Every baked texel an export may write, across all its pages. A viewer
/// uploads each of a page's four maps (colour, normal, metalness-roughness,
/// and the `.mtl`'s separate pair aside) as RGBA8 with a third more for its
/// mips: 128 Mtexels is about 2.7 GiB of GPU memory at most, what a 4 GB
/// card can still hold beside the rest of a scene, and 32 full 2048 pages,
/// a few hundred megabytes of PNG. Below it every bake keeps the pack's own
/// density; above it every bake is coarsened by the same factor, so no part
/// is singled out.
const TEXEL_BUDGET: u64 = 128 << 20;

/// [`meshes_of`] with every bake together kept to `budget` texels: a first
/// pass finds every bake the export needs and what each costs at its own
/// density, and the second bakes them all at the one density factor that
/// fits.
fn within(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    roots: &[Ref],
    budget: u64,
) -> Export {
    let mut textures = Textures {
        measuring: true,
        ..Textures::default()
    };
    collect(dom, database, meshes, roots, &mut textures);
    let planned = std::mem::take(&mut textures.planned);
    let total = |scale: f32| planned.values().map(|bake| bake.texels(scale)).sum::<u64>();
    textures.scale = 1.0;
    if total(1.0) > budget {
        // Page squares round up to powers of two, so the area does not fall
        // exactly with the square of the scale; step down until it fits.
        textures.scale = (budget as f64 / total(1.0) as f64).sqrt() as f32;
        while total(textures.scale) > budget && textures.scale > 1e-3 {
            textures.scale *= 0.9;
        }
    }
    textures.measuring = false;
    collect(dom, database, meshes, roots, &mut textures)
}

fn collect(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    roots: &[Ref],
    textures: &mut Textures,
) -> Export {
    let mut seen = HashSet::new();
    let mut tree = Tree::default();
    let mut export = Export::default();
    for &root in roots {
        for referent in descendants_of(dom, root) {
            if !seen.insert(referent) || !is_drawable(dom, database, referent) {
                continue;
            }
            let Some((surfaces, placement)) =
                part::export_part(dom, database, meshes, textures, referent)
            else {
                continue;
            };
            let node = tree.node(dom, database, &mut export.nodes, referent, root);
            let (_, rotation, translation) = placement.to_scale_rotation_translation();
            export.nodes[node].placement = Some((rotation.to_array(), translation.to_array()));
            let first = export.meshes.len();
            export.meshes.extend(surfaces);
            export.nodes[node].meshes = (first..export.meshes.len()).collect();
        }
    }
    export.textures = textures.pngs.clone();
    export
}

/// Which node each instance became, so a model's parts share its node.
#[derive(Default)]
struct Tree {
    nodes: HashMap<Ref, usize>,
}

impl Tree {
    fn node(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        nodes: &mut Vec<ExportNode>,
        referent: Ref,
        root: Ref,
    ) -> usize {
        if let Some(&index) = self.nodes.get(&referent) {
            return index;
        }
        let parent = match dom.parent(referent) {
            Some(parent) if referent != root => Some(self.node(dom, database, nodes, parent, root)),
            _ => None,
        };
        let instance = dom.get(referent);
        let material = instance
            .and_then(|i| match i.properties().get("Material") {
                Some(&rbx_dom::Variant::Enum(value)) => database.enum_name("Material", value),
                _ => None,
            })
            .map(str::to_owned);
        nodes.push(ExportNode {
            name: instance.map_or_else(String::new, |i| i.name().to_owned()),
            class: instance.map_or_else(String::new, |i| i.class().to_owned()),
            parent,
            placement: None,
            meshes: Vec::new(),
            material,
        });
        self.nodes.insert(referent, nodes.len() - 1);
        nodes.len() - 1
    }
}

/// The PNGs encoded so far, by what they were made from: every part wearing
/// one asset holds the very same `Arc` (see [`crate::pick::Meshes`]), so its
/// address is the asset's identity here.
#[derive(Default)]
struct Textures {
    pngs: Vec<Vec<u8>>,
    by_key: HashMap<Key, Option<usize>>,
    /// Each bake once, by a hash of everything it depends on (see
    /// [`packed::bake_key`]).
    bakes: HashMap<u64, Arc<Vec<packed::Bake>>>,
    /// The first of [`within`]'s two passes: bakes are listed in
    /// `planned`, not made.
    measuring: bool,
    planned: HashMap<u64, packed::Planned>,
    /// The density factor every bake is made at (1 below the budget).
    scale: f32,
}

#[derive(PartialEq, Eq, Hash)]
enum Key {
    Image(*const Image),
    /// A colour map baked, tinted, over a part colour (see
    /// `surface::overlay`), by the bits of the colour and the tint.
    Overlay(*const Image, [u32; 6]),
    /// Metalness and roughness packed (see `surface::pack`), null for a map
    /// the set does not carry.
    Packed(*const Image, *const Image),
    /// A ForceField's pattern (see `surface::force_field_pattern`),
    /// inverted or not.
    Pattern(*const Image, bool),
}

impl Textures {
    fn add(&mut self, key: Key, make: impl FnOnce() -> Option<Vec<u8>>) -> Option<usize> {
        *self.by_key.entry(key).or_insert_with(|| {
            self.pngs.push(make()?);
            Some(self.pngs.len() - 1)
        })
    }

    fn of(&mut self, image: &Arc<Image>) -> Option<usize> {
        self.add(Key::Image(Arc::as_ptr(image)), || surface::png(image))
    }

    /// An image made here rather than loaded, encoded every time it is
    /// asked for: callers share it through [`Textures::bakes`] instead.
    fn fresh(&mut self, image: &Image) -> Option<usize> {
        self.pngs.push(surface::png(image)?);
        Some(self.pngs.len() - 1)
    }
}

/// A part's matrix split into the rigid move its `CFrame` is and the
/// per-axis extent its unit mesh is stretched to. A part's matrix is always
/// a rotation times a scale, never a shear.
fn rigid(model: Mat4) -> (Mat4, Vec3) {
    let extent = Vec3::new(
        model.x_axis.truncate().length(),
        model.y_axis.truncate().length(),
        model.z_axis.truncate().length(),
    );
    let (_, rotation, translation) = model.to_scale_rotation_translation();
    (
        Mat4::from_rotation_translation(rotation, translation),
        extent,
    )
}

fn transform_points(model: Mat4, points: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    points
        .into_iter()
        .map(|p| model.transform_point3(Vec3::from(p)).to_array())
        .collect()
}

/// Through the inverse transpose, so a stretched part's normals stay
/// perpendicular to its faces.
fn transform_normals(model: Mat4, normals: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    let normal_matrix = model.inverse().transpose();
    normals
        .into_iter()
        .map(|n| {
            normal_matrix
                .transform_vector3(Vec3::from(n))
                .normalize_or(Vec3::Y)
                .to_array()
        })
        .collect()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod finish_tests;
