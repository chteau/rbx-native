//! Which `SpotLight`/`SurfaceLight`s cast their own shadow this frame, and the
//! perspective each of their maps is rendered from.
//!
//! One map per light, so the place's lights outnumber the quality level's own
//! cap (see `crate::quality::QualityProfile::local_shadow_lights_max`) far more
//! often than not; [`select`] is what picks which ones get one. A `PointLight`
//! reads as a cone with no axis at all (see `crate::lighting::local`), which is
//! the tell this filters on: it takes six faces rather than one, and
//! `shadow::point` is what casts it.

use bytemuck::{Pod, Zeroable};
use glam::camera::rh::proj::directx::perspective;
use glam::camera::rh::view::look_to_mat4;
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::lighting::LocalLight;

/// Roblox has no near plane of its own for a light; this is close enough that
/// nothing plausible sits inside it, and it costs the depth range almost
/// nothing next to a `Range` of several studs or more.
///
/// Shared with `shadow::point`, so a cone light's map and a point light's
/// six faces are read back through one comparison sampler and one bias.
pub(super) const NEAR_STUDS: f32 = 0.1;

/// Below this the cone axis is vertical and `Vec3::Y` stops being a usable up
/// vector for the light's own view — the same threshold `shadow::fit` uses.
const VERTICAL_LIGHT: f32 = 0.999;

/// A rectilinear projection cannot reach a full hemisphere; this keeps the FOV
/// a few degrees short of it, which is already far wider than Studio's own
/// `Angle` slider goes without being a `PointLight`.
const MAX_FOV_RADIANS: f32 = 170.0 / 180.0 * std::f32::consts::PI;

/// The narrowest a face light's map is made, so an `Angle` of 0 — a prism
/// straight out of the face, which no perspective reaches — still gets an eye
/// a finite distance behind it. A spot's own cone never gets this narrow:
/// `crate::lighting::local::cone` keeps its outer edge a little open.
const MIN_HALF_FOV_RADIANS: f32 = 0.25 / 180.0 * std::f32::consts::PI;

/// One light selected to cast a shadow this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::renderer) struct Selected {
    /// Position in the slice [`select`] was given — the same index the local
    /// light storage buffer uses, and so the index [`pack`] writes this entry's
    /// record at.
    pub(in crate::renderer) index: usize,
    pub(in crate::renderer) view_projection: Mat4,
}

/// Every enabled, cone-shaped, `Shadows = true` light in `lights`, nearest
/// `camera` first and truncated to `cap`.
///
/// Recomputed every frame rather than once: nothing in this viewer moves a
/// light, but the camera does, and it is the `cap` nearest lights *to the
/// camera* that earn a map — the ones a screenshot is actually looking at.
pub(in crate::renderer) fn select(
    lights: &[LocalLight],
    camera: Vec3,
    cap: usize,
) -> Vec<Selected> {
    let mut candidates: Vec<(usize, &LocalLight)> = lights
        .iter()
        .enumerate()
        .filter(|(_, light)| light.shadows && light.direction != Vec3::ZERO)
        .collect();

    candidates.sort_by(|(_, a), (_, b)| {
        let distance = |light: &LocalLight| light.position.distance_squared(camera);
        distance(a).total_cmp(&distance(b))
    });
    candidates.truncate(cap);

    candidates
        .into_iter()
        .map(|(index, light)| Selected {
            index,
            view_projection: view_projection(light),
        })
        .collect()
}

/// The light's own perspective, looking down its cone axis with the cone's
/// own angle.
///
/// A spot's eye is its position. A `SurfaceLight` on a part shines that cone
/// from every point of its face (see `lights.wgsl`'s `local_terms`), so its
/// eye backs off behind the face until the cone from there takes in the
/// whole face — and so, widening at the same angle, the whole frustum the
/// face lights — with the near plane on the face itself, which keeps the
/// light's own part out of its map.
///
/// Standard (not reversed) depth, matching `Shadows::render`'s own convention —
/// see that module's doc comment for why the sun map is not reversed either.
fn view_projection(light: &LocalLight) -> Mat4 {
    let up = if light.direction.y.abs() > VERTICAL_LIGHT {
        Vec3::Z
    } else {
        Vec3::Y
    };

    // `cos_outer` already IS the cosine `local_terms` in lights.wgsl tests
    // fragments against (see `crate::lighting::local::cone`), so recovering
    // the angle from it keeps the map as wide as the light actually reaches,
    // with no second copy of `Angle` to carry from the DOM to here.
    let half = light
        .cos_outer
        .clamp(-1.0, 1.0)
        .acos()
        .clamp(MIN_HALF_FOV_RADIANS, 0.5 * MAX_FOV_RADIANS);
    let extent = light.face_u.length().max(light.face_v);
    let behind = extent / half.tan();
    let view = look_to_mat4(
        light.position - light.direction * behind,
        light.direction,
        up,
    );
    let far = behind + light.range.max(NEAR_STUDS + 0.01);

    perspective(2.0 * half, 1.0, behind + NEAR_STUDS, far) * view
}

/// One [`Selected`] light, as `lights.wgsl`'s `LightShadow` reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(in crate::renderer) struct LightShadowRaw {
    view_projection: [[f32; 4]; 4],
    /// x: the array layer this cone light's map lives in. y: the cube of the
    /// point-light array this light's six faces live in (see
    /// `renderer::shadow::point`). Each is negative for a light that has
    /// none — not selected, `Shadows = false`, or simply the other kind —
    /// which `local_light_visibility` reads as "not shadowed" without
    /// touching either texture array at all.
    layer: [f32; 4],
}

const UNSHADOWED: LightShadowRaw = LightShadowRaw {
    view_projection: [[0.0; 4]; 4],
    layer: [-1.0, -1.0, 0.0, 0.0],
};

/// One record per local light the place has uploaded (`lights.len()`, which the
/// shader's `local_lights` and `light_shadows` arrays must always agree on),
/// filled in at each selected light's own index — a cone light with the layer
/// and matrix it drew into, a point light with the cube it holds — and
/// [`UNSHADOWED`] everywhere else.
pub(in crate::renderer) fn pack(
    lights: usize,
    selected: &[Selected],
    points: &[super::point::Selected],
) -> Vec<LightShadowRaw> {
    let mut packed = vec![UNSHADOWED; lights.max(1)];
    for (layer, light) in selected.iter().enumerate() {
        if let Some(entry) = packed.get_mut(light.index) {
            *entry = LightShadowRaw {
                view_projection: light.view_projection.to_cols_array_2d(),
                layer: [layer as f32, -1.0, 0.0, 0.0],
            };
        }
    }
    for (cube, light) in points.iter().enumerate() {
        if let Some(entry) = packed.get_mut(light.index) {
            // No matrix of its own: the face the fragment lands on decides
            // which of the six the shader projects through.
            *entry = LightShadowRaw {
                view_projection: [[0.0; 4]; 4],
                layer: [-1.0, cube as f32, 0.0, 0.0],
            };
        }
    }
    packed
}

/// The storage buffer `pack` fills, sized once for `lights` local lights and
/// rewritten every frame after (see `Renderer::draw`) — never empty, for the
/// same reason the local light buffer itself never is.
pub(in crate::renderer) fn buffer(device: &wgpu::Device, lights: usize) -> wgpu::Buffer {
    let packed = vec![UNSHADOWED; lights.max(1)];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rbxview local light shadows"),
        contents: bytemuck::cast_slice(&packed),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    })
}

#[cfg(test)]
#[path = "local/tests.rs"]
mod tests;
