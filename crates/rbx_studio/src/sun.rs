//! The Sun tool: placing the sun, or the moon, by pointing at the scene
//! instead of typing `ClockTime` and `GeographicLatitude`.
//!
//! Four gestures, each a left-button press and drag in the 3D view — see
//! [`Mode`]. This module is their geometry and the writes they make, pure so
//! it can be tested without a window. The view turns the cursor into a ray
//! and draws the guide (`workspace_view::sun`); `Shell` owns the DOM and the
//! gesture's one undo step (`shell::sun`); the sky model itself, both ways
//! round, is the renderer's own (`rbx_viewer::sun`).

use std::collections::BTreeMap;

use glam::{Mat4, Quat, Vec3, Vec4};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::pick::Ray;
use rbx_viewer::sun::{Body, Placement};

use crate::settle::Surface;

const LIGHTING_CLASS: &str = "Lighting";
const CLOCK_TIME: &str = "ClockTime";
const TIME_OF_DAY: &str = "TimeOfDay";
const LATITUDE: &str = "GeographicLatitude";

const SECONDS_PER_DAY: i64 = 24 * 60 * 60;
/// How far the guide line reaches toward the body, in dragger arms: sized on
/// screen like the gizmo, so it reads the same near and far.
const GUIDE_ARMS: f32 = 3.0;
/// The cube marking where a shadow lands, in dragger arms across.
const MARKER_ARMS: f32 = 0.15;

/// What a press and drag in the 3D view aims the body by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The body goes where the cursor points: drag it across the sky.
    #[default]
    Sky,
    /// The body shines straight onto the surface under the cursor.
    Face,
    /// Press on an object, then drag to where its shadow should fall.
    Shadow,
    /// The body goes wherever its reflection off the surface under the
    /// cursor reaches the camera.
    Glint,
}

impl Mode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Mode::Sky => "Sky",
            Mode::Face => "Face",
            Mode::Shadow => "Shadow",
            Mode::Glint => "Glint",
        }
    }

    /// The gesture in one sentence, for the mode's tooltip: nothing on the
    /// ribbon row itself says what to do with the mouse.
    pub(crate) fn hint(self) -> &'static str {
        match self {
            Mode::Sky => "drag in the view, and the light follows the cursor",
            Mode::Face => "press on a surface, and the light shines straight onto it",
            Mode::Shadow => "press on an object, then drag to where its shadow should fall",
            Mode::Glint => "press on a surface, and the light glints off it into the camera",
        }
    }
}

/// What the Sun tool is set to, and the gesture under way.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct SunTool {
    pub(crate) mode: Mode,
    pub(crate) body: Body,
    gesture: Gesture,
}

/// What a press found, held for the rest of its drag.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Gesture {
    /// Shadow's caster: the part pressed on, and the point on it whose
    /// shadow the drag places.
    caster: Option<(Ref, Vec3)>,
    /// Whether this gesture has written `Lighting` yet: its first real write
    /// is what opens its one undo step.
    wrote: bool,
}

/// Where one step points the body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Aim {
    /// Unit vector toward the body, before the sky has had its say about
    /// whether it can go there (see `rbx_viewer::sun::place`).
    pub(crate) toward: Vec3,
    /// Where the guide starts and, in Shadow, where the shadow lands. `None`
    /// in Sky, whose line would run straight down the cursor ray and draw as
    /// a dot under the pointer.
    anchor: Option<(Vec3, Option<Vec3>)>,
}

/// What the view draws for one step: a line from `from` toward the body,
/// starting back at `target` when there is one so the whole shadow ray
/// shows, and a marker on `target`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Guide {
    pub(crate) from: Vec3,
    pub(crate) toward: Vec3,
    pub(crate) target: Option<Vec3>,
}

impl SunTool {
    /// The left button going down: a new gesture. Shadow takes its caster
    /// here — the one point the rest of the drag measures from.
    ///
    /// `hit` is the drawn surface under a ray, leaving out one part if asked
    /// (see `shell::sun`, which answers it through `pick::surface_hit`).
    pub(crate) fn press(
        &mut self,
        ray: Ray,
        hit: impl Fn(Ray, Option<Ref>) -> Option<(Ref, Surface)>,
    ) {
        let caster = match self.mode {
            Mode::Shadow => hit(ray, None).map(|(part, surface)| (part, surface.point)),
            Mode::Sky | Mode::Face | Mode::Glint => None,
        };
        self.gesture = Gesture {
            caster,
            wrote: false,
        };
    }

    /// Where this step of the gesture points the body, or `None` where it
    /// points nowhere: over empty sky in a mode that needs a surface, and
    /// Shadow's own press, which only picks the caster up — its ground would
    /// be whatever stands behind the caster, and the light would jump before
    /// the drag had said anything.
    pub(crate) fn aim(
        &self,
        ray: Ray,
        first: bool,
        hit: impl Fn(Ray, Option<Ref>) -> Option<(Ref, Surface)>,
    ) -> Option<Aim> {
        match self.mode {
            Mode::Sky => Some(Aim {
                toward: ray.direction,
                anchor: None,
            }),
            Mode::Face => {
                let (_, surface) = hit(ray, None)?;
                Some(Aim {
                    toward: surface.normal,
                    anchor: Some((surface.point, None)),
                })
            }
            Mode::Glint => {
                let (_, surface) = hit(ray, None)?;
                Some(Aim {
                    // The camera is back along the ray, perspective or not.
                    toward: glint(surface.normal, -ray.direction),
                    anchor: Some((surface.point, None)),
                })
            }
            Mode::Shadow if first => None,
            Mode::Shadow => {
                let (caster, point) = self.gesture.caster?;
                let (_, ground) = hit(ray, Some(caster))?;
                Some(Aim {
                    toward: shadow(point, ground.point)?,
                    anchor: Some((point, Some(ground.point))),
                })
            }
        }
    }

    /// Whether a write is the gesture's first — the one that opens its undo
    /// step. Asked only of a step that really changes something, so a
    /// gesture that never did stays out of the history altogether.
    pub(crate) fn opens_step(&mut self) -> bool {
        !std::mem::replace(&mut self.gesture.wrote, true)
    }
}

impl Aim {
    /// The guide for this aim, along where the body really went — which is
    /// not where it was aimed when the sky could not reach that, and the
    /// line swinging away from the cursor is what says so.
    pub(crate) fn guide(self, placement: &Placement) -> Option<Guide> {
        let (from, target) = self.anchor?;
        Some(Guide {
            from,
            toward: placement.direction,
            target,
        })
    }
}

impl Guide {
    /// The guide as the renderer's preview boxes (`Headless::set_preview`),
    /// sized in dragger arms of `arm` studs. A box collapsed onto one edge
    /// *is* that edge, which is how the line reaches the one overlay pass
    /// there is rather than needing a second.
    pub(crate) fn boxes(self, arm: f32) -> Vec<Mat4> {
        let start = self.target.unwrap_or(self.from);
        let end = self.from + self.toward * arm * GUIDE_ARMS;
        let line = Mat4::from_cols(
            (end - start).extend(0.0),
            Vec4::ZERO,
            Vec4::ZERO,
            ((start + end) * 0.5).extend(1.0),
        );
        let marker = self.target.map(|target| {
            Mat4::from_scale_rotation_translation(
                Vec3::splat(arm * MARKER_ARMS),
                Quat::IDENTITY,
                target,
            )
        });
        std::iter::once(line).chain(marker).collect()
    }
}

/// Shadow: the light stands behind the caster, as seen from where its
/// shadow falls. `None` when the two points are one.
fn shadow(caster: Vec3, ground: Vec3) -> Option<Vec3> {
    (caster - ground).try_normalize()
}

/// Glint: the direction whose mirror reflection off a surface with this
/// `normal` leaves along `view`, `2(N·V)N − V`.
fn glint(normal: Vec3, view: Vec3) -> Vec3 {
    2.0 * normal.dot(view) * normal - view
}

/// The place's `Lighting` service, or `None` for a file without one (a bare
/// model), where the tool has nothing to write and is offered greyed out.
pub(crate) fn lighting(dom: &WeakDom) -> Option<Ref> {
    dom.root_refs().iter().copied().find(|&referent| {
        dom.get(referent)
            .is_some_and(|it| it.class() == LIGHTING_CLASS)
    })
}

/// What one step writes into `Lighting`, less anything already holding that
/// value: an empty list is a step that changed nothing, which must not open
/// an undo step.
///
/// `TimeOfDay` is always written. It is the clock a place file carries —
/// `ClockTime`'s `CanSave` is false in the API dump — and the one the
/// Properties panel edits. `ClockTime` is written only where the DOM already
/// has one (a script set it, say): the renderer reads it ahead of
/// `TimeOfDay` (`rbx_viewer`'s `lighting::clock_of`), so leaving it stale
/// would leave the sky where it was.
pub(crate) fn writes(
    properties: &BTreeMap<String, Variant>,
    placement: &Placement,
) -> Vec<(&'static str, Variant)> {
    let mut writes = vec![
        (
            LATITUDE,
            number_like(properties.get(LATITUDE), placement.geographic_latitude),
        ),
        (
            TIME_OF_DAY,
            Variant::String(time_of_day(placement.clock_time)),
        ),
    ];
    if let Some(current) = properties.get(CLOCK_TIME) {
        writes.push((CLOCK_TIME, number_like(Some(current), placement.clock_time)));
    }
    writes.retain(|(name, value)| properties.get(*name) != Some(value));
    writes
}

/// `value` in whichever float width the property already holds, so a write
/// never changes a property's type under it.
fn number_like(current: Option<&Variant>, value: f32) -> Variant {
    match current {
        Some(Variant::Float64(_)) => Variant::Float64(f64::from(value)),
        _ => Variant::Float32(value),
    }
}

/// `TimeOfDay`'s own spelling of a clock time, `"HH:MM:SS"`, wrapped into
/// one day.
fn time_of_day(clock: f32) -> String {
    let seconds = ((clock * 3600.0).round() as i64).rem_euclid(SECONDS_PER_DAY);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// What the drag readout says: which body, the time and the latitude that
/// put it there, and — when the sky could not reach where it was aimed —
/// that the latitude has run out.
pub(crate) fn readout(body: Body, placement: &Placement) -> String {
    let minutes = ((placement.clock_time * 60.0).round() as i64).rem_euclid(SECONDS_PER_DAY / 60);
    let name = match body {
        Body::Sun => "Sun",
        Body::Moon => "Moon",
    };
    let limit = if placement.clamped { " (limit)" } else { "" };
    format!(
        "{name} {:02}:{:02} · lat {:.1}°{limit}",
        minutes / 60,
        minutes % 60,
        placement.geographic_latitude
    )
}

#[cfg(test)]
#[path = "sun/tests.rs"]
mod tests;
