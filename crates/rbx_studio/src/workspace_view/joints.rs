//! The auto-rigger's joint markers: a dot on each fitted joint, drawn over
//! the rig while the Animation Editor's Auto-rig section is open, that a
//! left-button drag picks up and slides across the screen at the joint's
//! own depth. The view only knows where the dots are; `Shell` moves the
//! joint and cuts the body again (see `ViewportAction::Joint`).

use crate::tokens;
use glam::{Vec2, Vec3, Vec4};
use gpui_kit::{Context, Pixels, Point};
use rbx_viewer::pick::{self, Ray};
use rbx_viewer::Segment;

use super::gizmo::Drag;
use super::{ViewportAction, WorkspaceView};

/// The line layer the markers are drawn on (see `changes`' other layers).
const JOINT_MARKERS: usize = 5;
/// How close to a dot a press must land to grab it, in logical pixels.
const GRAB_RADIUS: f32 = 10.0;
/// The markers' orange as linear RGBA. A theme can change it, so the active
/// palette is read on each call rather than fixed at build time.
fn marker_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_joint())
}

/// The joint being edited, in the theme's selected-joint colour. Read from
/// the active palette on each call, as [`marker_color`] is.
fn selected_marker_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_joint_selected())
}

/// Which part of a marker drag a [`ViewportAction::Joint`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JointPhase {
    /// The marker was grabbed: select its joint.
    Press,
    /// The marker moved: carry the joint (and its mirror) along.
    Drag,
    /// The marker was let go: cut the body for where the joint now is.
    Release,
}

/// The dots for `markers`, the `selected` one larger and yellow.
pub(super) fn segments(markers: &[Vec3], selected: Option<usize>) -> Vec<Segment> {
    markers
        .iter()
        .enumerate()
        .map(|(i, &at)| {
            let picked = selected == Some(i);
            Segment {
                from: at,
                to: at,
                color: if picked {
                    selected_marker_color()
                } else {
                    marker_color()
                },
                on_top: true,
                width: if picked { 13.0 } else { 10.0 },
            }
        })
        .collect()
}

/// Where `ray` crosses the plane through `origin` facing `normal`.
pub(super) fn on_plane(ray: Ray, origin: Vec3, normal: Vec3) -> Option<Vec3> {
    let facing = ray.direction.dot(normal);
    if facing.abs() < 1e-6 {
        return None;
    }
    let t = (origin - ray.origin).dot(normal) / facing;
    Some(ray.origin + ray.direction * t)
}

/// The marker nearest `cursor` (physical pixels in a `extent`-sized panel)
/// within `radius`, as drawn through `view_projection`.
pub(super) fn nearest(
    markers: &[Vec3],
    view_projection: glam::Mat4,
    extent: Vec2,
    cursor: Vec2,
    radius: f32,
) -> Option<usize> {
    markers
        .iter()
        .enumerate()
        .filter_map(|(i, &at)| {
            let clip = view_projection * Vec4::new(at.x, at.y, at.z, 1.0);
            if clip.w <= 1e-6 {
                return None;
            }
            let ndc = clip.truncate() / clip.w;
            let pixel = Vec2::new(
                (ndc.x + 1.0) / 2.0 * extent.x,
                (1.0 - ndc.y) / 2.0 * extent.y,
            );
            let distance = pixel.distance(cursor);
            (distance <= radius).then_some((i, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

impl WorkspaceView {
    /// Shows the auto-rigger's joint markers at `markers` (world space,
    /// indexed as `rbx_autorig::Joint::ALL`), or takes them down when empty.
    /// A marker drag in progress keeps its own copy of where it started.
    pub(crate) fn set_joint_markers(&mut self, markers: Vec<Vec3>, selected: Option<usize>) {
        if self.joint_markers == (markers.clone(), selected) {
            return;
        }
        self.pump.lines(JOINT_MARKERS, segments(&markers, selected));
        self.joint_markers = (markers, selected);
    }

    /// The marker under a press at `position`, as the drag that carries it:
    /// across the plane facing the camera through the joint.
    pub(super) fn grab_joint(&self, position: Point<Pixels>, scale: f32) -> Option<Drag> {
        let (markers, _) = &self.joint_markers;
        if markers.is_empty() {
            return None;
        }
        let pose = self.view?;
        let viewport = self.viewport.get();
        if viewport.size.0 == 0 || viewport.size.1 == 0 {
            return None;
        }
        let cursor = Vec2::new(
            f32::from(position.x) * scale - viewport.origin.0 as f32,
            f32::from(position.y) * scale - viewport.origin.1 as f32,
        );
        let extent = Vec2::new(viewport.size.0 as f32, viewport.size.1 as f32);
        let projection = pose.view_projection(self.orthographic, extent.x / extent.y);
        let index = nearest(markers, projection, extent, cursor, GRAB_RADIUS * scale)?;
        let normal = pick::ray_through(projection, Vec2::ZERO)
            .direction
            .normalize_or_zero();
        let origin = markers[index];
        Some(Drag::Joint {
            index,
            origin,
            normal,
            to: origin,
        })
    }

    /// One step of a marker drag: the joint goes where the cursor's ray
    /// crosses the drag's plane.
    pub(super) fn joint_step(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(Drag::Joint {
            index,
            origin,
            normal,
            ..
        }) = self.drag
        else {
            return;
        };
        let Some(to) = self
            .cursor_ray(position, scale)
            .and_then(|ray| on_plane(ray, origin, normal))
        else {
            return;
        };
        self.drag = Some(Drag::Joint {
            index,
            origin,
            normal,
            to,
        });
        self.dragged = true;
        cx.emit(ViewportAction::Joint {
            index,
            to,
            phase: JointPhase::Drag,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_is_grabbed_only_near_its_dot() {
        // Looking down −Z from z = 10 at two markers.
        let view =
            glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y);
        let projection = glam::camera::rh::proj::directx::perspective(1.0, 1.0, 0.1, 100.0) * view;
        let extent = Vec2::splat(400.0);
        let markers = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)];
        assert_eq!(
            nearest(&markers, projection, extent, Vec2::splat(200.0), 10.0),
            Some(0)
        );
        assert_eq!(
            nearest(&markers, projection, extent, Vec2::new(200.0, 150.0), 10.0),
            None
        );
        // The second marker is to the right of the centre.
        let right = projection * Vec4::new(1.0, 0.0, 0.0, 1.0);
        let x = (right.x / right.w + 1.0) / 2.0 * 400.0;
        assert_eq!(
            nearest(
                &markers,
                projection,
                extent,
                Vec2::new(x + 3.0, 200.0),
                10.0
            ),
            Some(1)
        );
        // Behind the camera: never.
        let behind = [Vec3::new(0.0, 0.0, 20.0)];
        assert_eq!(
            nearest(&behind, projection, extent, Vec2::splat(200.0), 400.0),
            None
        );
    }

    #[test]
    fn a_drag_stays_at_the_joints_depth() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.1, 0.2, -1.0));
        let to = on_plane(ray, Vec3::new(3.0, 1.0, 2.0), Vec3::NEG_Z).unwrap();
        assert!((to.z - 2.0).abs() < 1e-5);
        assert!(on_plane(ray, Vec3::ZERO, Vec3::X.cross(ray.direction).normalize()).is_none());
        assert_eq!(
            segments(&[Vec3::ZERO, Vec3::ONE], Some(1))[1].color,
            selected_marker_color()
        );
    }
}
