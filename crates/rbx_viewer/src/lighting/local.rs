//! `PointLight`, `SpotLight` and `SurfaceLight` reduced to one shape the shader
//! can loop over: a position, a linear radiance, a reach and a cone.
//!
//! Entry point: [`local_lights`]. Roblox splats these into a voxel light grid,
//! so nothing here matches how the engine computes them — every light is
//! evaluated analytically per fragment instead, with no occlusion at all (a
//! light shines through walls, and `Light.Shadows` is ignored).

use glam::{Mat4, Vec3};
use rbx_dom::{Instance, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{boolean, color, number};
use crate::scene::{cframe_matrix, is_drawable, workspace_descendants};
use crate::textures::NormalId;

const LIGHT_CLASS: &str = "Light";
const SPOT_CLASS: &str = "SpotLight";
const SURFACE_CLASS: &str = "SurfaceLight";
const ATTACHMENT_CLASS: &str = "Attachment";

/// Studio's own defaults, so a light that omits a property lands where the
/// property sheet shows it rather than at zero.
const DEFAULT_BRIGHTNESS: f32 = 1.0;
const DEFAULT_ANGLE_DEGREES: f32 = 90.0;
const DEFAULT_RANGE: f32 = 8.0;
const DEFAULT_SPOT_RANGE: f32 = 16.0;
/// What Studio's `Range` slider stops at for all three classes.
const MAX_RANGE: f32 = 60.0;
const MAX_ANGLE_DEGREES: f32 = 180.0;

/// Radiance of a `Brightness = 1` white light at its falloff start.
///
/// Calibrated by eye rather than read off Roblox, which keeps these in a voxel
/// grid no place file carries: the sun lamp lands on `SUN_BASE * Brightness`
/// (see [`super`]), i.e. 1.35 in a default place, and a Roblox `PointLight` at
/// Brightness 1 is visibly a *local* light — it picks a lantern's surroundings
/// out of the night without reading as a second sun, and at noon it barely
/// shows. Half the sun is what lands both: a grey part four studs from a
/// Range 8 lamp is unmistakably lit at midnight and still plainly sunlit at
/// 14:00.
const RADIANCE_SCALE: f32 = 0.7;

/// Fraction of the cone's half-angle the edge is smoothed over, so a spot does
/// not end on a hard circle.
const CONE_SOFTNESS: f32 = 0.1;

/// `cos_outer` of a light that shines in every direction: below any real
/// cosine, so the cone factor saturates to 1 without a branch in the shader.
const OMNI_COS_OUTER: f32 = -2.0;
const OMNI_COS_INNER: f32 = -1.0;

/// Smallest gap the shader's `smoothstep(cos_outer, cos_inner, …)` can be given
/// without dividing by zero, which an `Angle` of 0 would otherwise do.
const MIN_CONE_GAP: f32 = 1e-3;

/// How many lights reach the GPU. Past this the buffer is capped rather than
/// grown: the shader loops over every light for every fragment, so a place with
/// thousands of them is already hopeless — and the ones nearest the scene centre
/// are the ones a screenshot is looking at.
pub(crate) const MAX_LOCAL_LIGHTS: usize = 1024;

/// One local light, in world space and linear radiance.
///
/// Invariant: `cos_outer < cos_inner`, which is what lets the shader smooth the
/// cone edge with no special case for a point light (see [`OMNI_COS_OUTER`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocalLight {
    pub(crate) position: Vec3,
    /// `Color` linearized, times `Brightness`, times [`RADIANCE_SCALE`].
    pub(crate) color: Vec3,
    /// `Range`: where the falloff reaches zero.
    pub(crate) range: f32,
    /// Studs of full brightness before the falloff starts. Only a
    /// `SurfaceLight` has one, standing in for the extent of its face.
    pub(crate) near: f32,
    /// Unit cone axis, or [`Vec3::ZERO`] for a light that shines everywhere.
    pub(crate) direction: Vec3,
    pub(crate) cos_outer: f32,
    pub(crate) cos_inner: f32,
    /// `Light.Shadows`. A `PointLight` never honours it — casting one needs six
    /// faces, which this renderer does not build (see
    /// `renderer::shadow::local`) — but it is read for every class alike, so a
    /// part that later swaps a `PointLight` for a `SpotLight` needs no property
    /// re-entered.
    pub(crate) shadows: bool,
}

/// Every enabled light in the DOM whose parent is drawable, nearest the scene
/// centre first once there are more than [`MAX_LOCAL_LIGHTS`] of them.
///
/// Walks parts down to their children rather than lights up to their parents:
/// `WeakDom` has no parent lookup, and a light only counts when the part it
/// hangs on is one this renderer draws anyway — which, same as `Scene::from_dom`,
/// means a part actually parented under `Workspace`.
pub(crate) fn local_lights(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    center: Vec3,
) -> Vec<LocalLight> {
    let mut lights: Vec<LocalLight> = workspace_descendants(dom, database)
        .filter(|&referent| is_drawable(dom, database, referent))
        .filter_map(|referent| dom.get(referent))
        .flat_map(|part| on_part(dom, database, part))
        .collect();

    if lights.len() > MAX_LOCAL_LIGHTS {
        eprintln!(
            "rbxview: {} local lights, keeping the {MAX_LOCAL_LIGHTS} nearest the scene centre",
            lights.len()
        );
        lights.sort_by(|a, b| {
            let distance = |light: &LocalLight| light.position.distance_squared(center);
            distance(a).total_cmp(&distance(b))
        });
        lights.truncate(MAX_LOCAL_LIGHTS);
    }
    lights
}

/// The lights on one part: its own children, plus those of its `Attachment`
/// children, whose `CFrame` is relative to the part.
fn on_part(dom: &WeakDom, database: &ReflectionDatabase, part: &Instance) -> Vec<LocalLight> {
    let Some(frame) = frame_of(part) else {
        return Vec::new();
    };
    let size = size_of(part).unwrap_or(Vec3::ZERO);
    let mut lights = Vec::new();

    for &child in part.children() {
        let Some(instance) = dom.get(child) else {
            continue;
        };
        if database.is_subclass_of(instance.class(), ATTACHMENT_CLASS) {
            // An attachment carries no extent of its own, so a `SurfaceLight`
            // on one gets no near field — it is a plain spot at that point.
            let Some(local) = frame_of(instance) else {
                continue;
            };
            let attached = frame * local;
            lights.extend(
                instance
                    .children()
                    .iter()
                    .filter_map(|&grandchild| dom.get(grandchild))
                    .filter_map(|light| read(database, light, attached, Vec3::ZERO)),
            );
            continue;
        }
        lights.extend(read(database, instance, frame, size));
    }
    lights
}

/// One `Light` instance, placed by the frame and extent of the part it hangs on.
fn read(
    database: &ReflectionDatabase,
    instance: &Instance,
    frame: Mat4,
    size: Vec3,
) -> Option<LocalLight> {
    let class = instance.class();
    if !database.is_subclass_of(class, LIGHT_CLASS) {
        return None;
    }
    let properties = instance.properties();
    if !boolean(properties.get("Enabled")).unwrap_or(true) {
        return None;
    }

    let spot = database.is_subclass_of(class, SPOT_CLASS);
    let surface = database.is_subclass_of(class, SURFACE_CLASS);
    let default_range = if spot {
        DEFAULT_SPOT_RANGE
    } else {
        DEFAULT_RANGE
    };
    let range = number(properties.get("Range"))
        .unwrap_or(default_range)
        .clamp(0.0, MAX_RANGE);
    let brightness = number(properties.get("Brightness"))
        .unwrap_or(DEFAULT_BRIGHTNESS)
        .max(0.0);
    let color = color(properties.get("Color")).unwrap_or(Vec3::ONE) * brightness * RADIANCE_SCALE;

    let mut light = LocalLight {
        position: frame.w_axis.truncate(),
        color,
        range,
        near: 0.0,
        direction: Vec3::ZERO,
        cos_outer: OMNI_COS_OUTER,
        cos_inner: OMNI_COS_INNER,
        shadows: boolean(properties.get("Shadows")).unwrap_or(true),
    };
    if !spot && !surface {
        return Some(light);
    }

    // Both cone classes aim along a face of their part, so a missing or
    // unreadable `Face` leaves nothing sensible to point at.
    let face = match properties.get("Face") {
        Some(&Variant::Enum(raw)) => NormalId::from_ordinal(raw)?,
        _ => return None,
    };
    let axis = face.axis();
    light.direction = frame.transform_vector3(axis).normalize_or(Vec3::Y);
    (light.cos_outer, light.cos_inner) = cone(number(properties.get("Angle")));
    if surface {
        // The whole face emits, not the part's centre: the light sits on the
        // face and keeps full brightness across the face's own width, or the
        // near half of its reach on a face wider than that. Without it a
        // ceiling panel reads as one hot spot in the middle of itself.
        let half_depth = 0.5 * size.dot(axis.abs());
        light.position += light.direction * half_depth;
        light.near = face_radius(size, axis).min(0.5 * range);
    }
    Some(light)
}

/// `(cos_outer, cos_inner)` of a cone of this full `Angle` in degrees.
fn cone(angle: Option<f32>) -> (f32, f32) {
    let half = angle
        .unwrap_or(DEFAULT_ANGLE_DEGREES)
        .clamp(0.0, MAX_ANGLE_DEGREES)
        .to_radians()
        * 0.5;
    let cos_inner = (half * (1.0 - CONE_SOFTNESS)).cos();
    (half.cos().min(cos_inner - MIN_CONE_GAP), cos_inner.min(1.0))
}

/// Half the shorter side of the face on `axis`, i.e. how far off a panel one has
/// to stand before it starts looking like a point.
fn face_radius(size: Vec3, axis: Vec3) -> f32 {
    let on_face = size - size * axis.abs();
    let sides = [on_face.x, on_face.y, on_face.z];
    let shorter = sides
        .into_iter()
        .filter(|side| *side > 0.0)
        .fold(f32::INFINITY, f32::min);
    if shorter.is_finite() {
        0.5 * shorter
    } else {
        0.0
    }
}

fn frame_of(instance: &Instance) -> Option<Mat4> {
    match instance.properties().get("CFrame")? {
        Variant::CFrame(cframe) => Some(cframe_matrix(cframe)),
        _ => None,
    }
}

fn size_of(instance: &Instance) -> Option<Vec3> {
    match instance.properties().get("size")? {
        Variant::Vector3(size) => Some(Vec3::new(size.x, size.y, size.z)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "local/tests.rs"]
mod tests;
