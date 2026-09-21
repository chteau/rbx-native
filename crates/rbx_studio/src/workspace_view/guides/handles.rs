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
                let slab = Slab::new(
                    handles.origin(),
                    basis,
                    axis as usize,
                    extent_in(basis, &models),
                );
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
                let travelled = self
                    .targets
                    .anchor()
                    .zip(self.held.anchor())
                    .map_or(0.0, |(now, then)| {
                        (now.position() - then.position()).dot(direction)
                    });
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
        self.guides.drawn = drawn;
        self.guides.label = label.map(|(at, text)| (at, SharedString::from(text)));
    }

    /// Where the Move label goes: beside the dragged arrow, leaning towards
    /// whichever other arrow looks most square to it on screen.
    fn move_label(
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

/// The extent of the boxes `models` on each of `basis`'s three axes.
fn extent_in(basis: [Vec3; 3], models: &[Mat4]) -> Vec3 {
    Vec3::from(basis.map(|axis| {
        let (low, high) =
            models
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), model| {
                    let at = model.w_axis.truncate().dot(axis);
                    let reach = 0.5
                        * (0..3)
                            .map(|column| model.col(column).truncate().dot(axis).abs())
                            .sum::<f32>();
                    (low.min(at - reach), high.max(at + reach))
                });
        (high - low).max(0.0)
    }))
}

/// Studio's floating measurement box, legacy dark theme: a white bold number
/// on RGB(37, 37, 37) with a black border and small rounded corners
/// (`FloatingValueInput`), padded 4/4/2/4 and centred on its anchor.
pub(in crate::workspace_view) fn label_element(
    at: Point<Pixels>,
    text: SharedString,
) -> impl IntoElement {
    // A box wider and taller than any label, centred on the anchor, with the
    // label centred inside it: GPUI places a child by its corner, not its
    // middle.
    const SLOT: f32 = 200.0;
    div()
        .absolute()
        .left(at.x - px(SLOT / 2.0))
        .top(at.y - px(SLOT / 2.0))
        .size(px(SLOT))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .pl(px(4.0))
                .pr(px(4.0))
                .pt(px(2.0))
                .pb(px(4.0))
                .bg(rgb(0x252525))
                .border_1()
                .border_color(rgb(0x000000))
                .rounded(px(3.0))
                .text_color(rgb(0xffffff))
                .text_size(px(24.0 * crate::tokens::font_scale()))
                .line_height(px(24.0 * crate::tokens::font_scale()))
                .font_weight(FontWeight::BOLD)
                .child(text),
        )
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Vec3};

    use super::extent_in;

    #[test]
    fn the_extent_is_measured_along_the_basis_given() {
        let a =
            Mat4::from_translation(Vec3::new(-2.0, 0.0, 0.0)) * Mat4::from_scale(Vec3::splat(2.0));
        let b = Mat4::from_translation(Vec3::new(3.0, 1.0, 0.0)) * Mat4::from_scale(Vec3::ONE);
        let extent = extent_in([Vec3::X, Vec3::Y, Vec3::Z], &[a, b]);
        assert!(
            (extent - Vec3::new(6.5, 2.5, 2.0)).length() < 1e-5,
            "{extent}"
        );
        let turned = [
            Vec3::new(1.0, 0.0, 1.0).normalize(),
            Vec3::Y,
            Vec3::new(-1.0, 0.0, 1.0).normalize(),
        ];
        let extent = extent_in(turned, &[Mat4::from_scale(Vec3::splat(2.0))]);
        assert!((extent.x - 2.0 * std::f32::consts::SQRT_2).abs() < 1e-5);
    }
}
