//! The Align tool: geometry for Studio's real Model-tab Align tool
//! (`studio/align-tool.md`), not the Move/Scale/Rotate gizmos in
//! `crate::transform`.
//!
//! Moves each selected top-level object's own **Min**/**Center**/**Max**
//! bound on the toggled **X**/**Y**/**Z** axes to match a reference value —
//! in **World** or **Local** space — taken from either the whole selection's
//! collective bounding box or a fixed **Active Object** (the object stays put;
//! everything else moves to meet it).
//!
//! `studio/align-tool.md` is explicit that this "keeps the model intact":
//! aligning a selected `Model` moves every part beneath it by the same
//! offset rather than aligning each part on its own — which is why this
//! operates on [`pick::Selected`] entries (one per top-level selected
//! instance, each carrying every drawable part it covers) rather than on
//! `crate::transform::Targets`' own flattened part list, which has already
//! thrown away which part came from which selected instance.
//!
//! The docs describe World/Local only for a *single* shared orientation
//! (a rotated part next to an axis-aligned one) and never spell out which
//! frame "Local" measures a multi-part `Model` selection by, or a Selection
//! Bounds reference with no one designated object. This implementation picks
//! one axis frame for the whole operation — the Active Object's orientation
//! when relative to it, otherwise the selection's own anchor (its first
//! entry, the same part `Targets::anchor` already treats as "the" object for
//! a local toggle) — so every selected object's bound is measured, and moved,
//! along the *same* local direction rather than each along its own, which is
//! the only reading that makes "Local" comparable across objects that don't
//! share an orientation at all.

use glam::{Mat3, Vec3};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick;

use crate::transform::Target;

/// One of the three axes an alignment can run along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub(crate) const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }

    fn world_direction(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }

    /// This axis's column of `orientation` — already unit length, since
    /// `Target::orientation` divides `Size` back out.
    fn local_direction(self, orientation: Mat3) -> Vec3 {
        match self {
            Axis::X => orientation.x_axis,
            Axis::Y => orientation.y_axis,
            Axis::Z => orientation.z_axis,
        }
    }
}

/// Which of an object's bounds an axis aligns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Min,
    Center,
    Max,
}

impl Mode {
    pub(crate) const ALL: [Mode; 3] = [Mode::Min, Mode::Center, Mode::Max];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Mode::Min => "Min",
            Mode::Center => "Center",
            Mode::Max => "Max",
        }
    }
}

/// Which axes an alignment measures and moves along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Space {
    World,
    Local,
}

impl Space {
    pub(crate) const ALL: [Space; 2] = [Space::World, Space::Local];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Space::World => "World",
            Space::Local => "Local",
        }
    }
}

/// What an alignment's reference value is taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelativeTo {
    SelectionBounds,
    ActiveObject,
}

impl RelativeTo {
    pub(crate) const ALL: [RelativeTo; 2] = [RelativeTo::SelectionBounds, RelativeTo::ActiveObject];

    pub(crate) fn label(self) -> &'static str {
        match self {
            RelativeTo::SelectionBounds => "Selection Bounds",
            RelativeTo::ActiveObject => "Active Object",
        }
    }
}

/// The Align toolbar's current toggles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Options {
    /// Indexed by `Axis as usize`.
    axes: [bool; 3],
    pub(crate) mode: Mode,
    pub(crate) space: Space,
    pub(crate) relative_to: RelativeTo,
}

impl Default for Options {
    /// Studio's docs publish no default toggle state; a single enabled axis
    /// (rather than none, which would align nothing at all) is this editor's
    /// own starting point, the same way `Transform::default`'s own
    /// undocumented defaults are picked in `crate::transform`.
    fn default() -> Self {
        Options {
            axes: [true, false, false],
            mode: Mode::Center,
            space: Space::World,
            relative_to: RelativeTo::SelectionBounds,
        }
    }
}

impl Options {
    pub(crate) fn axis_enabled(&self, axis: Axis) -> bool {
        self.axes[axis as usize]
    }

    pub(crate) fn toggle_axis(&mut self, axis: Axis) {
        self.axes[axis as usize] = !self.axes[axis as usize];
    }

    /// Replaces every axis toggle at once — what a debug spec applies (see
    /// `shell::align::ALIGN_VARIABLE`), which names the exact set of axes a
    /// screenshot needs rather than toggling from whatever the toolbar's
    /// current state happens to be.
    pub(crate) fn set_axes(&mut self, axes: [bool; 3]) {
        self.axes = axes;
    }
}

/// Every drawable part one top-level selected instance covers (see
/// `pick::Selected::parts`), read as [`Target`]s — one entry per referent in
/// `referents`, in the same order, so its index lines up with wherever a
/// caller tracks the active object (`crate::shell::selection::Selection`
/// only ever appends, so its last entry is Studio's "last selected").
///
/// An entry with nothing drawable beneath it (an empty container) reads as
/// an empty `Vec` rather than being dropped, so the index every other entry
/// is found at never shifts.
pub(crate) fn read_entries(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referents: &[Ref],
) -> Vec<Vec<Target>> {
    pick::selection(dom, database, referents)
        .iter()
        .map(|entry| {
            entry
                .parts()
                .iter()
                .filter_map(|&part| Target::read(dom, database, Some(part)))
                .collect()
        })
        .collect()
}

/// How far `target`'s own oriented box reaches along `direction` (a unit
/// vector), nearest and farthest: the standard support-function extent of a
/// box, which reduces to a plain axis-aligned bound when `direction` is a
/// world axis and to the same box measured along a rotated axis otherwise —
/// one formula for both of [`Space`]'s cases.
fn support(target: &Target, direction: Vec3) -> (f32, f32) {
    let axes = target.rotation(); // still `Size`-scaled — see `Target::rotation`.
    let half = 0.5
        * (axes.x_axis.dot(direction).abs()
            + axes.y_axis.dot(direction).abs()
            + axes.z_axis.dot(direction).abs());
    let centre = target.position().dot(direction);
    (centre - half, centre + half)
}

/// The Min/Center/Max value of the union of `parts`' boxes along `direction`,
/// or `None` for no parts at all.
fn bound_along<'a>(
    parts: impl Iterator<Item = &'a Target>,
    direction: Vec3,
    mode: Mode,
) -> Option<f32> {
    let (min, max) = parts
        .map(|target| support(target, direction))
        .reduce(|(min, max), (lo, hi)| (min.min(lo), max.max(hi)))?;
    Some(match mode {
        Mode::Min => min,
        Mode::Center => (min + max) * 0.5,
        Mode::Max => max,
    })
}

/// The axis frame the whole alignment measures and moves along for one
/// `Space::Local` run — see this module's doc comment for why it is one
/// shared frame rather than each entry's own.
fn local_frame(entries: &[Vec<Target>], active_index: usize, relative_to: RelativeTo) -> Mat3 {
    let anchor = match relative_to {
        RelativeTo::ActiveObject => entries.get(active_index).and_then(|entry| entry.first()),
        RelativeTo::SelectionBounds => entries.iter().flatten().next(),
    };
    anchor.map(Target::orientation).unwrap_or(Mat3::IDENTITY)
}

/// Every part's new position after aligning `entries` (one `Vec<Target>` per
/// top-level selected instance, in selection order — see [`read_entries`])
/// under `options`, relative to `entries[active_index]` when
/// [`RelativeTo::ActiveObject`] is picked.
///
/// A selection of fewer than two entries returns nothing: aligning a single
/// object to itself (whether to the selection's own bounds or, with nothing
/// else selected, to "the active object") is a no-op by construction — moved
/// nowhere because it already *is* the reference — so there is nothing here
/// worth writing back to the DOM. Selecting nothing behaves the same way.
pub(crate) fn plan(
    entries: &[Vec<Target>],
    active_index: usize,
    options: Options,
) -> Vec<(Ref, Vec3)> {
    if entries.len() < 2 {
        return Vec::new();
    }

    let frame = local_frame(entries, active_index, options.relative_to);
    let mut deltas = vec![Vec3::ZERO; entries.len()];

    for axis in Axis::ALL
        .into_iter()
        .filter(|&axis| options.axis_enabled(axis))
    {
        let direction = match options.space {
            Space::World => axis.world_direction(),
            Space::Local => axis.local_direction(frame),
        };

        let reference = match options.relative_to {
            RelativeTo::SelectionBounds => {
                bound_along(entries.iter().flatten(), direction, options.mode)
            }
            RelativeTo::ActiveObject => entries
                .get(active_index)
                .and_then(|entry| bound_along(entry.iter(), direction, options.mode)),
        };
        let Some(reference) = reference else {
            continue;
        };

        // Every axis is measured from each entry's *original* placement, not
        // from wherever an earlier axis in this same loop just moved it —
        // sound because World's three axes, and Local's three (columns of one
        // rotation matrix), are always mutually orthogonal: moving an entry
        // along one never changes its own projection onto another, so the
        // deltas below may simply be summed rather than threaded through a
        // moving working copy.
        for (index, entry) in entries.iter().enumerate() {
            if matches!(options.relative_to, RelativeTo::ActiveObject) && index == active_index {
                continue; // the active object stays fixed.
            }
            let Some(value) = bound_along(entry.iter(), direction, options.mode) else {
                continue;
            };
            deltas[index] += direction * (reference - value);
        }
    }

    // The active object itself is left out of the result entirely, not just
    // given a zero delta: `studio/align-tool.md` — "it will not move during
    // the operation" — and a caller writing every returned position back to
    // the DOM should not have to filter out a no-op write to something that
    // was never meant to move at all.
    entries
        .iter()
        .enumerate()
        .filter(|&(index, _)| {
            !(matches!(options.relative_to, RelativeTo::ActiveObject) && index == active_index)
        })
        .flat_map(|(index, entry)| {
            let delta = deltas[index];
            entry
                .iter()
                .map(move |target| (target.referent, target.position() + delta))
        })
        .collect()
}

#[cfg(test)]
#[path = "align/tests.rs"]
mod tests;
