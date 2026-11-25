//! The viewpoint a place file was saved at, as Studio restores it on open:
//! `Workspace.CurrentCamera`, the `CFrame` of the `Camera` it points at, and
//! that same instance's `FieldOfView`. [`write_pose`] is the other direction,
//! mirroring the render thread's live free-flight pose back into the same two
//! properties, the way Studio's own Explorer/Properties stay live as the
//! camera flies, so this module owns both the read Studio opens with and the
//! write Ctrl+S later picks back up.

use rbx_dom::{CFrameData, Change, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_viewer::{OrthoAxis, Pose};

/// Roblox's own default when a `Camera` names no `FieldOfView` at all, in degrees.
const DEFAULT_FOV_DEGREES: f32 = 70.0;
/// A new `Camera`'s `OrthographicSize` in Studio 0.748, in studs.
const DEFAULT_ORTHO_SIZE: f32 = 35.0;
/// Studio's "Orthographic Camera" beta: the edit camera follows the place
/// camera's `ProjectionType`, `OrthographicSize` and `OrthographicMode`.
pub(crate) const ORTHOGRAPHIC_BETA_ID: &str = "orthographic-camera";

/// Where a place says its view opens: the eye, a point it looks toward, both in
/// studs, and the field of view to open it at, exactly what
/// [`rbx_viewer::Headless::open_at`] takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlaceCamera {
    pub(crate) eye: [f32; 3],
    pub(crate) look_at: [f32; 3],
    pub(crate) fov_degrees: f32,
    /// `Some((OrthographicSize, OrthographicMode))` when the camera's
    /// `ProjectionType` is Orthographic.
    pub(crate) ortho: Option<(f32, OrthoAxis)>,
}

impl PlaceCamera {
    /// `None` for a file that names no camera, names one that has since gone,
    /// names something that is not a `Camera`, or whose `CFrame` aims nowhere.
    /// Callers then have to frame the place themselves.
    pub(crate) fn from_dom(dom: &WeakDom) -> Option<Self> {
        let camera = dom.get(current_camera(dom)?)?;
        // A `Ref` property can point at any instance; only a Camera has the
        // pose meaning we need, whatever the property is called.
        if camera.class() != "Camera" {
            return None;
        }

        let Some(Variant::CFrame(cframe)) = camera.properties().get("CFrame") else {
            return None;
        };
        let eye = [cframe.position.x, cframe.position.y, cframe.position.z];
        if !eye.iter().all(|axis| axis.is_finite()) {
            return None;
        }
        let look = look_vector(&cframe.rotation)?;
        // Studio's new camera orbits the camera's `Focus` when the view looks
        // at it: aiming at the point keeps that distance for the orbit.
        let reach = match camera.properties().get("Focus") {
            Some(Variant::CFrame(focus))
                if crate::beta::enabled(crate::workspace_view::camera_row::BETA_ID) =>
            {
                focus_depth(
                    eye,
                    look,
                    [focus.position.x, focus.position.y, focus.position.z],
                )
            }
            _ => 1.0,
        };

        Some(PlaceCamera {
            eye,
            look_at: [
                eye[0] + look[0] * reach,
                eye[1] + look[1] * reach,
                eye[2] + look[2] * reach,
            ],
            fov_degrees: field_of_view(camera),
            ortho: orthographic(camera),
        })
    }
}

/// How far ahead `focus` stands when the view from `eye` along `look` is
/// aimed at it, or 1 when it is off to one side or behind.
fn focus_depth(eye: [f32; 3], look: [f32; 3], focus: [f32; 3]) -> f32 {
    let to = glam::Vec3::from(focus) - glam::Vec3::from(eye);
    let depth = to.dot(glam::Vec3::from(look));
    let aimed = depth > 0.0 && (to - glam::Vec3::from(look) * depth).length() <= depth * 0.01;
    if aimed && depth.is_finite() {
        depth
    } else {
        1.0
    }
}

/// A degenerate `FieldOfView` (absent, non-finite, or non-positive) must never
/// reach the renderer's projection matrix, which would otherwise divide by a
/// tangent of zero or produce a NaN one.
fn field_of_view(camera: &Instance) -> f32 {
    let Some(Variant::Float32(degrees)) = camera.properties().get("FieldOfView") else {
        return DEFAULT_FOV_DEGREES;
    };

    if degrees.is_finite() && *degrees > 0.0 {
        *degrees
    } else {
        DEFAULT_FOV_DEGREES
    }
}

/// [`orthographic`] for `Workspace.CurrentCamera`: the place's in Edit, a
/// player's own in a running game.
pub(crate) fn current_ortho(dom: &WeakDom) -> Option<(f32, OrthoAxis)> {
    orthographic(dom.get(current_camera(dom)?)?)
}

/// The camera's orthographic span, `None` while its `ProjectionType` is
/// Perspective (or missing, Roblox's default).
fn orthographic(camera: &Instance) -> Option<(f32, OrthoAxis)> {
    let properties = camera.properties();
    let Some(Variant::Enum(projection)) = properties.get("ProjectionType") else {
        return None;
    };
    if *projection != 1 {
        return None;
    }
    let size = match properties.get("OrthographicSize") {
        Some(Variant::Float32(size)) if size.is_finite() && *size > 0.0 => *size,
        _ => DEFAULT_ORTHO_SIZE,
    };
    let axis = match properties.get("OrthographicMode") {
        Some(Variant::Enum(mode)) => OrthoAxis::from_value(*mode),
        _ => OrthoAxis::Vertical,
    };
    Some((size, axis))
}

/// Where `changes` moved the view to, when one of them wrote the current
/// camera's `CFrame`, `FieldOfView`, `ProjectionType` or `OrthographicSize`:
/// Studio's edit camera follows `workspace.CurrentCamera.CFrame = ...` from
/// the Command Bar, a plugin or the Properties panel. Not `OrthographicMode`:
/// Studio keeps the picture when the mode changes and rewrites the size,
/// which [`write_ortho_size`] does here on the next pose sync.
pub(crate) fn moved_by(dom: &WeakDom, changes: &[Change]) -> Option<PlaceCamera> {
    let camera = current_camera(dom)?;
    changes
        .iter()
        .any(|change| {
            matches!(change, Change::Property { referent, name }
            if *referent == camera
                && matches!(
                    name.as_str(),
                    "CFrame" | "FieldOfView" | "ProjectionType" | "OrthographicSize"
                ))
        })
        .then(|| PlaceCamera::from_dom(dom))
        .flatten()
}

fn current_camera(dom: &WeakDom) -> Option<Ref> {
    let workspace = workspace_ref(dom)?;
    match dom.get(workspace)?.properties().get("CurrentCamera") {
        Some(Variant::Ref(referent)) => Some(*referent),
        _ => None,
    }
}

/// The file's own `Workspace`, root-level and unique the way every real place
/// has it. `None` for a model file or anything else with no `Workspace` at
/// all, [`write_pose`]'s cue that there is nowhere to attach a camera.
fn workspace_ref(dom: &WeakDom) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .filter_map(|referent| dom.get(*referent))
        .find(|instance| instance.class() == "Workspace")
        .map(Instance::referent)
}

/// `workspace`'s `CurrentCamera`, but only when it actually resolves to a
/// `Camera` instance, the same standard [`PlaceCamera::from_dom`] reads by
/// (a `Ref` pointing at something else, or at nothing, is treated as no
/// camera at all), mirrored here so [`write_pose`] reuses one instead of
/// leaving a stray second `Camera` behind every time it runs.
fn existing_camera(dom: &WeakDom, workspace: Ref) -> Option<Ref> {
    let Some(Variant::Ref(referent)) = dom.get(workspace)?.properties().get("CurrentCamera") else {
        return None;
    };
    let camera = dom.get(*referent)?;
    (camera.class() == "Camera").then_some(*referent)
}

/// Mirrors a free-flight [`Pose`] into `Workspace.CurrentCamera.CFrame` and
/// `FieldOfView` on the live DOM the editor holds, called a few times a
/// second while the user flies the camera (see `workspace_view::PoseSynced`),
/// never through `Shell::push_history`/`History::push`: a mouse-look frame is
/// not an edit, and letting this land on the 50-entry undo stack would blow
/// the whole thing on camera movement alone.
///
/// Creates the `Camera` instance under `Workspace`: exactly as a script's
/// `Instance.new("Camera", workspace)` would, the first time this is called
/// on a place that opened with none; a silent no-op for a file with no
/// `Workspace` at all, since there is nowhere to attach one.
pub(crate) fn write_pose(dom: &mut WeakDom, pose: Pose) {
    let Some(workspace) = workspace_ref(dom) else {
        return;
    };
    let camera = existing_camera(dom, workspace).unwrap_or_else(|| {
        let camera = dom.new_instance("Camera", "Camera", Some(workspace));
        let _ = dom.set_property(workspace, "CurrentCamera", Variant::Ref(camera));
        camera
    });

    // Past the change log: the editor mirroring its own camera is not a
    // write for [`moved_by`] to turn back into a camera move.
    if let Some(instance) = dom.get_mut(camera) {
        let properties = instance.properties_mut();
        properties.insert("CFrame".into(), Variant::CFrame(cframe_from_pose(pose)));
        properties.insert("FieldOfView".into(), Variant::Float32(pose.fov_degrees));
    }
}

/// Mirrors the orthographic view's zoom into the current camera's
/// `OrthographicSize`, along its own `OrthographicMode` on a view `aspect`
/// wide per high. Only while that camera is Orthographic, and past the
/// change log like [`write_pose`].
pub(crate) fn write_ortho_size(dom: &mut WeakDom, half_height: f32, aspect: f32) {
    let Some(referent) = workspace_ref(dom).and_then(|workspace| existing_camera(dom, workspace))
    else {
        return;
    };
    let Some(instance) = dom.get_mut(referent) else {
        return;
    };
    let Some((_, axis)) = orthographic(instance) else {
        return;
    };
    let size = axis.size(half_height, aspect);
    instance
        .properties_mut()
        .insert("OrthographicSize".into(), Variant::Float32(size));
}

/// Builds the `CFrameData` a place would need to reopen at exactly this
/// `Pose`: [`look_vector`]'s write-side counterpart, built from the same
/// yaw/pitch trigonometry `rbx_viewer::camera::direction` uses (mirrored, not
/// imported: that function is private to the other crate) so a pose written
/// here and read back through [`PlaceCamera::from_dom`] reconstructs the same
/// eye position and look direction, see this module's
/// `a_written_pose_round_trips_through_place_camera` test.
pub(crate) fn cframe_from_pose(pose: Pose) -> CFrameData {
    let forward = free_flight_direction(pose.yaw, pose.pitch);
    // World up, never derived from the pose: matches `look_to_mat4(_, _,
    // Vec3::Y)`, which is what the renderer actually draws a free pose with,
    // a roll-free camera has no other "up" to build a right vector from.
    let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
    let up = cross(right, forward);
    let back = [-forward[0], -forward[1], -forward[2]];

    CFrameData {
        position: Vector3Data {
            x: pose.position.x,
            y: pose.position.y,
            z: pose.position.z,
        },
        // Row-major, columns are the basis vectors [right, up, back], the
        // same layout `look_vector` decodes back out of indices 2/5/8.
        rotation: [
            right[0], up[0], back[0], right[1], up[1], back[1], right[2], up[2], back[2],
        ],
    }
}

/// Mirrors `rbx_viewer::camera::direction` (private to that crate, and to
/// `rbx_viewer::controller::look_direction`, its exact copy there): the
/// forward vector a free-flight `yaw`/`pitch` looks along. The two must stay
/// in lockstep, which is what this module's round-trip test against
/// [`look_vector`] guards.
pub(crate) fn free_flight_direction(yaw: f32, pitch: f32) -> [f32; 3] {
    [
        -(yaw.sin() * pitch.cos()),
        -pitch.sin(),
        -(yaw.cos() * pitch.cos()),
    ]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Falls back to world `+X` for a zero-length input rather than dividing by
/// zero. `free_flight_direction`'s pitch never actually reaches world-up
/// exactly, the free controller clamps it to ±89° (see
/// `rbx_viewer::controller::MAX_PITCH_DEGREES`), but a CFrame write must
/// never produce a NaN rotation regardless of what upstream promises.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !length.is_finite() || length < f32::EPSILON {
        return [1.0, 0.0, 0.0];
    }
    v.map(|axis| axis / length)
}

/// Roblox's `LookVector`: a CFrame looks down its own -Z, whose axis is the
/// third column of the row-major 3x3 rotation, hence indices 2, 5 and 8.
pub(crate) fn look_vector(rotation: &[f32; 9]) -> Option<[f32; 3]> {
    let look = [-rotation[2], -rotation[5], -rotation[8]];
    let length = look.iter().map(|axis| axis * axis).sum::<f32>().sqrt();

    // A zeroed or corrupt rotation would put `look_at` on top of the eye, which
    // is not a direction at all: orbit framing is the better answer.
    if !length.is_finite() || length < f32::EPSILON {
        return None;
    }
    Some(look.map(|axis| axis / length))
}

#[cfg(test)]
#[path = "camera/tests.rs"]
mod tests;
