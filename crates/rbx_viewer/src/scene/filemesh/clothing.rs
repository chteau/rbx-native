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
    // A body's own texture is laid out by its mesh, so what it makes is that
    // section's alone.
    let beneath = match base_of(entry, images) {
        Some((reference, ..)) => format!("/under:{reference:?}@{:?}", entry.referent),
        None => String::new(),
    };
    Some(AssetRef::Thumb(format!(
        "clothed/{:?}/{r:02x}{g:02x}{b:02x}/{}{beneath}",
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

/// The image a section's own skin is painted from, once it has arrived: its
/// `TextureID`, or the colour map of its `SurfaceAppearance` with that
/// appearance's tint. The clothes are laid over it, as over the body colour a
/// plain part has.
fn base_of<'a>(
    entry: &'a Entry,
    images: &'a HashMap<AssetRef, Arc<Image>>,
) -> Option<(&'a AssetRef, &'a Arc<Image>, [f32; 3])> {
    let (reference, tint) = match (&entry.texture, &entry.appearance) {
        (Some(texture), _) => (texture, [1.0; 3]),
        (None, Some(appearance)) => (
            appearance.maps[0].as_ref()?,
            appearance.tint.map(scene::linear_to_srgb),
        ),
        (None, None) => return None,
    };
    Some((reference, images.get(reference)?, tint))
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
    let mut composites: HashMap<AssetRef, (Arc<Image>, Vec<bool>)> = HashMap::new();
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
            let model = entry.fit.transform(mesh);
            let base = base_of(entry, images);
            let covered = match composites.get(&image_key) {
                Some((_, covered)) if base.is_none() => covered.clone(),
                _ => coverage(entry, images),
            };
            let (remapped, sources) = frame.remap(&model, mesh, &covered);
            let skin = base.map(|(_, image, tint)| Skin {
                image,
                tint,
                sources: &sources,
                targets: &remapped,
            });
            let (image, _) = composites.entry(image_key.clone()).or_insert_with(|| {
                let (image, covered) = composite(entry, images, skin.as_ref());
                (Arc::new(image), covered)
            });
            dressed.insert(
                entry.referent,
                Dressed {
                    mesh: (mesh_key(entry.referent), Arc::new(remapped)),
                    image: (image_key, image.clone()),
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
    ///
    /// Also answers where each new vertex was in the mesh's own UVs, which is
    /// how a body's own texture is carried across.
    fn remap(
        &self,
        model: &Mat4,
        mesh: &rbx_mesh::Mesh,
        covered: &[bool],
    ) -> (rbx_mesh::Mesh, Vec<[f32; 2]>) {
        let normals = Mat3::from_mat4(*model).inverse().transpose();
        let mut vertices = Vec::with_capacity(mesh.indices.len());
        let mut sources = Vec::with_capacity(mesh.indices.len());
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
            let mut face = face_of(normal);
            let bare = |face| {
                [pa, pb, pc].iter().all(|&point| {
                    let [u, v] = self.uv(face, point);
                    let at = (v * TEMPLATE[1]) as usize * TEMPLATE[0] as usize
                        + (u * TEMPLATE[0]) as usize;
                    !covered.get(at).copied().unwrap_or(true)
                })
            };
            // Joints (the armpits, the collar, the wrists) are cut through the
            // box's caps, where a template has nothing drawn: they take the
            // side they lean on next.
            if matches!(face, NormalId::Top | NormalId::Bottom) && bare(face) {
                face = face_of(Vec3::new(normal.x, 0.0, normal.z));
            }
            for (vertex, point) in [(a, pa), (b, pb), (c, pc)] {
                sources.push(vertex.uv);
                vertices.push(rbx_mesh::Vertex {
                    uv: self.uv(face, point),
                    ..*vertex
                });
            }
        }
        let faces = (vertices.len() / 3) as u32;
        let remapped = rbx_mesh::Mesh {
            version: mesh.version,
            indices: (0..vertices.len() as u32).collect(),
            vertices,
            #[allow(clippy::single_range_in_vec_init)]
            lods: vec![0..faces],
            bounds: mesh.bounds,
        };
        (remapped, sources)
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

/// The share of a torso's front that is the UpperTorso, which is what a
/// T-shirt's graphic is printed on: an R15 torso is 1.6 studs of UpperTorso
/// over 0.4 of LowerTorso.
const UPPER_TORSO: f32 = 0.8;

/// Where a T-shirt's graphic lies in the template, as `[x, y, w, h]`: the
/// largest square of the UpperTorso's front, centred across the chest and
/// level with its top, holding the image at its own aspect.
fn graphic_region(image: &Image) -> [f32; 4] {
    let [x, y, w, h] = Limb::Torso.rect(NormalId::Front);
    let side = (h * UPPER_TORSO).round().min(w);
    let aspect = image.width.max(1) as f32 / image.height.max(1) as f32;
    let (dw, dh) = if aspect >= 1.0 {
        (side, (side / aspect).round().max(1.0))
    } else {
        ((side * aspect).round().max(1.0), side)
    };
    [
        x + ((w - dw) / 2.0).floor(),
        y + ((side - dh) / 2.0).floor(),
        dw,
        dh,
    ]
}

/// Hands `each` every template pixel a garment of `entry` draws on, with the
/// garment's own pixel there, bottom layer first.
fn garment_pixels(
    entry: &Entry,
    images: &HashMap<AssetRef, Arc<Image>>,
    mut each: impl FnMut(usize, &[u8], &clothing::Layer),
) {
    let Some(dressing) = &entry.dressing else {
        return;
    };
    let (width, height) = (TEMPLATE[0] as usize, TEMPLATE[1] as usize);
    for (garment, layer, image) in layers(dressing, images) {
        let region = match garment {
            Garment::Graphic => graphic_region(image),
            _ => [0.0, 0.0, TEMPLATE[0], TEMPLATE[1]],
        };
        let [rx, ry, rw, rh] = region.map(|value| value as usize);
        for y in ry..(ry + rh).min(height) {
            for x in rx..(rx + rw).min(width) {
                let sx = ((x - rx) * image.width as usize / rw).min(image.width as usize - 1);
                let sy = ((y - ry) * image.height as usize / rh).min(image.height as usize - 1);
                let at = (sy * image.width as usize + sx) * 4;
                each(y * width + x, &image.pixels[at..at + 4], layer);
            }
        }
    }
}

/// Which template pixels a garment of `entry` draws on at all.
fn coverage(entry: &Entry, images: &HashMap<AssetRef, Arc<Image>>) -> Vec<bool> {
    let mut covered = vec![false; TEMPLATE[0] as usize * TEMPLATE[1] as usize];
    garment_pixels(entry, images, |at, source, _| covered[at] |= source[3] > 0);
    covered
}

/// A body's own texture, and where each of its triangles went in the template.
struct Skin<'a> {
    image: &'a Image,
    tint: [f32; 3],
    /// The UVs of every vertex of `targets` in the mesh before it was remapped.
    sources: &'a [[f32; 2]],
    targets: &'a rbx_mesh::Mesh,
}

impl Skin<'_> {
    /// Paints the texture into the template wherever a triangle of the body
    /// now lies, over what is there.
    fn paint(&self, pixels: &mut [u8]) {
        let (width, height) = (TEMPLATE[0] as usize, TEMPLATE[1] as usize);
        let at = |uv: [f32; 2]| [uv[0] * TEMPLATE[0], uv[1] * TEMPLATE[1]];
        for (corners, from) in self
            .targets
            .vertices
            .as_chunks::<3>()
            .0
            .iter()
            .zip(self.sources.as_chunks::<3>().0)
        {
            let [a, b, c] = [0, 1, 2].map(|i| at(corners[i].uv));
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if area.abs() < f32::EPSILON {
                continue;
            }
            let xs = [a[0], b[0], c[0]];
            let ys = [a[1], b[1], c[1]];
            let (x0, x1) = (min3(xs).floor().max(0.0) as usize, max3(xs).ceil() as usize);
            let (y0, y1) = (min3(ys).floor().max(0.0) as usize, max3(ys).ceil() as usize);
            for y in y0..y1.min(height) {
                for x in x0..x1.min(width) {
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    let wa = ((b[0] - p[0]) * (c[1] - p[1]) - (c[0] - p[0]) * (b[1] - p[1])) / area;
                    let wb = ((c[0] - p[0]) * (a[1] - p[1]) - (a[0] - p[0]) * (c[1] - p[1])) / area;
                    let wc = 1.0 - wa - wb;
                    // A little outside counts, so that seams between triangles
                    // do not let the body colour through.
                    if wa < -0.05 || wb < -0.05 || wc < -0.05 {
                        continue;
                    }
                    let u = wa * from[0][0] + wb * from[1][0] + wc * from[2][0];
                    let v = wa * from[0][1] + wb * from[1][1] + wc * from[2][1];
                    let sx = (u.clamp(0.0, 1.0) * self.image.width as f32) as usize;
                    let sy = (v.clamp(0.0, 1.0) * self.image.height as f32) as usize;
                    let sx = sx.min(self.image.width as usize - 1);
                    let sy = sy.min(self.image.height as usize - 1);
                    let source =
                        &self.image.pixels[(sy * self.image.width as usize + sx) * 4..][..4];
                    let alpha = f32::from(source[3]) / 255.0;
                    let out = &mut pixels[(y * width + x) * 4..][..3];
                    for channel in 0..3 {
                        let tinted =
                            f32::from(source[channel]) * self.tint[channel].clamp(0.0, 1.0);
                        out[channel] = (tinted * alpha + f32::from(out[channel]) * (1.0 - alpha))
                            .round() as u8;
                    }
                }
            }
        }
    }
}

fn min3(values: [f32; 3]) -> f32 {
    values[0].min(values[1]).min(values[2])
}

fn max3(values: [f32; 3]) -> f32 {
    values[0].max(values[1]).max(values[2])
}

/// The garments of `entry` laid over its skin: the body's own texture where
/// it has one, its body colour elsewhere. Clothes are drawn source-over, so a
/// garment's transparent pixels let the skin through.
fn composite(
    entry: &Entry,
    images: &HashMap<AssetRef, Arc<Image>>,
    skin: Option<&Skin>,
) -> (Image, Vec<bool>) {
    let (width, height) = (TEMPLATE[0] as usize, TEMPLATE[1] as usize);
    let [r, g, b] = body_colour(entry.color);
    let mut pixels = [r, g, b, u8::MAX].repeat(width * height);
    if let Some(skin) = skin {
        skin.paint(&mut pixels);
    }
    let mut covered = vec![false; width * height];
    garment_pixels(entry, images, |at, source, layer| {
        let alpha = f32::from(source[3]) / 255.0;
        covered[at] |= source[3] > 0;
        let out = &mut pixels[at * 4..][..3];
        for channel in 0..3 {
            let tinted = f32::from(source[channel]) * layer.tint[channel].clamp(0.0, 1.0);
            out[channel] = (tinted * alpha + f32::from(out[channel]) * (1.0 - alpha)).round() as u8;
        }
    });
    bleed(&mut pixels, &covered, width);
    let image = Image {
        width: width as u32,
        height: height as u32,
        pixels,
    };
    (image, covered)
}

/// How far, in template pixels, a garment's edge is carried into the bare
/// pixels beside it.
const BLEED: usize = 4;

/// Carries the garments' edge pixels into the uncovered pixels within
/// [`BLEED`] of them, inside each face of each limb. R15 meshes reach a little
/// past the template's drawn area at the armpits, collar and wrists, and
/// without this the body colour shows there as wedges.
fn bleed(pixels: &mut [u8], covered: &[bool], width: usize) {
    let height = covered.len() / width;
    let limbs = [
        Limb::Torso,
        Limb::RightArm,
        Limb::LeftArm,
        Limb::RightLeg,
        Limb::LeftLeg,
    ];
    let source = pixels.to_vec();
    for limb in limbs {
        for face in clothing::FACES {
            let [rx, ry, rw, rh] = limb.rect(face).map(|value| value as usize);
            let (x1, y1) = ((rx + rw).min(width), (ry + rh).min(height));
            for y in ry..y1 {
                for x in rx..x1 {
                    if covered[y * width + x] {
                        continue;
                    }
                    let nearest = (y.saturating_sub(BLEED).max(ry)..(y + BLEED + 1).min(y1))
                        .flat_map(|ny| {
                            (x.saturating_sub(BLEED).max(rx)..(x + BLEED + 1).min(x1))
                                .map(move |nx| (nx, ny))
                        })
                        .filter(|&(nx, ny)| covered[ny * width + nx])
                        .min_by_key(|&(nx, ny)| nx.abs_diff(x).pow(2) + ny.abs_diff(y).pow(2));
                    if let Some((nx, ny)) = nearest {
                        let from = (ny * width + nx) * 4;
                        pixels[(y * width + x) * 4..][..3].copy_from_slice(&source[from..from + 3]);
                    }
                }
            }
        }
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
    fn a_body_with_its_own_texture_shows_it_wherever_the_shirt_is_clear() {
        let mut dom = character("rbxassetid://5");
        dom.get_mut(Ref::new(12)).unwrap().properties_mut().insert(
            "TextureID".to_string(),
            Variant::String("rbxassetid://6".to_string()),
        );
        let database = ReflectionDatabase::embedded();
        let plan = super::super::plan(&dom, &database, &mut Catalog::new(&dom, &database));
        let meshes =
            HashMap::from([(AssetRef::parse("rbxassetid://1").unwrap(), Arc::new(cube()))]);
        // A shirt drawn on the front alone, over a green skin.
        let [fx, fy, fw, fh] = Limb::Torso.rect(NormalId::Front);
        let mut shirt = vec![0; 585 * 559 * 4];
        for y in fy as usize..(fy + fh) as usize {
            for x in fx as usize..(fx + fw) as usize {
                shirt[(y * 585 + x) * 4..][..4].copy_from_slice(&[200, 0, 0, 255]);
            }
        }
        let images = HashMap::from([
            (
                AssetRef::parse("rbxassetid://5").unwrap(),
                Arc::new(Image {
                    width: 585,
                    height: 559,
                    pixels: shirt,
                }),
            ),
            (
                AssetRef::parse("rbxassetid://6").unwrap(),
                Arc::new(Image {
                    width: 4,
                    height: 4,
                    pixels: [0, 200, 0, 255].repeat(16),
                }),
            ),
        ]);
        let (resolved, _) = super::super::resolve(&plan, meshes, images);
        let instance = &resolved.instances[0];
        assert!(instance.appearance.is_none());
        let image = &resolved.images[&instance.texture.clone().unwrap()];
        let at = |rect: [f32; 4]| {
            let (x, y) = (
                (rect[0] + rect[2] / 2.0) as usize,
                (rect[1] + rect[3] / 2.0) as usize,
            );
            image.pixels[(y * 585 + x) * 4..][..3].to_vec()
        };
        assert_eq!(at(Limb::Torso.rect(NormalId::Front)), [200, 0, 0]);
        assert_eq!(at(Limb::Torso.rect(NormalId::Right)), [0, 200, 0]);
    }

    #[test]
    fn a_shirt_whose_texture_has_not_arrived_leaves_the_part_plain() {
        let (resolved, _) = dressed("rbxassetid://404").unwrap();
        assert!(resolved.instances[0].texture.is_none());
    }

    #[test]
    fn a_garments_edge_is_carried_a_few_pixels_into_the_bare_template() {
        let [x, y, w, _] = Limb::Torso.rect(NormalId::Front);
        let (x, y, w) = (x as usize, y as usize, w as usize);
        let mut pixels = [9, 9, 9, 255].repeat(TEMPLATE[0] as usize * TEMPLATE[1] as usize);
        let mut covered = vec![false; pixels.len() / 4];
        let width = TEMPLATE[0] as usize;
        for cx in x..x + w / 2 {
            covered[y * width + cx] = true;
            pixels[(y * width + cx) * 4..][..3].copy_from_slice(&[200, 0, 0]);
        }
        bleed(&mut pixels, &covered, width);
        let at = |px: usize| pixels[(y * width + px) * 4];
        assert_eq!(at(x + w / 2 + BLEED - 1), 200, "within reach");
        assert_eq!(at(x + w / 2 + BLEED + 1), 9, "beyond reach");
        assert_eq!(at(x.saturating_sub(1)), 9, "outside the face");
    }

    #[test]
    fn a_graphic_is_a_centred_square_on_the_upper_torso_at_its_own_aspect() {
        let [x, y, w, h] = Limb::Torso.rect(NormalId::Front);
        let image = |width, height| Image {
            width,
            height,
            pixels: Vec::new(),
        };
        let [gx, gy, gw, gh] = graphic_region(&image(420, 420));
        assert_eq!(gw, gh);
        assert!(gw <= h * UPPER_TORSO && gw <= w);
        assert_eq!(gx - x, (x + w) - (gx + gw), "centred across the chest");
        assert_eq!(gy, y, "level with the top");
        let [_, _, gw, gh] = graphic_region(&image(400, 200));
        assert_eq!(gw, gh * 2.0);
    }
}
