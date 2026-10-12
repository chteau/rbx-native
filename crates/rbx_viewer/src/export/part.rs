//! One part as the surfaces it exports: its geometry, as the viewport draws
//! it, and the look each surface is shaded with.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{
    packed, surface, transform_normals, transform_points, ExportMesh, Finish, Key, Maps, Textures,
};
use crate::pick::Meshes;
use crate::scene::{
    cframe_matrix, file_mesh_fit, resolve_shape, srgb_to_linear, union_fit, union_is_empty,
    unit_mesh, Kind, FALLBACK_COLOR,
};

/// `scene::FORCE_FIELD_ALPHA`: how solid a ForceField is drawn at most.
const FORCE_FIELD_ALPHA: f32 = 0.5;

/// Triangles in a unit (or a mesh's own) frame, and the matrix that places
/// them.
pub(super) struct Geometry {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) normals: Vec<[f32; 3]>,
    pub(super) indices: Vec<u32>,
    /// The mesh's own; empty for a procedural solid.
    pub(super) uvs: Vec<[f32; 2]>,
    /// The frame the viewport reads a mesh's own normal map in (see
    /// `renderer::mesh_tangents`); empty for a procedural solid.
    pub(super) tangents: Vec<[f32; 4]>,
    pub(super) model: Mat4,
}

/// The part's surfaces, and its `CFrame` (what its node is placed by).
pub(super) fn export_part(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    textures: &mut Textures,
    referent: Ref,
) -> Option<(Vec<ExportMesh>, Mat4)> {
    let instance = dom.get(referent)?;
    let properties = instance.properties();
    let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
        return None;
    };
    let placement = cframe_matrix(cframe);
    let kind = meshes.kind(referent);
    let color = match properties.get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) => [r, g, b],
        _ => FALLBACK_COLOR,
    }
    .map(|channel| srgb_to_linear(f32::from(channel) / 255.0));
    let alpha = alpha(properties.get("Transparency"), kind);

    let file_mesh = file_mesh_fit(dom, database, referent).and_then(|(asset, fit)| {
        let mesh = meshes.drawn(referent, &asset)?;
        Some((mesh, fit.transform(mesh)))
    });
    let union = || {
        let (asset, model) = union_fit(dom, database, referent)?;
        Some((meshes.get(&asset)?, model))
    };
    let looks = Look {
        name: instance.name(),
        referent,
        kind,
    };
    if let Some((mesh, model)) = file_mesh.or_else(union) {
        let geometry = Geometry {
            positions: mesh.vertices.iter().map(|v| v.position).collect(),
            normals: mesh.vertices.iter().map(|v| v.normal).collect(),
            indices: mesh.lod0().to_vec(),
            uvs: mesh.vertices.iter().map(|v| v.uv).collect(),
            tangents: crate::renderer::mesh_tangents(mesh),
            model,
        };
        let surfaces = looks.dress(geometry, color, alpha, meshes, textures);
        return (!surfaces.is_empty()).then_some((surfaces, placement));
    }

    // A union whose boolean could not be run is drawn as its additive
    // pieces, each in its own colour.
    let pieces = meshes.pieces(referent);
    if !pieces.is_empty() {
        let surfaces = pieces
            .iter()
            .flat_map(|piece| {
                let unit = unit_mesh(piece.kind);
                let geometry = Geometry {
                    positions: unit.positions,
                    normals: unit.normals,
                    indices: unit.indices,
                    uvs: Vec::new(),
                    tangents: Vec::new(),
                    model: piece.transform,
                };
                let alpha = alpha.min(piece.alpha);
                looks.dress(geometry, piece.color, alpha, meshes, textures)
            })
            .collect::<Vec<_>>();
        return (!surfaces.is_empty()).then_some((surfaces, placement));
    }

    // Studio draws a union with no geometry anywhere as nothing, and so
    // does the viewport; a box would be invented.
    if union_is_empty(dom, database, referent) {
        return None;
    }
    let Some(Variant::Vector3(size)) = properties.get("size") else {
        return None;
    };
    let size = Vec3::new(size.x, size.y, size.z);
    let shape = resolve_shape(dom, database, instance, size);
    let unit = unit_mesh(shape.kind);
    let geometry = Geometry {
        positions: unit.positions,
        normals: unit.normals,
        indices: unit.indices,
        uvs: Vec::new(),
        tangents: Vec::new(),
        model: shape.model(placement),
    };
    let surfaces = looks.dress(geometry, color, alpha, meshes, textures);
    (!surfaces.is_empty()).then_some((surfaces, placement))
}

/// `1 - Transparency`, capped on a ForceField as `scene::alpha` caps it.
fn alpha(transparency: Option<&Variant>, kind: Kind) -> f32 {
    let alpha = match transparency {
        Some(&Variant::Float32(t)) => 1.0 - t.clamp(0.0, 1.0),
        _ => 1.0,
    };
    match kind {
        Kind::ForceField => alpha.min(FORCE_FIELD_ALPHA),
        _ => alpha,
    }
}

struct Look<'a> {
    name: &'a str,
    referent: Ref,
    kind: Kind,
}

impl Look<'_> {
    /// `geometry` shaded the way `filemesh.wgsl`/`material.wgsl` shade it.
    fn dress(
        &self,
        geometry: Geometry,
        color: [f32; 3],
        alpha: f32,
        meshes: &Meshes,
        textures: &mut Textures,
    ) -> Vec<ExportMesh> {
        if geometry.indices.is_empty() {
            return Vec::new();
        }
        let finish = match self.kind {
            Kind::Neon => Finish::Neon,
            // Only terrain water is drawn as water, and it reaches no part.
            Kind::Glass | Kind::Water => Finish::Glass,
            Kind::ForceField => Finish::ForceField,
            Kind::Plastic | Kind::Textured => Finish::Plain,
        };
        let has_uvs = !geometry.uvs.is_empty();
        let surface = meshes.surface(self.referent).filter(|_| has_uvs);
        let image = meshes.texture(self.referent).filter(|_| has_uvs);
        // A ForceField reads its image only for the moving pattern, never
        // paints it on (`filemesh.wgsl`).
        let (image, pattern) = match finish {
            Finish::ForceField => (None, image),
            _ => (image, None),
        };
        // Neon and ForceField shade procedurally whatever pack they have.
        let pack = meshes
            .material(self.referent)
            .filter(|_| surface.is_none() && !matches!(finish, Finish::Neon | Finish::ForceField));

        let template = ExportMesh {
            name: self.name.to_owned(),
            positions: Vec::new(),
            normals: Vec::new(),
            tangents: Vec::new(),
            indices: Vec::new(),
            color: [color[0], color[1], color[2], alpha],
            uvs: Vec::new(),
            maps: Maps::default(),
            blend: alpha < 1.0,
            finish,
        };
        if let Some(pack) = pack {
            return packed::dress(&template, geometry, pack, image, textures);
        }
        let mut mesh = ExportMesh {
            positions: transform_points(geometry.model, geometry.positions),
            normals: transform_normals(geometry.model, geometry.normals),
            indices: geometry.indices,
            uvs: geometry.uvs,
            ..template
        };
        if let Some(surface) = surface {
            surface::wear(&mut mesh, surface, textures);
            if mesh.maps.normal.is_some() {
                let model = geometry.model;
                mesh.tangents = geometry
                    .tangents
                    .iter()
                    .map(|&[x, y, z, w]| {
                        let along = model.transform_vector3(glam::Vec3::new(x, y, z));
                        along.normalize_or(glam::Vec3::X).extend(w).to_array()
                    })
                    .collect();
            }
        } else if let Some(pattern) = pattern {
            for clear in [false, true] {
                let index = textures.add(Key::Pattern(Arc::as_ptr(pattern), clear), || {
                    surface::png(&surface::force_field_pattern(pattern, clear))
                });
                match clear {
                    false => mesh.maps.pattern = index,
                    true => mesh.maps.see_through = index,
                }
            }
        } else if let Some(image) = image {
            mesh.maps.color = textures.of(image);
            // The viewport multiplies the image's alpha into the part's.
            mesh.blend |= image.has_alpha();
        }
        vec![mesh]
    }
}
