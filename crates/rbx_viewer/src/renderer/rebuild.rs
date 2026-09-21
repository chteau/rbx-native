//! Rebuilding a renderer around another scene on the same device.
//!
//! An edit the fast paths cannot take (see `crate::Headless::reload`) means
//! re-deriving every batch from the DOM; it does not mean the GPU has to
//! start over. What a scene decides — instance buffers, shadow casters,
//! effect lists, GUI canvases, the selection's placements — is rebuilt here
//! exactly as [`Renderer::new`] would build it. What it does not decide is
//! kept: the device's pipelines, layouts, samplers, post chain and shadow
//! maps, which no scene touches, and every upload keyed by an asset — the
//! material packs, the sky's probe and panels, the sun and moon, the decal
//! images, the file meshes, the effect and GUI textures — wherever the new
//! scene names the same asset again, which for an edit is nearly always.
//!
//! The one rule that keeps this honest: a kept upload is only ever reused
//! under a key that says it would be uploaded identically (see each pass's
//! `holds`/`rebuild`), never on the assumption that "an edit changes little".
//! A `Sky` edit changes the probe; a material on one part does not change the
//! pack. The keys, not this module, decide which.

use std::collections::{HashMap, HashSet};

use rbx_assets::AssetRef;

use super::envmap::EnvMap;
use super::material::Materials;
use super::pipeline::Shared;
use super::shaped::{self, Shaped};
use super::skybox::Skybox;
use super::stars::Stars;
use super::sun::Bodies;
use super::translucent::Translucent;
use super::{highlight, lighting, shadow, Renderer, World};
use crate::camera::Camera;

impl Renderer {
    /// Rebuilds every scene-derived piece of GPU state from `world`, at the
    /// quality level already set, keeping everything [`Renderer::new`] would
    /// have built identically — see the module doc for which is which. Mirrors
    /// `new` step for step, so the two draw the same picture from the same
    /// world: anything added to one belongs in the other.
    ///
    /// The camera is re-framed on the new bounds exactly as `new` frames it
    /// (with the projection mode kept); the selection outline and the gizmo
    /// survive too, though the caller re-applies its own `View` regardless.
    pub(crate) fn rebuild(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, world: World<'_>) {
        let World {
            scene,
            decor,
            lighting,
            lights,
            images,
            fonts,
        } = world;
        let quality = self.quality;

        self.lighting = *lighting;
        self.post.set_effects(lighting.effects);
        // The same cap `new` applies. A different count of allowed lights is a
        // different buffer size, which is the one thing a write cannot change.
        let allowed = lights.len().min(quality.local_lights_max);
        if allowed == self.lights {
            lighting::local_lights_write(queue, &self.lights_buffer, &lights[..allowed]);
        } else {
            self.lights_buffer = lighting::local_lights_buffer(device, &lights[..allowed]);
            self.light_shadows_buffer = shadow::local::buffer(device, allowed);
            self.lights = allowed;
        }
        self.all_lights = lights.to_vec();

        if !self.env.holds(decor.sky.as_deref()) {
            self.env = EnvMap::new(device, queue, decor.sky.as_deref(), &quality);
        }
        // Never dropped, only added to: a kind the new scene stopped using
        // costs two small buffers, and the next edit may well bring it back.
        for kind in shaped::kinds(scene.parts()) {
            self.meshes.ensure(device, kind);
        }
        if !self.materials.holds(scene.materials()) {
            self.materials = Materials::new(
                device,
                queue,
                &self.material_layout,
                scene.materials(),
                &quality,
            );
        }
        self.shadows.rebuild(device, scene);

        // Every pass with a camera bind group of its own is built (or rebound
        // below) against the buffers and views as they stand from here on.
        let shared = Shared {
            lighting: &self.lighting_buffer,
            lights: &self.lights_buffer,
            env: &self.env,
            shadow_map: self.shadows.view(),
            shadow_sampler: self.shadows.sampler(),
            local_shadow_map: self.shadows.local_view(),
            light_shadows: &self.light_shadows_buffer,
        };
        self.sky = match (self.sky.take(), decor.sky.as_deref()) {
            (Some(sky), Some(panels)) if sky.holds(panels) => Some(sky),
            (Some(mut sky), Some(panels)) => {
                sky.replace(device, queue, panels, &quality);
                Some(sky)
            }
            (None, Some(panels)) => Some(Skybox::new(
                device,
                queue,
                self.target,
                &self.frame_layout,
                shared,
                panels,
                &quality,
            )),
            (_, None) => None,
        };
        self.bodies = match (self.bodies.take(), &decor.bodies[..]) {
            (Some(bodies), discs) if bodies.holds(discs) => Some(bodies),
            (Some(mut bodies), discs) if !discs.is_empty() => {
                bodies.replace(device, queue, discs, &quality);
                Some(bodies)
            }
            (_, discs) => Bodies::new(
                device,
                queue,
                self.target,
                &self.frame_layout,
                shared,
                discs,
                &quality,
            ),
        };
        self.stars = match (self.stars.take(), &decor.stars[..]) {
            (Some(stars), field) if stars.holds(field) => Some(stars),
            (Some(mut stars), field) if !field.is_empty() => {
                stars.replace(device, field);
                Some(stars)
            }
            (_, field) => Stars::new(device, self.target, &self.frame_layout, shared, field),
        };

        // `all_placements`, not `placements`: the selection outline draws a
        // mesh part (a `MeshPart`, or a union carved into one) by its own
        // bounding box, so it needs the suppressed box placements that the
        // filtered `placements` drops — the same map `Renderer::new` seeds it
        // with. Without this a full rebuild (an asset streaming in, a beam or
        // particle resolving) stripped every mesh part's outline and gizmo.
        self.selection.rebuild(device, scene.all_placements());
        self.shaped = Shaped::new(device, scene.parts());
        self.translucent = Translucent::new(device, scene.parts());
        self.filemesh
            .rebuild(device, queue, scene.resolved_file_meshes());
        self.textured
            .rebuild(device, queue, &decor.groups, &quality);
        self.beams
            .rebuild(device, queue, scene.beams(), images, &quality);
        self.trails
            .rebuild(device, queue, scene.trails(), images, &quality);
        self.particles
            .rebuild(device, queue, scene.particle_emitters(), images, &quality);
        // After `shaped`/`filemesh` above only for readability — the mask's
        // own batches are built from the scene, not from theirs.
        self.highlights.replace(
            device,
            queue,
            &self.frame_layout,
            highlight::Source {
                highlights: scene.highlights(),
                parts: scene.parts(),
                resolved: scene.resolved_file_meshes(),
            },
        );
        self.gui.rebuild(
            device,
            queue,
            (scene.gui_screens(), scene.gui_spaces()),
            &self.materials.bind_group,
            images,
            fonts,
            &quality,
        );

        self.camera =
            Camera::framing(scene.bounds()).with_orthographic(self.camera.is_orthographic());
        self.bounds = *scene.bounds();
        // Last: the probe, the light buffers or the shadow views above may
        // have been replaced, and every kept camera bind group still names
        // the old ones.
        self.rebind_frames(device);
    }
}

/// Takes the first entry of `spare` keyed `key` that `fits`, for a rebuild
/// handing a previous scene's uploads to the batches of the next: each entry
/// is given out at most once, so two batches that would each have uploaded
/// the same thing still get one upload each rather than sharing a handle
/// neither owns.
pub(super) fn take_spare<K: PartialEq, G>(
    spare: &mut Vec<(K, G)>,
    key: &K,
    fits: impl Fn(&G) -> bool,
) -> Option<G> {
    let position = spare
        .iter()
        .position(|(known, payload)| known == key && fits(payload))?;
    Some(spare.swap_remove(position).1)
}

/// Whichever of `wanted` a pass never tried to download, in first-seen order
/// and without duplicates — `tried` holding every reference it did, whether
/// the download succeeded or not. A failed asset stays failed for the life
/// of the renderer rather than being fetched again on every edit.
pub(super) fn untried<V>(
    tried: &HashMap<AssetRef, V>,
    wanted: impl IntoIterator<Item = AssetRef>,
) -> Vec<AssetRef> {
    let mut seen = HashSet::new();
    wanted
        .into_iter()
        .filter(|reference| !tried.contains_key(reference) && seen.insert(reference.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spare_is_given_out_once_and_only_under_its_own_key() {
        let mut spare = vec![("a", 1), ("b", 2), ("a", 3)];

        assert_eq!(take_spare(&mut spare, &"a", |_| true), Some(1));
        assert_eq!(take_spare(&mut spare, &"a", |_| true), Some(3));
        assert_eq!(take_spare(&mut spare, &"a", |_| true), None);
        assert_eq!(take_spare(&mut spare, &"c", |_| true), None);
        assert_eq!(spare, vec![("b", 2)]);
    }

    // A file mesh's geometry is keyed by its batch but converted for a skin,
    // and the skin's index can move between two scenes: an entry under the
    // right key in the wrong format has to be passed over, not handed out.
    #[test]
    fn a_spare_that_does_not_fit_is_passed_over() {
        let mut spare = vec![("a", 1), ("a", 2)];

        assert_eq!(
            take_spare(&mut spare, &"a", |payload| *payload == 2),
            Some(2)
        );
        assert_eq!(take_spare(&mut spare, &"a", |payload| *payload == 2), None);
        assert_eq!(spare, vec![("a", 1)]);
    }

    #[test]
    fn only_never_tried_references_are_fetched_again() {
        let tried: HashMap<AssetRef, Option<usize>> =
            HashMap::from([(AssetRef::Id(1), Some(0)), (AssetRef::Id(2), None)]);
        let wanted = [
            AssetRef::Id(1),
            AssetRef::Id(2),
            AssetRef::Id(3),
            AssetRef::Id(3),
        ];

        // 1 uploaded, 2 tried and failed: neither is worth another download.
        assert_eq!(untried(&tried, wanted), vec![AssetRef::Id(3)]);
    }

    #[test]
    fn a_first_build_fetches_everything_once() {
        let tried: HashMap<AssetRef, usize> = HashMap::new();
        let wanted = [AssetRef::Id(5), AssetRef::Id(4), AssetRef::Id(5)];

        assert_eq!(
            untried(&tried, wanted),
            vec![AssetRef::Id(5), AssetRef::Id(4)]
        );
    }
}
