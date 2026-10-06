//! A `ViewportFrame`'s view and contents as the UI Editor's frame sheet
//! edits them: which `Camera` it looks through, the pose that camera holds,
//! framing the contents, what in `Workspace` can be cloned in, and how a
//! camera flight folds into undo steps.
//!
//! No GPUI here — `shell::ui_editor::frame_sheet` drives this and writes
//! through the editor's own history. Nothing is cached either: every read
//! is of the DOM as it stands.

use glam::{Mat4, Vec3};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::{pick, Pose};

use crate::camera::{cframe_from_pose, free_flight_direction, look_vector};

pub(crate) const FRAME_CLASS: &str = "ViewportFrame";
const CAMERA_CLASS: &str = "Camera";

/// `Camera.FieldOfView`'s own default, in degrees.
const DEFAULT_FOV_DEGREES: f32 = 70.0;

/// Where a fit with no view to keep looks from: a three-quarter view from
/// the front-right, a little above — the angle an item thumbnail is
/// usually shot at.
pub(crate) const THREE_QUARTER: (f32, f32) = (std::f32::consts::FRAC_PI_4, 0.35);

/// The `Camera` `frame` looks through: `CurrentCamera` where it names one,
/// otherwise a `Camera` already under the frame. The second matters on
/// every reopened place: `CurrentCamera` does not save (the API dump marks
/// it `DoesNotSerialize`), while the `Camera` under the frame does — so a
/// reloaded frame has its camera back but no pointer to it, and reusing it
/// beats leaving a stray second one behind on the first flight.
pub(crate) fn camera_of(dom: &WeakDom, database: &ReflectionDatabase, frame: Ref) -> Option<Ref> {
    let is_camera = |referent: Ref| {
        dom.get(referent)
            .is_some_and(|instance| database.is_subclass_of(instance.class(), CAMERA_CLASS))
    };
    let instance = dom.get(frame)?;
    if let Some(&Variant::Ref(current)) = instance.properties().get("CurrentCamera") {
        if is_camera(current) {
            return Some(current);
        }
    }
    instance
        .children()
        .iter()
        .copied()
        .find(|&child| is_camera(child))
}

/// The pose the frame is seen from: its camera's `CFrame` and
/// `FieldOfView`, else the `CameraCFrame`/`CameraFieldOfView` a saved place
/// carries on the frame itself (the latter in radians). `None` for a frame
/// with neither — nothing to fly from until a fit picks a view.
pub(crate) fn pose_of(dom: &WeakDom, database: &ReflectionDatabase, frame: Ref) -> Option<Pose> {
    let (cframe, fov_degrees) = match camera_of(dom, database, frame) {
        Some(camera) => {
            let properties = dom.get(camera)?.properties();
            let fov = match properties.get("FieldOfView") {
                Some(&Variant::Float32(fov)) => fov,
                _ => DEFAULT_FOV_DEGREES,
            };
            (properties.get("CFrame")?, fov)
        }
        None => {
            let properties = dom.get(frame)?.properties();
            let fov = match properties.get("CameraFieldOfView") {
                Some(&Variant::Float32(fov)) => fov.to_degrees(),
                _ => DEFAULT_FOV_DEGREES,
            };
            (properties.get("CameraCFrame")?, fov)
        }
    };
    let Variant::CFrame(cframe) = cframe else {
        return None;
    };
    let [x, y, z] = look_vector(&cframe.rotation)?;
    // `free_flight_direction` run backwards: the look vector is
    // `(-sin yaw cos pitch, -sin pitch, -cos yaw cos pitch)`.
    let fov_degrees = match fov_degrees.is_finite() && fov_degrees > 0.0 {
        true => fov_degrees,
        false => DEFAULT_FOV_DEGREES,
    };
    Some(Pose {
        position: Vec3::new(cframe.position.x, cframe.position.y, cframe.position.z),
        yaw: (-x).atan2(-z),
        pitch: (-y).clamp(-1.0, 1.0).asin(),
        fov_degrees,
        ortho_scale: 1.0,
    })
}

/// Puts `pose` on the frame, creating the `Camera` under it and pointing
/// `CurrentCamera` at it first if it has none. The pose is written twice:
/// on the camera, which is what a script reading the frame finds, and on
/// the frame's hidden `CameraCFrame`/`CameraFieldOfView`, which is what a
/// saved place keeps — "when you set this property, `Camera.CFrame` and
/// `Camera.FieldOfView` will be saved and replicate with the
/// `ViewportFrame` internally" (`ViewportFrame.yaml`). Returns the camera.
pub(crate) fn write_pose(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    frame: Ref,
    pose: Pose,
) -> Ref {
    let camera = camera_of(dom, database, frame)
        .unwrap_or_else(|| dom.new_instance(CAMERA_CLASS, CAMERA_CLASS, Some(frame)));
    let current = dom
        .get(frame)
        .and_then(|instance| instance.properties().get("CurrentCamera").cloned());
    if current != Some(Variant::Ref(camera)) {
        let _ = dom.set_property(frame, "CurrentCamera", Variant::Ref(camera));
    }
    let cframe = Variant::CFrame(cframe_from_pose(pose));
    let _ = dom.set_property(camera, "CFrame", cframe.clone());
    let _ = dom.set_property(camera, "FieldOfView", Variant::Float32(pose.fov_degrees));
    let _ = dom.set_property(frame, "CameraCFrame", cframe);
    let _ = dom.set_property(
        frame,
        "CameraFieldOfView",
        Variant::Float32(pose.fov_degrees.to_radians()),
    );
    camera
}

/// The smallest sphere about the centre of every drawn part's box under
/// `frame` that holds them all, as `(centre, radius)`. `None` for a frame
/// with nothing in it.
pub(crate) fn contents(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    frame: Ref,
) -> Option<(Vec3, f32)> {
    let corners: Vec<Vec3> = pick::parts_of(dom, database, frame)
        .filter_map(|part| pick::model_of(dom, part))
        .flat_map(box_corners)
        .collect();
    let (min, max) = corners.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(min, max), &corner| (min.min(corner), max.max(corner)),
    );
    let centre = (min + max) * 0.5;
    let radius = corners
        .iter()
        .map(|corner| corner.distance(centre))
        .fold(None, |far: Option<f32>, distance| {
            Some(far.map_or(distance, |far| far.max(distance)))
        })?;
    Some((centre, radius))
}

fn box_corners(model: Mat4) -> [Vec3; 8] {
    std::array::from_fn(|index| {
        let sign = |bit: usize| match index >> bit & 1 {
            0 => -0.5,
            _ => 0.5,
        };
        model.transform_point3(Vec3::new(sign(0), sign(1), sign(2)))
    })
}

/// The pose looking along `yaw`/`pitch` that just holds a sphere of
/// `radius` about `centre` in a frame of `aspect` (width over height): the
/// sphere touches the narrower of the two half-angles, so it is whole on
/// screen whichever way the frame is shaped.
pub(crate) fn fit(
    (centre, radius): (Vec3, f32),
    (yaw, pitch): (f32, f32),
    fov_degrees: f32,
    aspect: f32,
) -> Pose {
    let vertical = fov_degrees.to_radians() * 0.5;
    let horizontal = (vertical.tan() * aspect.max(f32::EPSILON)).atan();
    let half = vertical.min(horizontal);
    let distance = radius.max(0.01) / half.sin();
    let forward = Vec3::from(free_flight_direction(yaw, pitch));
    Pose {
        position: centre - forward * distance,
        yaw,
        pitch,
        fov_degrees,
        ortho_scale: radius,
    }
}

/// One row of the sheet's Workspace picker.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Insertable {
    pub(crate) referent: Ref,
    /// How deep under `Workspace` it sits: the indent it is listed at.
    pub(crate) depth: usize,
    pub(crate) name: String,
    pub(crate) class: String,
    /// The names of what it sits in, below `Workspace`, outermost first —
    /// what tells two same-named parts apart in a filtered list.
    pub(crate) path: Vec<String>,
    pub(crate) model: bool,
}

/// Every `Model` and `BasePart` in `Workspace` — what a clone into a frame
/// draws — in tree order. Models are looked into, parts are not: a part's
/// children are not things a frame can show on their own.
pub(crate) fn insertable(dom: &WeakDom, database: &ReflectionDatabase) -> Vec<Insertable> {
    let Some(workspace) = dom
        .root_refs()
        .iter()
        .copied()
        .find(|&root| dom.get(root).is_some_and(|i| i.class() == "Workspace"))
    else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut stack: Vec<(Ref, Vec<String>)> = children_of(dom, workspace, &[]);
    while let Some((referent, path)) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        let class = instance.class();
        let part = database.is_subclass_of(class, "BasePart") && class != "Terrain";
        let model = database.is_subclass_of(class, "Model");
        if part || model {
            rows.push(Insertable {
                referent,
                depth: path.len(),
                name: instance.name().to_owned(),
                class: class.to_owned(),
                path: path.clone(),
                model,
            });
        }
        if !part && class != CAMERA_CLASS {
            let mut inner = path;
            inner.push(instance.name().to_owned());
            stack.extend(children_of(dom, referent, &inner));
        }
    }
    rows
}

/// Where `query` sits in `name`, case-insensitively, as a byte range of
/// `name` — what a filtered row lights up. `None` where it does not.
pub(crate) fn matched(name: &str, query: &str) -> Option<std::ops::Range<usize>> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let lower = name.to_lowercase();
    // Lowercasing can change a string's length outside ASCII; a name
    // that does is matched, but not lit, rather than lit at the wrong place.
    let start = lower.find(&query.to_lowercase())?;
    match lower.len() == name.len() {
        true => Some(start..start + query.len()),
        false => Some(0..0),
    }
}

/// Whether `row` is already in `frame`: a direct child of the same name and
/// class. A clone keeps no link to what it was cloned from, so this is the
/// nearest the sheet can tell — and the one that matters, since a second
/// copy of the same thing is rarely what a click meant.
// ponytail: name+class match; tag clones with their source only if a false
// tick ever bites.
pub(crate) fn already_in(dom: &WeakDom, frame: Ref, row: &Insertable) -> bool {
    dom.get(frame).is_some_and(|instance| {
        instance.children().iter().any(|&child| {
            dom.get(child)
                .is_some_and(|child| child.name() == row.name && child.class() == row.class)
        })
    })
}

/// `referent`'s children, each with `path` — reversed so a stack pops them
/// in tree order.
fn children_of(dom: &WeakDom, referent: Ref, path: &[String]) -> Vec<(Ref, Vec<String>)> {
    dom.get(referent)
        .map(|instance| {
            instance
                .children()
                .iter()
                .rev()
                .map(|&child| (child, path.to_vec()))
                .collect()
        })
        .unwrap_or_default()
}

/// What a `ViewportFrame` holds that its Contents list shows: its models
/// and parts, in order — not its `Camera`, nor anything else under it.
pub(crate) fn contents_of(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    frame: Ref,
) -> Vec<(Ref, String, String, bool)> {
    let Some(instance) = dom.get(frame) else {
        return Vec::new();
    };
    instance
        .children()
        .iter()
        .filter_map(|&child| {
            let instance = dom.get(child)?;
            let class = instance.class();
            let model = database.is_subclass_of(class, "Model");
            (model || database.is_subclass_of(class, "BasePart"))
                .then(|| (child, instance.name().to_owned(), class.to_owned(), model))
        })
        .collect()
}

/// Steepest the orbit tool tips the view, short of looking straight down
/// its own axis — the free camera's own limit.
const MAX_PITCH: f32 = 89.0 * std::f32::consts::PI / 180.0;

/// `pose` turned about `pivot` by `yaw`/`pitch` radians, keeping its
/// distance: the Orbit tool's drag.
pub(crate) fn orbit(pose: Pose, pivot: Vec3, yaw: f32, pitch: f32) -> Pose {
    let distance = pose.position.distance(pivot);
    let yaw = pose.yaw + yaw;
    let pitch = (pose.pitch + pitch).clamp(-MAX_PITCH, MAX_PITCH);
    let forward = Vec3::from(free_flight_direction(yaw, pitch));
    Pose {
        position: pivot - forward * distance,
        yaw,
        pitch,
        ..pose
    }
}

/// `pose` slid sideways and up by `right`/`up` studs in its own view plane:
/// the Hand tool's drag. The view's own right and up, built around world
/// up the way `crate::camera::cframe_from_pose` builds them.
pub(crate) fn pan(pose: Pose, right: f32, up: f32) -> Pose {
    let forward = Vec3::from(free_flight_direction(pose.yaw, pose.pitch));
    let side = forward.cross(Vec3::Y).normalize_or(Vec3::X);
    let lift = side.cross(forward);
    Pose {
        position: pose.position + side * right + lift * up,
        ..pose
    }
}

/// How a camera flight lands in the history: one undo step per gesture, not
/// one per frame. A gesture opens with the first step that moves the pose
/// and closes once nothing steers any more — every button and key up and
/// every eased move settled — so a flight that coasts to a stop after the
/// keys are let go is still that one step.
#[derive(Debug, Default)]
pub(crate) struct Gesture {
    open: bool,
}

/// What one step of a flight writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The gesture's first move: a new undo step.
    First,
    /// A later move, folded into the step the gesture opened.
    Continue,
}

impl Gesture {
    /// `moved`: this step changed the pose. `busy`: something still steers
    /// after it (`rbx_viewer::Flight::busy`).
    pub(crate) fn step(&mut self, moved: bool, busy: bool) -> Option<Step> {
        let step = moved.then(|| match std::mem::replace(&mut self.open, true) {
            false => Step::First,
            true => Step::Continue,
        });
        if !busy {
            self.open = false;
        }
        step
    }

    /// Ends the gesture in flight, so the next move opens a new step — for
    /// an edit made in between (a fit, an undo) that the flight must not
    /// fold itself into.
    pub(crate) fn close(&mut self) {
        self.open = false;
    }
}

#[cfg(test)]
#[path = "viewport_frame/tests.rs"]
mod tests;
