//! Reads a `ViewportFrame`: the `Camera` it looks through, the lighting its
//! docs give it — one directional lamp plus an ambient term, nothing else —
//! and every `BasePart` under it, built into the very same `Part`s the
//! `Workspace`'s own become (`scene::build_part`, against the scene's one
//! material catalog) so the renderer can draw them through its ordinary
//! shape pipelines, into a texture of the frame's own pixel size.
//!
//! What ends up in that texture is then an image as far as the GUI is
//! concerned: `ImageColor3`/`ImageTransparency` are read like an
//! `ImageLabel`'s, and the renderer composites the texture exactly as one.

use std::collections::BTreeMap;

use glam::camera::rh::proj::directx::perspective_infinite_reverse;
use glam::{Mat4, Vec3};
use rbx_dom::{Instance, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::props::{alpha, color, float};
use crate::camera::NEAR_PLANE;
use crate::scene::{self, cframe_matrix, Catalog, Part};

const FRAME_CLASS: &str = "ViewportFrame";
const CAMERA_CLASS: &str = "Camera";

/// `Camera.FieldOfView`'s own default, in degrees.
const DEFAULT_FOV_DEGREES: f32 = 70.0;

/// Straight off `ViewportFrame.yaml`: `Ambient` defaults to
/// `Color3.fromRGB(200, 200, 200)`, `LightColor` to `fromRGB(140, 140, 140)`
/// and `LightDirection` to `(-1, -1, -1)`.
const DEFAULT_AMBIENT: [f32; 3] = [200.0 / 255.0; 3];
const DEFAULT_LIGHT_COLOR: [f32; 3] = [140.0 / 255.0; 3];
const DEFAULT_LIGHT_DIRECTION: Vec3 = Vec3::new(-1.0, -1.0, -1.0);

/// The camera a frame renders through, reduced to what a projection needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewCamera {
    /// The camera's world `CFrame`: it looks down its own -Z, with +Y up.
    pub(crate) cframe: Mat4,
    /// Vertical field of view, in degrees.
    pub(crate) fov_degrees: f32,
}

impl ViewCamera {
    /// World to clip space for a target of `aspect` (width over height), on
    /// the same reversed-Z, infinite-far projection the main camera draws
    /// with — so the frame's depth buffer is cleared and compared the same
    /// way (see `crate::camera`).
    pub(crate) fn view_projection(&self, aspect: f32) -> Mat4 {
        // A zero-sized frame would make the projection singular; a square one
        // is at least drawable.
        let aspect = match aspect.is_finite() && aspect > 0.0 {
            true => aspect,
            false => 1.0,
        };
        perspective_infinite_reverse(self.fov_degrees.to_radians(), aspect, NEAR_PLANE)
            * self.cframe.inverse()
    }

    pub(crate) fn eye(&self) -> Vec3 {
        self.cframe.w_axis.truncate()
    }
}

/// One `ViewportFrame`'s 3D content and how it is lit and composited.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Viewport {
    /// `None` where no camera could be resolved (see [`camera`]): the docs
    /// give `CurrentCamera` no default but `nil`, and a frame with nothing to
    /// look through draws nothing but its own background.
    pub(crate) camera: Option<ViewCamera>,
    /// `Ambient`, linear.
    pub(crate) ambient: [f32; 3],
    /// `LightColor`, linear.
    pub(crate) light_color: [f32; 3],
    /// Unit vector pointing *at* the lamp — `L` in `dot(N, L)`, the way
    /// `crate::lighting::Lighting::sun_direction` is held.
    pub(crate) light: Vec3,
    /// `ImageColor3`, linear, and `1 - ImageTransparency`: what the rendered
    /// image is multiplied by on its way into the GUI.
    pub(crate) tint: [f32; 3],
    pub(crate) alpha: f32,
    /// Every `BasePart` under the frame, wherever in its subtree it sits —
    /// nested `GuiObject`s are unsupported by Roblox itself, so nothing under
    /// the frame is anything but 3D content or a `Camera`.
    pub(crate) parts: Vec<Part>,
}

/// `None` for any class but a `ViewportFrame`.
///
/// `materials` is the scene's own catalog: the frame's parts sample the same
/// material arrays as the workspace's, so their layers have to come from the
/// one place those arrays are built from.
pub(in crate::scene::gui) fn read(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    instance: &Instance,
    properties: &BTreeMap<String, Variant>,
    materials: &mut Catalog,
) -> Option<Viewport> {
    if !database.is_subclass_of(instance.class(), FRAME_CLASS) {
        return None;
    }

    let parts = scene::descendants_of(dom, instance.referent())
        .filter(|&referent| scene::is_drawable(dom, database, referent))
        .filter_map(|referent| scene::build_part(dom, database, referent, materials))
        .collect();

    // "The direction of the light source from position (0, 0, 0)": the docs
    // never say which way round, but read as the direction the light *travels*
    // the default `(-1, -1, -1)` lights a part's top and its camera-facing
    // side, and read the other way it would light only the faces the camera
    // cannot see — so the lamp sits opposite the vector.
    let direction = vector3(properties, "LightDirection").unwrap_or(DEFAULT_LIGHT_DIRECTION);
    let light = -direction.normalize_or(DEFAULT_LIGHT_DIRECTION.normalize());

    Some(Viewport {
        camera: camera(dom, database, properties),
        ambient: color(properties, "Ambient", DEFAULT_AMBIENT),
        light_color: color(properties, "LightColor", DEFAULT_LIGHT_COLOR),
        light,
        tint: color(properties, "ImageColor3", [1.0, 1.0, 1.0]),
        alpha: alpha(properties, "ImageTransparency"),
        parts,
    })
}

/// `CurrentCamera` where it points at a live `Camera`, which is what a tree
/// built in code has; otherwise the pose Studio writes onto the frame itself.
/// `CurrentCamera` is `CanSave: false` — "when you set this property,
/// `Camera.CFrame` and `Camera.FieldOfView` will be saved and replicate with
/// the `ViewportFrame` internally" — and a saved place carries those two as
/// the hidden `CameraCFrame` and `CameraFieldOfView` (the latter in radians,
/// unlike the `Camera`'s own property).
fn camera(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    properties: &BTreeMap<String, Variant>,
) -> Option<ViewCamera> {
    if let Some(&Variant::Ref(referent)) = properties.get("CurrentCamera") {
        let live = dom
            .get(referent)
            .filter(|camera| database.is_subclass_of(camera.class(), CAMERA_CLASS));
        if let Some(camera) = live {
            let properties = camera.properties();
            return Some(ViewCamera {
                cframe: match properties.get("CFrame") {
                    Some(Variant::CFrame(cframe)) => cframe_matrix(cframe),
                    _ => Mat4::IDENTITY,
                },
                fov_degrees: float(properties, "FieldOfView", DEFAULT_FOV_DEGREES),
            });
        }
    }

    let Some(Variant::CFrame(cframe)) = properties.get("CameraCFrame") else {
        return None;
    };
    Some(ViewCamera {
        cframe: cframe_matrix(cframe),
        fov_degrees: float(
            properties,
            "CameraFieldOfView",
            DEFAULT_FOV_DEGREES.to_radians(),
        )
        .to_degrees(),
    })
}

fn vector3(properties: &BTreeMap<String, Variant>, name: &str) -> Option<Vec3> {
    match properties.get(name) {
        Some(Variant::Vector3(value)) => Some(Vec3::new(value.x, value.y, value.z)),
        _ => None,
    }
}

/// Every part of every `ViewportFrame` in `node`'s subtree, for the material
/// re-read a scene does once the packs have landed (see
/// `Scene::resolve_materials`) — a frame's parts point at the same layers
/// the workspace's do, and go plastic-then-textured the same way.
pub(in crate::scene::gui) fn each_part(node: &mut super::Node, apply: &mut impl FnMut(&mut Part)) {
    if let Some(viewport) = &mut node.viewport {
        viewport.parts.iter_mut().for_each(&mut *apply);
    }
    for child in &mut node.children {
        each_part(child, apply);
    }
}
