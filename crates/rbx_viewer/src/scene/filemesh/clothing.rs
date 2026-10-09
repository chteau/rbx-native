//! Classic clothing on an R15 character.
//!
//! R15 limbs are meshes whose UVs do not follow Roblox's clothing template, so
//! the template cannot be sampled through them. Instead the limb is dressed on
//! the CPU: the sections of one limb (an arm is an upper arm, a lower arm and
//! a hand) are treated as the single box the template was cut for, each
//! triangle takes the face of that box it lies on, and gets UVs into that
//! face's rectangle of the template. The garments are then composited over the
//! body colour into one image per limb and colour, which the part draws in
//! place of its plain colour.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat3, Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};

use super::Entry;
use crate::assets::Image;
use crate::scene;
use crate::textures::clothing::{self, Garment, Limb, Wardrobe, TEMPLATE};
use crate::textures::NormalId;

/// What an R15 section is dressed in, read off its model.
#[derive(Clone)]
pub(super) struct Dressing {
    limb: Limb,
    /// The character model: the sections of one limb are those of it that
    /// share a [`Limb`].
    model: Ref,
    wardrobe: Wardrobe,
}

/// The dressing of `referent`, when it is an R15 section of a character that
/// wears something that fits it.
pub(super) fn dressing(dom: &WeakDom, referent: Ref) -> Option<Dressing> {
    let limb = Limb::r15(dom.get(referent)?.name())?;
    let model = dom.parent(referent)?;
    let wardrobe = Wardrobe::of(dom, model)?;
    (!wardrobe.for_limb(limb).is_empty()).then_some(Dressing {
        limb,
        model,
        wardrobe,
    })
}

impl Dressing {
    /// The images the clothes are cut from.
    pub(super) fn images(&self) -> impl Iterator<Item = &AssetRef> {
        self.wardrobe
            .for_limb(self.limb)
            .into_iter()
            .map(|(_, layer)| &layer.image)
    }
}

/// The dressed geometry and image of one section.
pub(super) struct Dressed {
    pub(super) mesh: (AssetRef, Arc<rbx_mesh::Mesh>),
    pub(super) image: (AssetRef, Arc<Image>),
}

fn mesh_key(referent: Ref) -> AssetRef {
    AssetRef::Thumb(format!("clothed-mesh/{referent:?}"))
}

/// The key of the composite `entry` draws, `None` when none of its garments
/// has an image yet. Carries everything the composite depends on, so an edit
/// that changes any of it asks for a new one.
pub(super) fn image_key(entry: &Entry, images: &HashMap<AssetRef, Arc<Image>>) -> Option<AssetRef> {
    let dressing = entry.dressing.as_ref()?;
    let layers = layers(dressing, images);
    if layers.is_empty() {
        return None;
    }
    let [r, g, b] = body_colour(entry.color);
    let worn: Vec<String> = layers
        .iter()
        .map(|(garment, layer, _)| format!("{garment:?}:{:?}:{:?}", layer.image, layer.tint))
        .collect();
    Some(AssetRef::Thumb(format!(
        "clothed/{:?}/{r:02x}{g:02x}{b:02x}/{}",
        dressing.limb,
        worn.join("/")
    )))
}

/// The keys under which `entry`'s dressed mesh and image are held.
pub(super) fn keys(
    entry: &Entry,
    images: &HashMap<AssetRef, Arc<Image>>,
) -> Option<(AssetRef, AssetRef)> {
    Some((mesh_key(entry.referent), image_key(entry, images)?))
}

fn layers<'a>(
    dressing: &'a Dressing,
    images: &'a HashMap<AssetRef, Arc<Image>>,
) -> Vec<(Garment, &'a clothing::Layer, &'a Arc<Image>)> {
    dressing
        .wardrobe
        .for_limb(dressing.limb)
        .into_iter()
        .filter_map(|(garment, layer)| Some((garment, layer, images.get(&layer.image)?)))
        .collect()
}

fn body_colour(linear: [f32; 3]) -> [u8; 3] {
    linear.map(|channel| {
        (scene::linear_to_srgb(channel) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

/// Dresses every limb of the plan whose meshes and garments have all arrived,
/// answering each section's dressed mesh and image by its referent.
pub(super) fn derive(
    entries: &[Entry],
    meshes: &HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
    images: &HashMap<AssetRef, Arc<Image>>,
) -> HashMap<Ref, Dressed> {
    let mut limbs: Vec<((Ref, Limb), Vec<&Entry>)> = Vec::new();
    for entry in entries {
        let Some(dressing) = &entry.dressing else {
            continue;
        };
        let key = (dressing.model, dressing.limb);
        match limbs.iter_mut().find(|(held, _)| *held == key) {
            Some((_, sections)) => sections.push(entry),
            None => limbs.push((key, vec![entry])),
        }
    }

    let mut dressed = HashMap::new();
    let mut composites: HashMap<AssetRef, Arc<Image>> = HashMap::new();
    for ((_, limb), sections) in limbs {
        // A limb is only dressed whole: the box its template was cut for is
        // measured over all of its sections.
        let Some(parts) = sections
            .iter()
            .map(|entry| Some((*entry, meshes.get(&entry.mesh)?)))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(frame) = Frame::of(limb, &parts) else {
            continue;
        };
        for (entry, mesh) in parts {
            let Some(image_key) = image_key(entry, images) else {
                continue;
            };
            let image = composites
                .entry(image_key.clone())
                .or_insert_with(|| Arc::new(composite(entry, images)))
                .clone();
            dressed.insert(
                entry.referent,
                Dressed {
                    mesh: (
                        mesh_key(entry.referent),
                        Arc::new(frame.remap(&entry.fit.transform(mesh), mesh)),
                    ),
                    image: (image_key, image),
                },
            );
        }
    }
    dressed
}

/// The box of one limb, in the frame of its sections' own axes.
struct Frame {
    limb: Limb,
    /// World to limb axes.
    axes: Mat3,
    min: Vec3,
    size: Vec3,
}

impl Frame {
    fn of(limb: Limb, parts: &[(&Entry, &Arc<rbx_mesh::Mesh>)]) -> Option<Frame> {
        let (first, mesh) = parts.first()?;
        let rotation = Mat3::from_mat4(first.fit.transform(mesh));
        let axes = Mat3::from_cols(
            rotation.x_axis.normalize_or_zero(),
            rotation.y_axis.normalize_or_zero(),
            rotation.z_axis.normalize_or_zero(),
        )
        .transpose();

        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for (entry, mesh) in parts {
            let model = entry.fit.transform(mesh);
            for &index in &mesh.indices {
                let vertex = mesh.vertices.get(index as usize)?;
                let at = axes * model.transform_point3(Vec3::from(vertex.position));
                min = min.min(at);
                max = max.max(at);
            }
        }
        let size = max - min;
        (size.min_element() > f32::EPSILON).then_some(Frame {
            limb,
            axes,
            min,
            size,
        })
    }

    /// `mesh`, drawn through `model`, with every triangle given its own
    /// vertices and UVs into the template.
    fn remap(&self, model: &Mat4, mesh: &rbx_mesh::Mesh) -> rbx_mesh::Mesh {
        let normals = Mat3::from_mat4(*model).inverse().transpose();
        let mut vertices = Vec::with_capacity(mesh.indices.len());
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let corners: Vec<&rbx_mesh::Vertex> = triangle
                .iter()
                .filter_map(|&index| mesh.vertices.get(index as usize))
                .collect();
            let [a, b, c] = corners[..] else { continue };
            let at = |vertex: &rbx_mesh::Vertex| {
                self.axes * model.transform_point3(Vec3::from(vertex.position))
            };
            let (pa, pb, pc) = (at(a), at(b), at(c));
            // The winding does not say which way the triangle faces in every
            // file, the vertex normals do.
            let facing: Vec3 = corners
                .iter()
                .map(|vertex| self.axes * (normals * Vec3::from(vertex.normal)))
                .sum();
            let mut normal = (pb - pa).cross(pc - pa);
            if normal.dot(facing) < 0.0 {
                normal = -normal;
            }
            let face = face_of(normal);
            for (vertex, point) in [(a, pa), (b, pb), (c, pc)] {
                vertices.push(rbx_mesh::Vertex {
                    uv: self.uv(face, point),
                    ..*vertex
                });
            }
        }
        let faces = (vertices.len() / 3) as u32;
        rbx_mesh::Mesh {
            version: mesh.version,
            indices: (0..vertices.len() as u32).collect(),
            vertices,
            #[allow(clippy::single_range_in_vec_init)]
            lods: vec![0..faces],
            bounds: mesh.bounds,
        }
    }

    /// Where `point` of the limb box lands in the template, taking it as lying
    /// on `face`. Kept half a pixel inside the face's rectangle, so filtering
    /// never reaches into its neighbour.
    fn uv(&self, face: NormalId, point: Vec3) -> [f32; 2] {
        let (_, u, v) = clothing::axes(face);
        let centre = point - (self.min + self.size / 2.0);
        let along =
            |axis: Vec3| (0.5 + centre.dot(axis) / self.size.dot(axis.abs())).clamp(0.0, 1.0);
        let [x, y, w, h] = self.limb.rect(face);
        [
            (x + 0.5 + along(u) * (w - 1.0)) / TEMPLATE[0],
            (y + 0.5 + along(v) * (h - 1.0)) / TEMPLATE[1],
        ]
    }
}

/// The face of a box a surface with this normal lies on: the axis it leans on
/// most, ties going to Y, then X.
fn face_of(normal: Vec3) -> NormalId {
    let axis = normal.abs();
    if axis.y >= axis.x && axis.y >= axis.z {
        if normal.y >= 0.0 {
            NormalId::Top
        } else {
            NormalId::Bottom
        }
    } else if axis.x >= axis.z {
        if normal.x >= 0.0 {
            NormalId::Right
        } else {
            NormalId::Left
        }
    } else if normal.z >= 0.0 {
        NormalId::Back
    } else {
        NormalId::Front
    }
}

/// The garments of `entry` laid over its body colour, at the template's size.
fn composite(entry: &Entry, images: &HashMap<AssetRef, Arc<Image>>) -> Image {
    let (width, height) = (TEMPLATE[0] as usize, TEMPLATE[1] as usize);
    let [r, g, b] = body_colour(entry.color);
    let mut pixels = [r, g, b, u8::MAX].repeat(width * height);
    let Some(dressing) = &entry.dressing else {
        return Image {
            width: width as u32,
            height: height as u32,
            pixels,
        };
    };

    for (garment, layer, image) in layers(dressing, images) {
        let region = match garment {
            Garment::Graphic => dressing.limb.rect(NormalId::Front),
            _ => [0.0, 0.0, TEMPLATE[0], TEMPLATE[1]],
        };
        let [rx, ry, rw, rh] = region.map(|value| value as usize);
        for y in ry..(ry + rh).min(height) {
            for x in rx..(rx + rw).min(width) {
                let sx = ((x - rx) * image.width as usize / rw).min(image.width as usize - 1);
                let sy = ((y - ry) * image.height as usize / rh).min(image.height as usize - 1);
                let at = (sy * image.width as usize + sx) * 4;
                let source = &image.pixels[at..at + 4];
                let alpha = f32::from(source[3]) / 255.0;
                let out = &mut pixels[(y * width + x) * 4..][..3];
                for channel in 0..3 {
                    let tinted = f32::from(source[channel]) * layer.tint[channel].clamp(0.0, 1.0);
                    out[channel] =
                        (tinted * alpha + f32::from(out[channel]) * (1.0 - alpha)).round() as u8;
                }
            }
        }
    }
    Image {
        width: width as u32,
        height: height as u32,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::material::Catalog;
    use rbx_dom::{CFrameData, Instance, Variant, Vector3Data};
    use rbx_reflection::ReflectionDatabase;

    fn cube() -> rbx_mesh::Mesh {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (normal, u, v) in [
            (Vec3::X, Vec3::Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Y, Vec3::Z),
            (Vec3::Y, Vec3::X, Vec3::Z),
            (Vec3::NEG_Y, Vec3::Z, Vec3::X),
            (Vec3::Z, Vec3::Y, Vec3::X),
            (Vec3::NEG_Z, Vec3::X, Vec3::Y),
        ] {
            let at = indices.len() as u32;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                vertices.push(rbx_mesh::Vertex {
                    position: (normal * 0.5 + u * a * 0.5 + v * b * 0.5).to_array(),
                    normal: normal.to_array(),
                    uv: [0.0; 2],
                    color: [255; 4],
                });
            }
            let base = (vertices.len() - 4) as u32;
            let _ = at;
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        rbx_mesh::Mesh {
            version: (4, 1),
            vertices,
            indices,
            #[allow(clippy::single_range_in_vec_init)]
            lods: vec![0..12],
            bounds: rbx_mesh::Aabb {
                min: [-0.5; 3],
                max: [0.5; 3],
            },
        }
    }

    fn character(shirt: &str) -> WeakDom {
        let mut dom = WeakDom::new();
        let workspace = Ref::new(9000);
        dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
        dom.set_parent(workspace, None);
        let model = Ref::new(10);
        dom.insert(Instance::new(model, "Model", "Rig"));
        dom.set_parent(model, Some(workspace));
        let mut cloth = Instance::new(Ref::new(11), "Shirt", "Shirt");
        cloth.properties_mut().insert(
            "ShirtTemplate".to_string(),
            Variant::String(shirt.to_string()),
        );
        dom.insert(cloth);
        dom.set_parent(Ref::new(11), Some(model));
        let mut torso = Instance::new(Ref::new(12), "MeshPart", "UpperTorso");
        let properties = torso.properties_mut();
        properties.insert(
            "MeshId".to_string(),
            Variant::String("rbxassetid://1".to_string()),
        );
        properties.insert(
            "size".to_string(),
            Variant::Vector3(Vector3Data {
                x: 2.0,
                y: 1.0,
                z: 1.0,
            }),
        );
        properties.insert(
            "CFrame".to_string(),
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        );
        dom.insert(torso);
        dom.set_parent(Ref::new(12), Some(model));
        dom
    }

    fn red() -> Arc<Image> {
        Arc::new(Image {
            width: 585,
            height: 559,
            pixels: [200, 0, 0, 255].repeat(585 * 559),
        })
    }

    fn dressed(shirt: &str) -> Option<(super::super::Resolved, Vec<[f32; 2]>)> {
        let dom = character(shirt);
        let database = ReflectionDatabase::embedded();
        let plan = super::super::plan(&dom, &database, &mut Catalog::new(&dom, &database));
        let meshes =
            HashMap::from([(AssetRef::parse("rbxassetid://1").unwrap(), Arc::new(cube()))]);
        let images = HashMap::from([(AssetRef::parse("rbxassetid://5").unwrap(), red())]);
        let (resolved, _) = super::super::resolve(&plan, meshes, images);
        let instance = resolved.instances.first()?;
        let mesh = resolved.meshes.get(&instance.mesh)?;
        let uvs = mesh.vertices.iter().map(|vertex| vertex.uv).collect();
        Some((resolved, uvs))
    }

    #[test]
    fn a_shirt_on_an_r15_torso_is_cut_into_the_templates_faces() {
        let (resolved, uvs) = dressed("rbxassetid://5").unwrap();
        let instance = &resolved.instances[0];
        assert_eq!(instance.color, [1.0; 3]);
        let texture = instance.texture.clone().unwrap();
        assert!(matches!(&texture, AssetRef::Thumb(key) if key.starts_with("clothed/Torso")));
        let image = &resolved.images[&texture];
        assert_eq!((image.width, image.height), (585, 559));

        // 12 triangles, each with its own three vertices.
        assert_eq!(uvs.len(), 36);
        // Front (-Z) is the template's (231, 74, 128, 128) cell: the last two
        // faces of the cube, in file order, are +Z then -Z.
        let front = &uvs[30..36];
        assert!(front.iter().all(|&[u, v]| {
            (231.0 / 585.0..=359.0 / 585.0).contains(&u)
                && (74.0 / 559.0..=202.0 / 559.0).contains(&v)
        }));
        // The shirt covers that cell, the corner of the atlas stays body colour.
        let at = |x: usize, y: usize| &image.pixels[(y * 585 + x) * 4..][..3];
        assert_eq!(at(290, 130), [200, 0, 0]);
    }

    #[test]
    fn a_shirt_whose_texture_has_not_arrived_leaves_the_part_plain() {
        let (resolved, _) = dressed("rbxassetid://404").unwrap();
        assert!(resolved.instances[0].texture.is_none());
    }
}
