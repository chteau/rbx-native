//! A handle drag's guides: the soft-snap search at the press, then the axis
//! line, the dots and Studio's distance label on every step.

use std::time::Instant;

use glam::{Mat4, Vec2, Vec3};
use gpui_kit::*;
use rbx_viewer::gizmo::{self, Axis};
use rbx_viewer::pick::Ray;

use super::super::gizmo::Drag;
use super::super::WorkspaceView;
use crate::dragger::label::{self, Arrow};
use crate::dragger::sweep::{self, Slab};
use crate::dragger::{handle_scale, Guides, MAX_SOFT_SNAPS};

impl WorkspaceView {
    /// A handle press: the soft-snap search Studio runs once at the press
    /// (see `dragger::sweep`), over every part but the selection.
    pub(in crate::workspace_view) fn grab_guides(&mut self, drag: Drag, ray: Ray) {
        let Some(pose) = self.view else {
            return;
        };
        let started = Instant::now();
        let (slab, offsets) = match drag {
            Drag::Axis { .. } => {
                let Some(handles) = self.handles() else {
                    return;
                };
                let Some((axis, along)) = handles.grab(ray).and_then(|axis| {
                    let along = gizmo::along_axis(handles.origin(), handles.direction(axis), ray)?;
                    Some((axis, along))
                }) else {
                    return;
                };
                self.guides.arrow = Some((axis, if along < 0.0 { -1.0 } else { 1.0 }, along.abs()));
                let basis = Axis::ALL.map(|axis| handles.direction(axis));
                let models: Vec<Mat4> = self.targets.iter().map(|target| target.model).collect();
                let slab = selection_slab(handles.origin(), basis, axis as usize, &models);
                let offsets = sweep::offsets(&slab, &models);
                (slab, offsets)
            }
            Drag::Size { .. } | Drag::Box { .. } => {
                let Some((faces, (axis, sign))) = self
                    .faces()
                    .and_then(|faces| Some((faces, faces.grab(ray)?)))
                else {
                    return;
                };
                let mut basis = Axis::ALL.map(|axis| faces.direction(axis));
                basis[axis as usize] *= sign;
                let size = Vec3::from(Axis::ALL.map(|axis| faces.extent(axis)));
                // Studio casts from the moving face itself, so the only
                // offset is the face's own.
                (
                    Slab::new(faces.handle(axis, sign), basis, axis as usize, size),
                    vec![0.0],
                )
            }
            Drag::Plane { .. } | Drag::Ring { .. } => return,
        };
        self.guides.snaps = sweep::soft_snaps(
            &slab,
            &offsets,
            &self.neighbours,
            MAX_SOFT_SNAPS,
            started,
            pose,
            self.orthographic,
        );
        if std::env::var_os("RBX_STUDIO_STATS").is_some() {
            eprintln!(
                "rbxstudio: soft-snap search over {} parts: {} snaps in {:?}",
                self.neighbours.len(),
                self.guides.snaps.len(),
                started.elapsed()
            );
        }
    }

    /// A handle drag's guides after a step: the axis line, the soft-snap
    /// dots and the distance label.
    pub(in crate::workspace_view) fn step_guides(
        &mut self,
        drag: Drag,
        ray: Ray,
        shift: bool,
        scale: f32,
    ) {
        let Some(pose) = self.view else {
            return;
        };
        let (origin, axis, grabbed) = match drag {
            Drag::Axis {
                origin,
                axis,
                grabbed,
            }
            | Drag::Size {
                origin,
                axis,
                grabbed,
                ..
            }
            | Drag::Box {
                origin,
                axis,
                grabbed,
                ..
            } => (origin, axis, grabbed),
            Drag::Plane { .. } | Drag::Ring { .. } => return,
        };
        let grid = self.transform.translate.grid(shift);
        let snaps = self.soft_snaps(shift);
        let current = gizmo::along_axis(origin, axis, ray)
            .and_then(|along| sweep::choose(snaps, along - grabbed, grid));
        let mut drawn = Guides {
            lines: Vec::new(),
            dots: sweep::dots(snaps, current, pose, self.orthographic),
        };
        let measure = self.guides.settings.show_measurement;

        let label = match drag {
            Drag::Axis { .. } => {
                let (Some(handles), Some((held, sign, along))) =
                    (self.handles(), self.guides.arrow)
                else {
                    return;
                };
                let direction = axis * sign;
                drawn
                    .lines
                    .extend(sweep::axis_line(handles.origin(), direction, handles.arm()));
                let travelled = self.travelled(direction);
                let orthographic = self.orthographic;
                drawn.lines.extend(label::tail(
                    handles.origin(),
                    direction,
                    travelled,
                    held.color(),
                    |at| handle_scale(at, pose, orthographic),
                ));
                measure
                    .then(|| self.move_label(&handles, held, sign, along, scale))
                    .flatten()
                    .map(|at| (at, label::concise(travelled)))
            }
            Drag::Size { component, .. } => {
                let anchor = self.targets.anchor();
                if let Some(anchor) = anchor {
                    drawn
                        .lines
                        .push(sweep::extrude_line(anchor.position(), axis));
                }
                anchor.filter(|_| measure).and_then(|anchor| {
                    let at = self.project(anchor.position(), scale)?;
                    Some((at, label::concise(anchor.size()[component])))
                })
            }
            _ => {
                let scaled = self.targets.scale_box();
                if let Some(scaled) = scaled {
                    drawn
                        .lines
                        .push(sweep::extrude_line(scaled.w_axis.truncate(), axis));
                }
                scaled.filter(|_| measure).and_then(|scaled| {
                    let centre = scaled.w_axis.truncate();
                    let extent = (0..3)
                        .map(|column| scaled.col(column).truncate().dot(axis).abs())
                        .sum::<f32>();
                    Some((self.project(centre, scale)?, label::concise(extent)))
                })
            }
        };
        self.show_guides(drawn);
        self.guides.label = label.map(|(at, text)| (at, SharedString::from(text)));
    }

    /// Where the Move label goes: beside the dragged arrow, leaning towards
    /// whichever other arrow looks most square to it on screen.
    pub(in crate::workspace_view) fn move_label(
        &self,
        handles: &gizmo::Handles,
        held: Axis,
        sign: f32,
        along: f32,
        scale: f32,
    ) -> Option<Point<Pixels>> {
        let base = handles.origin();
        let arrow = |direction: Vec3| -> Option<Arrow> {
            Some(Arrow {
                direction,
                base: self.screen(base)?,
                tip: self.screen(base + direction * handles.arm())?,
            })
        };
        let direction = handles.direction(held) * sign;
        let others: Vec<Arrow> = Axis::ALL
            .into_iter()
            .filter(|&axis| axis != held)
            .flat_map(|axis| [1.0, -1.0].map(|sign| handles.direction(axis) * sign))
            .filter_map(arrow)
            .collect();
        let viewport = self.viewport.get();
        let middle = Vec2::new(viewport.size.0 as f32, viewport.size.1 as f32) * 0.5;
        let lagging = label::lagging(arrow(direction)?, &others, middle)?;
        let arm = handles.arm();
        let pose = self.view?;
        // Sized where the handles stood at the grab, as Studio sizes it.
        let start = self.held.centre().unwrap_or(base);
        let offset = label::LABEL_OFFSET * handle_scale(start, pose, self.orthographic);
        let at = label::move_label(
            base,
            direction,
            (gizmo::SHAFT_START * arm, arm),
            along,
            lagging.direction,
            offset,
        );
        self.project(at, scale)
    }

    /// `point` on the frame, in the frame's own physical pixels — off the
    /// frame too, for measuring directions. `None` behind the camera.
    fn screen(&self, point: Vec3) -> Option<Vec2> {
        let pose = self.view?;
        let viewport = self.viewport.get();
        let size = Vec2::new(viewport.size.0 as f32, viewport.size.1 as f32);
        if size.min_element() <= 0.0 {
            return None;
        }
        let clip = pose.view_projection(self.orthographic, size.x / size.y) * point.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = Vec2::new(clip.x, clip.y) / clip.w;
        Some(Vec2::new(
            (ndc.x + 1.0) * 0.5 * size.x,
            (1.0 - ndc.y) * 0.5 * size.y,
        ))
    }

    /// `point` in the panel's own logical pixels, where an absolutely placed
    /// child goes; `None` off screen, where Studio hides its label.
    fn project(&self, point: Vec3, scale: f32) -> Option<Point<Pixels>> {
        let pixel = self.screen(point)?;
        let viewport = self.viewport.get();
        let size = Vec2::new(viewport.size.0 as f32, viewport.size.1 as f32);
        (pixel.cmpge(Vec2::ZERO).all() && pixel.cmple(size).all())
            .then(|| Point::new(px(pixel.x / scale), px(pixel.y / scale)))
    }
}

/// The slab a Move drag of the boxes `models` along `basis[axis]` sweeps:
/// their cross-section in `basis`, centred on it. Only the position along
/// the axis is `origin`'s, where the drag is measured from; across it the
/// slab sits where the boxes are, which is not `origin` when the handles
/// stand at the middle of a world-aligned box and `basis` is turned.
fn selection_slab(origin: Vec3, basis: [Vec3; 3], axis: usize, models: &[Mat4]) -> Slab {
    let spans = basis.map(|direction| span_along(direction, models));
    let mut centre = origin;
    for (index, direction) in basis.into_iter().enumerate() {
        if index != axis {
            let (low, high) = spans[index];
            centre += direction * ((low + high) * 0.5 - origin.dot(direction));
        }
    }
    let size = Vec3::from(spans.map(|(low, high)| (high - low).max(0.0)));
    Slab::new(centre, basis, axis, size)
}

/// Where the boxes `models` start and end along the unit `direction`.
fn span_along(direction: Vec3, models: &[Mat4]) -> (f32, f32) {
    models
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), model| {
            let at = model.w_axis.truncate().dot(direction);
            let reach = 0.5
                * (0..3)
                    .map(|column| model.col(column).truncate().dot(direction).abs())
                    .sum::<f32>();
            (low.min(at - reach), high.max(at + reach))
        })
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Vec3};

    use super::selection_slab;
    use crate::dragger::sweep;

    fn cube(at: Vec3) -> Mat4 {
        Mat4::from_translation(at)
    }

    #[test]
    fn the_slab_is_the_selections_cross_section_in_the_basis_given() {
        let a =
            Mat4::from_translation(Vec3::new(-2.0, 0.0, 0.0)) * Mat4::from_scale(Vec3::splat(2.0));
        let b = Mat4::from_translation(Vec3::new(3.0, 1.0, 0.0)) * Mat4::from_scale(Vec3::ONE);
        let slab = selection_slab(
            Vec3::new(0.25, 0.75, 0.0),
            [Vec3::X, Vec3::Y, Vec3::Z],
            0,
            &[a, b],
        );
        // Across X: Y spans -1…1.5 and Z -1…1, each widened by a tenth.
        assert!((slab.half[0] - (1.25 + 0.1)).abs() < 1e-5 && (slab.half[1] - 1.1).abs() < 1e-5);
        assert!(
            (slab.origin - Vec3::new(0.25, 0.25, 0.0)).length() < 1e-5,
            "{}",
            slab.origin
        );
    }

    // Three parts whose world box's middle is not their middle along a
    // turned basis: the slab has to cover all three, not start past the
    // first one.
    #[test]
    fn a_turned_basis_centres_the_slab_on_the_parts_not_the_world_box() {
        let parts = [
            cube(Vec3::ZERO),
            cube(Vec3::new(10.0, 0.0, 0.0)),
            cube(Vec3::new(0.0, 0.0, 10.0)),
        ];
        let turn = glam::Mat3::from_rotation_y(std::f32::consts::FRAC_PI_4);
        let basis = [turn.x_axis, turn.y_axis, turn.z_axis];
        let world_middle = Vec3::new(5.0, 0.0, 5.0);
        let slab = selection_slab(world_middle, basis, 0, &parts);

        let root = std::f32::consts::FRAC_1_SQRT_2;
        let along_z = slab.origin.dot(basis[2]);
        assert!((along_z - (10.0 * root) * 0.5).abs() < 1e-4, "{along_z}");
        // The first part is inside the slab, not beside it.
        assert_eq!(sweep::offsets(&slab, &parts[..1]).len(), 3);
        // Along X' the parts run from part 3's far side to part 2's.
        let offsets = sweep::offsets(&slab, &parts);
        let x = |at: Vec3| (at - slab.origin).dot(basis[0]);
        let leading = x(Vec3::new(10.0, 0.0, 0.0)) + root;
        let trailing = x(Vec3::new(0.0, 0.0, 10.0)) - root;
        assert!(
            offsets.iter().any(|offset| (offset + leading).abs() < 1e-4),
            "{offsets:?}"
        );
        assert!(
            offsets
                .iter()
                .any(|offset| (offset + trailing).abs() < 1e-4),
            "{offsets:?}"
        );
    }
}
