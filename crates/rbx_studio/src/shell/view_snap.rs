//! Snapping the camera square-on: a click on the orientation indicator's
//! cube, or one of the View menu's fixed views; and Focus (F), which frames
//! the selection.

use glam::Vec3;
use gpui_kit::Context;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::pick::{self, PartSurface, Ray};

use crate::workspace_view::snapped_forward;

use super::Shell;

/// How far ahead the camera turns around when nothing is under the centre
/// of the view.
const EMPTY_FOCUS: f32 = 50.0;

/// The View menu's fixed views: where the camera looks from, named the way
/// Roblox names a part's faces (`Enum.NormalId`), so Front looks at a
/// part's Front face (`-Z`) from in front of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresetView {
    Top,
    Front,
    Right,
}

impl PresetView {
    /// The way the camera looks in this view.
    fn forward(self) -> Vec3 {
        match self {
            PresetView::Top => Vec3::NEG_Y,
            PresetView::Front => Vec3::Z,
            PresetView::Right => Vec3::NEG_X,
        }
    }
}

/// The eye and target that turn the camera to look along `forward`, about
/// the point `distance` ahead of it.
fn orbit(eye: Vec3, look: Vec3, distance: f32, forward: Vec3) -> (Vec3, Vec3) {
    let focus = eye + look * distance;
    (focus - forward * distance, focus)
}

impl Shell {
    /// Turns the camera to look along `forward` (straight up or down tilts
    /// a degree short, see `snapped_forward`), around the surface under the
    /// centre of the view, so what was in the middle stays there.
    pub(super) fn snap_view(&mut self, forward: Vec3, cx: &mut Context<Self>) {
        let view = self.viewport.read(cx);
        let Some(pose) = view.pose() else {
            return;
        };
        let forward = snapped_forward(pose, forward);
        let look = pose.basis().0;
        let ray = Ray::new(pose.position, look);
        let meshes = view.meshes().clone();
        let distance = pick::parts_along(&self.dom, &self.database, &meshes, ray)
            .into_iter()
            .find_map(|part| {
                PartSurface::read(&self.dom, &self.database, &meshes, part)?.raycast(ray)
            })
            .map_or(EMPTY_FOCUS, |(distance, _)| distance);
        let (eye, target) = orbit(pose.position, look, distance, forward);
        self.viewport
            .read(cx)
            .look_at(eye.to_array(), target.to_array());
    }

    /// Focus: frames everything selected, a model as the box round all its
    /// parts, keeping the way the camera looks, as Studio's F does.
    pub(super) fn focus_selection(&mut self, cx: &mut Context<Self>) {
        let Some((low, high)) = parts_box(&self.dom, self.selected_all()) else {
            return;
        };
        let view = self.viewport.read(cx);
        let Some(pose) = view.pose() else {
            return;
        };
        let centre = (low + high) * 0.5;
        let reach = ((high - low).length() * 0.5).max(1.0);
        // Far enough that the box's bounding sphere fits the view's height.
        let distance = reach / (pose.fov_degrees.to_radians() * 0.5).sin();
        let eye = centre - pose.basis().0 * distance;
        view.look_at(eye.to_array(), centre.to_array());
    }

    /// The View menu's Top, Front and Right.
    pub(crate) fn preset_view(&mut self, view: PresetView, cx: &mut Context<Self>) {
        self.snap_view(view.forward(), cx);
    }
}

/// The world box round every part under (and including) `roots`, each
/// part's own box turned by its rotation; `None` when there is no part.
pub(super) fn parts_box(dom: &WeakDom, roots: &[Ref]) -> Option<(Vec3, Vec3)> {
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut pending = roots.to_vec();
    while let Some(at) = pending.pop() {
        let Some(instance) = dom.get(at) else {
            continue;
        };
        pending.extend_from_slice(instance.children());
        let properties = instance.properties();
        let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
            continue;
        };
        let size = match properties.get("size") {
            Some(Variant::Vector3(size)) => Vec3::new(size.x, size.y, size.z),
            _ => Vec3::ONE,
        };
        let r = cframe.rotation;
        let row = |i: usize| Vec3::new(r[i], r[i + 1], r[i + 2]).abs().dot(size * 0.5);
        let half = Vec3::new(row(0), row(3), row(6));
        let centre = Vec3::new(cframe.position.x, cframe.position.y, cframe.position.z);
        low = low.min(centre - half);
        high = high.max(centre + half);
    }
    (low.x <= high.x).then_some((low, high))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snap_keeps_the_focus_point_and_its_distance() {
        let eye = Vec3::new(0.0, 10.0, 20.0);
        let look = Vec3::new(0.0, -0.6, -0.8);
        let (new_eye, target) = orbit(eye, look, 25.0, Vec3::NEG_Y);
        assert!((target - Vec3::new(0.0, -5.0, 0.0)).length() < 1e-4);
        assert!((new_eye - Vec3::new(0.0, 20.0, 0.0)).length() < 1e-4);
    }

    #[test]
    fn front_looks_at_a_parts_front_face() {
        // A part's Front face points -Z, so the camera looking at it looks +Z.
        assert_eq!(PresetView::Front.forward(), Vec3::Z);
        assert_eq!(PresetView::Right.forward(), Vec3::NEG_X);
        assert_eq!(PresetView::Top.forward(), Vec3::NEG_Y);
    }

    #[test]
    fn the_focus_box_holds_every_part_turned_by_its_rotation() {
        use rbx_dom::{CFrameData, Vector3Data};
        let v = |x, y, z| Vector3Data { x, y, z };
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        let flat = dom.new_instance("Part", "Flat", Some(model));
        let _ = dom.set_property(
            flat,
            "CFrame",
            Variant::CFrame(CFrameData {
                position: v(0.0, 0.0, 0.0),
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        );
        let _ = dom.set_property(flat, "size", Variant::Vector3(v(4.0, 2.0, 2.0)));
        // A quarter turn about Y: its 4 studs now run along Z.
        let turned = dom.new_instance("Part", "Turned", Some(model));
        let _ = dom.set_property(
            turned,
            "CFrame",
            Variant::CFrame(CFrameData {
                position: v(10.0, 0.0, 0.0),
                rotation: [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0],
            }),
        );
        let _ = dom.set_property(turned, "size", Variant::Vector3(v(4.0, 2.0, 2.0)));
        let (low, high) = parts_box(&dom, &[model]).unwrap();
        assert!((low - Vec3::new(-2.0, -1.0, -2.0)).length() < 1e-4, "{low}");
        assert!((high - Vec3::new(11.0, 1.0, 2.0)).length() < 1e-4, "{high}");
        assert!(parts_box(&dom, &[]).is_none());
    }
}
