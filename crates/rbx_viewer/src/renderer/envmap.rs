//! The environment probe: the six sky panels rebuilt as a mipmapped cube map so
//! `Reflectance`, `EnvironmentSpecularScale` and `EnvironmentDiffuseScale` have
//! something to sample.
//!
//! The mip chain is the prefilter the specular terms read: the rougher the
//! surface, the smaller the level. The diffuse term does not sample the cube at
//! all — it reads the six irradiance values `irradiance` integrates here, since
//! no single texel of a sky is what an upward face is actually lit by.

mod irradiance;

use rbx_assets::AssetRef;

use super::texture;
use crate::assets::Image;
use crate::quality::QualityProfile;
use crate::textures::Panel;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const CHANNELS: usize = 4;
const FACES: usize = irradiance::FACES;
/// Face size the diffuse integral is taken at. The irradiance of a sky is a
/// very low frequency signal — sixteen texels a side already agree with the
/// full-size integral to well under one sRGB step, at a 4096th of the cost.
const IRRADIANCE_FACE_SIZE: u32 = 16;
/// Mid grey, in the sRGB bytes the texture stores: what a scene with no `Sky`
/// reflects, so a reflective part looks like metal in an overcast room rather
/// than like a hole in the world.
const NO_SKY_GREY: u8 = 128;

/// The probe, ready to bind: a cube view, its sampler, and the numbers the
/// lighting uniform carries about it.
pub(super) struct EnvMap {
    /// The whole cube, every level of it, kept alive so a change of quality level
    /// only re-views it (see [`EnvMap::set_quality`]) instead of rebuilding the
    /// sky's prefilter.
    cube: wgpu::Texture,
    /// Face side and mip depth of `cube`, which is what a texture cap is applied
    /// against.
    size: u32,
    levels: u32,
    pub(super) view: wgpu::TextureView,
    pub(super) sampler: wgpu::Sampler,
    pub(super) probe: Probe,
    /// Which panels the cube was prefiltered from (see [`sky_key`]), so a
    /// scene rebuild whose `Sky` did not change keeps the probe instead of
    /// resampling six faces and integrating their irradiance again.
    sky: Option<Vec<AssetRef>>,
}

/// The identity of a sky as far as its uploads go: the six panel assets in
/// face order, or `None` for a scene drawing no sky at all. The panel quads
/// are fixed per face (see `textures::sky`), and the same asset decodes to
/// the same texels, so two skies with the same key build the same probe and
/// the same skybox.
pub(super) fn sky_key(panels: Option<&[Panel]>) -> Option<Vec<AssetRef>> {
    panels.map(|panels| panels.iter().map(|panel| panel.reference.clone()).collect())
}

/// What the shaders need to know about the probe once it is bound: how deep its
/// mip chain is, and the sky irradiance the diffuse term rebuilds from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Probe {
    /// Highest mip level, which the shader stretches Roblox's fixed roughness
    /// curve over however many levels the sky panels allowed.
    pub(super) top_mip: f32,
    /// Cosine-weighted sky irradiance at the six cardinal normals, in cube map
    /// layer order.
    pub(super) irradiance: [[f32; 4]; FACES],
}

impl EnvMap {
    /// Builds the probe from the panels the skybox already decoded, or a 1x1 grey
    /// cube when the scene has no usable sky.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        panels: Option<&[Panel]>,
        quality: &QualityProfile,
    ) -> Self {
        let faces = panels.and_then(cube_faces).unwrap_or_else(grey_faces);
        let chains: Vec<Vec<Image>> = faces.iter().map(texture::mip_chain).collect();
        let size = chains[0][0].width;
        let levels = chains[0].len();

        let cube = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rbxview environment cube"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: FACES as u32,
            },
            mip_level_count: u32::try_from(levels).unwrap_or(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        for (face, chain) in chains.iter().enumerate() {
            for (level, mip) in chain.iter().enumerate() {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &cube,
                        mip_level: u32::try_from(level).unwrap_or(0),
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: u32::try_from(face).unwrap_or(0),
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &mip.pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(mip.width * CHANNELS as u32),
                        rows_per_image: Some(mip.height),
                    },
                    wgpu::Extent3d {
                        width: mip.width,
                        height: mip.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }

        let levels = u32::try_from(levels).unwrap_or(1);
        let (view, top_mip) = viewed(&cube, size, levels, quality.texture_max_size);

        EnvMap {
            probe: Probe {
                top_mip,
                // Cap-independent on purpose: the integral runs over a 16-texel
                // face, which every cap in the table leaves in the chain.
                irradiance: irradiance::axis_irradiance(&integration_faces(&chains)),
            },
            cube,
            size,
            levels,
            view,
            sampler: texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy),
            sky: sky_key(panels),
        }
    }

    /// Whether this probe was built from exactly `panels` (see [`sky_key`]),
    /// which is what lets a scene rebuild keep it.
    pub(super) fn holds(&self, panels: Option<&[Panel]>) -> bool {
        self.sky == sky_key(panels)
    }

    /// Re-views the probe at the new texture cap and rebuilds its sampler at the
    /// new anisotropy. The caller must rebind every bind group holding the old
    /// view, and rewrite the lighting uniform for the new top mip.
    pub(super) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        let (view, top_mip) = viewed(&self.cube, self.size, self.levels, quality.texture_max_size);
        self.view = view;
        self.probe.top_mip = top_mip;
        self.sampler = texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);
    }
}

/// The cube view a cap of `max_size` asks for, and the top mip level left in it —
/// which is the number the specular terms stretch Roblox's roughness curve over.
fn viewed(cube: &wgpu::Texture, size: u32, levels: u32, max_size: u32) -> (wgpu::TextureView, f32) {
    let base = texture::skipped(size, levels, max_size);
    let view = cube.create_view(&wgpu::TextureViewDescriptor {
        label: Some("rbxview environment cube"),
        dimension: Some(wgpu::TextureViewDimension::Cube),
        base_mip_level: base,
        mip_level_count: Some(levels - base),
        ..Default::default()
    });

    (view, (levels - base - 1) as f32)
}

/// The mip level of each face closest to [`IRRADIANCE_FACE_SIZE`], which is
/// what the diffuse integral runs over.
fn integration_faces(chains: &[Vec<Image>]) -> Vec<Image> {
    chains
        .iter()
        .filter_map(|chain| {
            chain
                .iter()
                .find(|mip| mip.width <= IRRADIANCE_FACE_SIZE)
                .or_else(|| chain.last())
                .cloned()
        })
        .collect()
}

/// Quarter turns needed per panel (only `Up` and `Dn` are rotated).
/// Sky panel order already matches cube layer order (+X, -X, +Y, -Y, +Z, -Z);
/// five panels need no rotation, but `Up` and `Dn` need adjustment for GL/D3D convention.
const QUARTER_TURNS: [u32; FACES] = [0, 0, 1, 3, 0, 0];

/// Resamples the six panels into equal square faces, turning the two that need
/// it. Cube faces must be square and the same size, and sky panels are neither
/// guaranteed to be: the smallest side wins, which loses detail on a mismatched
/// sky rather than refusing to reflect it at all.
fn cube_faces(panels: &[Panel]) -> Option<Vec<Image>> {
    if panels.len() != FACES {
        return None;
    }

    let size = panels
        .iter()
        .map(|panel| panel.image.width.min(panel.image.height))
        .min()
        .filter(|&size| size > 0)?;

    Some(
        panels
            .iter()
            .zip(QUARTER_TURNS)
            .map(|(panel, turns)| square(&panel.image, size, turns))
            .collect(),
    )
}

fn grey_faces() -> Vec<Image> {
    vec![
        Image {
            width: 1,
            height: 1,
            pixels: vec![NO_SKY_GREY, NO_SKY_GREY, NO_SKY_GREY, u8::MAX],
        };
        FACES
    ]
}

/// Nearest-neighbour resample to `size` x `size`, rotated by `turns` quarter
/// turns anticlockwise. Nearest rather than filtered: the very next step builds
/// the whole mip chain by box-averaging, which recovers the smoothing anyway.
fn square(image: &Image, size: u32, turns: u32) -> Image {
    let mut pixels = Vec::with_capacity((size * size) as usize * CHANNELS);

    for y in 0..size {
        for x in 0..size {
            let (turned_x, turned_y) = match turns % 4 {
                1 => (size - 1 - y, x),
                2 => (size - 1 - x, size - 1 - y),
                3 => (y, size - 1 - x),
                _ => (x, y),
            };
            let source_x = (turned_x * image.width / size).min(image.width - 1);
            let source_y = (turned_y * image.height / size).min(image.height - 1);
            let offset = (source_y as usize * image.width as usize + source_x as usize) * CHANNELS;
            pixels.extend_from_slice(&image.pixels[offset..offset + CHANNELS]);
        }
    }

    Image {
        width: size,
        height: size,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textures::Quad;

    fn panel(width: u32, height: u32, value: u8) -> Panel {
        Panel {
            reference: AssetRef::Id(u64::from(value)),
            image: std::sync::Arc::new(Image {
                width,
                height,
                pixels: vec![value; (width * height) as usize * CHANNELS],
            }),
            quad: Quad {
                positions: [[0.0; 3]; 4],
                uvs: [[0.0; 2]; 4],
                normal: [0.0; 3],
            },
        }
    }

    #[test]
    fn a_scene_without_a_sky_reflects_one_grey_texel_per_face() {
        let faces = grey_faces();

        assert_eq!(faces.len(), FACES);
        assert!(faces.iter().all(|face| face.width == 1 && face.height == 1));
    }

    #[test]
    fn mismatched_panels_are_squared_off_to_the_smallest_side() {
        let panels: Vec<Panel> = (0..6).map(|i| panel(8, 4 + i, 0)).collect();

        let faces = cube_faces(&panels).unwrap();

        assert!(faces.iter().all(|face| face.width == 4 && face.height == 4));
    }

    #[test]
    fn a_sky_missing_panels_gets_no_cube_at_all() {
        assert!(cube_faces(&[panel(4, 4, 0)]).is_none());
        assert!(cube_faces(&[]).is_none());
    }

    // The rebuild decision for the probe and the skybox: the same six assets
    // in the same order are the same sky, whatever else in the place changed.
    #[test]
    fn the_same_panel_assets_are_the_same_sky() {
        let panels: Vec<Panel> = (0..6).map(|i| panel(4, 4, i)).collect();
        let again: Vec<Panel> = (0..6).map(|i| panel(8, 8, i)).collect();

        // Image size is not part of it: the same asset decodes the same way
        // every time, so the reference alone says what was uploaded.
        assert_eq!(sky_key(Some(&panels)), sky_key(Some(&again)));
        assert_eq!(sky_key(None), None);
    }

    // A `Sky` edit that swaps one panel — or removes the sky outright — has
    // to invalidate: a stale probe would reflect a sky no longer drawn.
    #[test]
    fn a_changed_or_removed_panel_is_a_different_sky() {
        let panels: Vec<Panel> = (0..6).map(|i| panel(4, 4, i)).collect();
        let mut swapped: Vec<Panel> = (0..6).map(|i| panel(4, 4, i)).collect();
        swapped[2] = panel(4, 4, 9);

        assert_ne!(sky_key(Some(&panels)), sky_key(Some(&swapped)));
        assert_ne!(sky_key(Some(&panels)), sky_key(None));
    }

    // The two panels a quarter turn out of the cube map convention, and the four
    // that are already in it.
    #[test]
    fn only_the_up_and_down_panels_are_turned() {
        assert_eq!(QUARTER_TURNS, [0, 0, 1, 3, 0, 0]);
        // Turning up and down by opposite quarters is what makes their shared
        // borders with the side ring line up; equal turns would not.
        assert_eq!((QUARTER_TURNS[2] + QUARTER_TURNS[3]) % 4, 0);
    }

    #[test]
    fn a_quarter_turn_moves_the_corner_it_should() {
        let image = Image {
            width: 2,
            height: 2,
            // Top-left is the only marked texel.
            pixels: vec![255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
        };

        // One quarter turn anticlockwise sends the top-left corner to the
        // bottom-left, three send it to the top-right.
        assert_eq!(square(&image, 2, 1).pixels[4 * 2], 255);
        assert_eq!(square(&image, 2, 3).pixels[4], 255);
        assert_eq!(square(&image, 2, 0).pixels[0], 255);
    }
}
