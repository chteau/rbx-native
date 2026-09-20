//! The curve and gradient editor behind a `NumberSequence`/`ColorSequence`
//! row: what the keypoints are, what dragging one is allowed to do to them,
//! and where they sit inside a plot rectangle.
//!
//! No rendering and no GPUI here — `shell::sequence_panel` draws this and
//! feeds it pointer positions, and every edit leaves through [`Editor::text`]
//! into the same textual commit path a typed row takes
//! (`properties::edit::sequence`), so the graph is a second *input* to one
//! mutation path rather than a second mutation path.

use rbx_dom::{Color3Data, ColorSequence, NumberSequence, Variant};

mod plot;

pub(crate) use plot::{sample, Handle, Rect};

/// Roblox's own bounds, enforced here so the graph cannot build a sequence
/// `properties::edit::sequence` would then refuse — see that module for the
/// documentation these come from.
const MIN_STOPS: usize = 2;
const MAX_STOPS: usize = 20;

/// How far above the tallest keypoint the value axis reaches. Without it the
/// top stop sits exactly on the plot's top edge, with half its handle
/// clipped and nothing above it to drag into.
const HEADROOM: f32 = 1.15;

/// How close in normalized plot space a click has to land to grab a handle.
/// A handle draws at roughly 5px on a ~440x260 plot, so this is a little
/// over the mark's own radius — WCAG 2.5.8's target floor asks for slack
/// around a small control, and a curve editor is unusable if a grab has to
/// be pixel-exact.
const GRAB_RADIUS: f32 = 0.045;

/// Which sequence is being edited. The two share every operation below —
/// stops move, split and merge identically — and differ only in what a stop
/// *carries*, so they share one type rather than two near-identical ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Number,
    Color,
}

/// One keypoint, in the shape the editor works with whichever sequence it
/// came from: a `Number` stop ignores `color`, a `Color` stop ignores
/// `value`. Carrying both is one unused float per stop and saves a second
/// copy of every operation in this module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stop {
    pub(crate) time: f32,
    pub(crate) value: f32,
    pub(crate) envelope: f32,
    pub(crate) color: Color3Data,
}

/// A drag in progress: which stop, and whether the pointer is moving the
/// stop itself or the envelope handle above it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drag {
    pub(crate) index: usize,
    pub(crate) handle: Handle,
}

/// One sequence, laid out for editing.
///
/// Deliberately **not** something the panel keeps: `shell::sequence_panel`
/// builds one from whatever the DOM holds at that instant, applies the edit,
/// and commits the result as text. The DOM stays the single copy of the
/// value, so an undo, a script, or a second panel writing the same property
/// all show up here with nothing to invalidate — and nothing here can go
/// stale against the viewport it is supposed to be steering.
pub(crate) struct Editor {
    pub(crate) kind: Kind,
    pub(crate) stops: Vec<Stop>,
    pub(crate) selected: usize,
    pub(crate) drag: Option<Drag>,
}

impl Editor {
    /// An editor for `value`, or `None` when it is not a sequence at all.
    pub(crate) fn open(value: &Variant) -> Option<Self> {
        let (kind, stops) = match value {
            Variant::NumberSequence(sequence) => (Kind::Number, number_stops(sequence)),
            Variant::ColorSequence(sequence) => (Kind::Color, color_stops(sequence)),
            _ => return None,
        };
        Some(Editor {
            kind,
            stops,
            selected: 0,
            drag: None,
        })
    }

    /// The commit text, in the spelling `properties::edit::sequence` reads
    /// back — the only way an edit here reaches the DOM.
    pub(crate) fn text(&self) -> String {
        let one = |stop: &Stop| match self.kind {
            Kind::Number => format!("{}, {}, {}", stop.time, stop.value, stop.envelope),
            Kind::Color => format!(
                "{}, {}, {}, {}",
                stop.time,
                channel(stop.color.r),
                channel(stop.color.g),
                channel(stop.color.b)
            ),
        };
        self.stops
            .iter()
            .map(one)
            .collect::<Vec<String>>()
            .join("; ")
    }

    pub(crate) fn selected_stop(&self) -> Stop {
        self.stops[self.selected.min(self.stops.len() - 1)]
    }

    /// The top of the value axis. Auto-fitted rather than typed into a "Max"
    /// box the way Studio's graph asks for one: every sequence this editor
    /// opens already says how tall it is. A drag therefore cannot push a
    /// value past the current ceiling — the **Value** field in the panel's
    /// footer is the way up, and the axis refits around it on the next
    /// frame.
    pub(crate) fn ceiling(&self) -> f32 {
        let reach = self
            .stops
            .iter()
            .map(|stop| stop.value + stop.envelope)
            .fold(1.0_f32, f32::max)
            * HEADROOM;
        // Rounded up to something a gridline can land on, so the top of the
        // plot is a number a reader recognizes rather than 3.7418.
        let step = 10.0_f32.powf(reach.log10().floor()) / 2.0;
        (reach / step).ceil() * step
    }

    /// The stop a click at this normalized position should grab, and which
    /// of its handles. `None` for a click that landed on empty plot.
    pub(crate) fn grab(&self, time: f32, value: f32) -> Option<Drag> {
        let ceiling = self.ceiling();
        let mut best: Option<(f32, Drag)> = None;
        for (index, stop) in self.stops.iter().enumerate() {
            for handle in plot::handles(self.kind, stop) {
                let at = plot::handle_value(handle, stop);
                // A colour stop is a marker on a timeline: it has no height,
                // so grabbing it must not depend on how high up the ramp the
                // pointer happened to be.
                let distance = match self.kind {
                    Kind::Color => (time - stop.time).abs(),
                    Kind::Number => plot::distance(time, value, stop.time, at, ceiling),
                };
                if distance <= GRAB_RADIUS && best.is_none_or(|(closest, _)| distance < closest) {
                    best = Some((distance, Drag { index, handle }));
                }
            }
        }
        best.map(|(_, drag)| drag)
    }

    /// Moves whatever [`grab`](Self::grab) picked up to this position.
    ///
    /// Time is clamped between the neighbouring stops so a drag can never
    /// reorder the list behind the user's back, and the first and last stops
    /// keep their times entirely: Roblox's own constructors refuse a
    /// sequence that does not start at 0 and end at 1, so those two are not
    /// free to move even by a pixel.
    pub(crate) fn drag_to(&mut self, time: f32, value: f32) {
        let Some(Drag { index, handle }) = self.drag else {
            return;
        };
        let ceiling = self.ceiling();
        let last = self.stops.len() - 1;
        let (low, high) = match index {
            0 => (0.0, 0.0),
            i if i == last => (1.0, 1.0),
            i => (self.stops[i - 1].time, self.stops[i + 1].time),
        };
        let kind = self.kind;
        let stop = &mut self.stops[index];
        stop.time = time.clamp(low, high);
        match handle {
            // A colour stop only ever moves along the timeline; its `value`
            // is the unused half of [`Stop`] and must stay that way.
            Handle::Point if kind == Kind::Color => {}
            Handle::Point => stop.value = value.clamp(0.0, ceiling),
            // The band is symmetric, so either handle sets the same number
            // and a drag past the stop reads as the distance, not a
            // negative width.
            Handle::Envelope => {
                stop.envelope = (value - stop.value).abs().min(ceiling);
            }
        }
    }

    /// Adds a stop at `time`, interpolating whatever the sequence already
    /// reads there, and selects it. Refused once the list is at Roblox's own
    /// ceiling, or at a time a stop already occupies.
    pub(crate) fn insert(&mut self, time: f32) -> bool {
        if self.stops.len() >= MAX_STOPS {
            return false;
        }
        let time = time.clamp(0.0, 1.0);
        let Some(index) = self.stops.iter().position(|stop| stop.time > time) else {
            return false;
        };
        if index == 0 {
            return false;
        }
        let (before, after) = (self.stops[index - 1], self.stops[index]);
        let span = after.time - before.time;
        let f = if span > 0.0 {
            (time - before.time) / span
        } else {
            0.0
        };
        self.stops.insert(
            index,
            Stop {
                time,
                value: lerp(before.value, after.value, f),
                envelope: lerp(before.envelope, after.envelope, f),
                color: Color3Data {
                    r: lerp(before.color.r, after.color.r, f),
                    g: lerp(before.color.g, after.color.g, f),
                    b: lerp(before.color.b, after.color.b, f),
                },
            },
        );
        self.selected = index;
        true
    }

    /// Whether [`remove_selected`](Self::remove_selected) would do anything
    /// — what greys the panel's Delete button rather than letting it look
    /// live and then refuse.
    pub(crate) fn can_remove_selected(&self) -> bool {
        self.stops.len() > MIN_STOPS && self.selected != 0 && self.selected != self.stops.len() - 1
    }

    /// Drops the selected stop. Refused for either end (their times are
    /// fixed by the format, so removing one leaves a sequence Roblox would
    /// not accept) and once only [`MIN_STOPS`] are left.
    pub(crate) fn remove_selected(&mut self) -> bool {
        if !self.can_remove_selected() {
            return false;
        }
        self.stops.remove(self.selected);
        self.selected = self.selected.saturating_sub(1);
        true
    }

    /// Whether `text` — typed into one of the panel's numeric fields — is a
    /// legal new value for `field` on the selected stop. Applied here rather
    /// than in the field so the same clamping a drag obeys also applies to a
    /// typed number.
    pub(crate) fn set_field(&mut self, field: Field, text: &str) -> bool {
        let Ok(number) = text.trim().parse::<f32>() else {
            return false;
        };
        let index = self.selected;
        let last = self.stops.len() - 1;
        match field {
            Field::Time if index == 0 || index == last => false,
            Field::Time => {
                let (low, high) = (self.stops[index - 1].time, self.stops[index + 1].time);
                self.stops[index].time = number.clamp(low, high);
                true
            }
            Field::Value => {
                self.stops[index].value = number.max(0.0);
                true
            }
            Field::Envelope => {
                self.stops[index].envelope = number.max(0.0);
                true
            }
        }
    }

    pub(crate) fn set_color(&mut self, color: Color3Data) {
        self.stops[self.selected].color = color;
    }

    /// The value this sequence reads at `t`, for drawing the curve and the
    /// ramp. Same linear rule `rbx_viewer`'s `eval_number`/`eval_color` use,
    /// duplicated rather than depended on because this crate does not link
    /// the renderer's scene module.
    #[cfg(test)]
    fn sample(&self, t: f32) -> Stop {
        plot::sample(&self.stops, t)
    }

    /// What the edit becomes once committed — the shape
    /// `properties::edit::parse` rebuilds from [`text`](Self::text). Only
    /// the tests need it as a `Variant`: everything else in the editor
    /// works in stops and commits as text.
    #[cfg(test)]
    fn value(&self) -> Variant {
        match self.kind {
            Kind::Number => Variant::NumberSequence(NumberSequence {
                keypoints: self
                    .stops
                    .iter()
                    .map(|stop| rbx_dom::NumberSequenceKeypoint {
                        time: stop.time,
                        value: stop.value,
                        envelope: stop.envelope,
                    })
                    .collect(),
            }),
            Kind::Color => Variant::ColorSequence(ColorSequence {
                keypoints: self
                    .stops
                    .iter()
                    .map(|stop| rbx_dom::ColorSequenceKeypoint {
                        time: stop.time,
                        color: stop.color,
                        envelope: stop.envelope,
                    })
                    .collect(),
            }),
        }
    }
}

/// Which of the selected stop's numbers a panel field edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Time,
    Value,
    Envelope,
}

fn number_stops(sequence: &NumberSequence) -> Vec<Stop> {
    sequence
        .keypoints
        .iter()
        .map(|k| Stop {
            time: k.time,
            value: k.value,
            envelope: k.envelope,
            color: Color3Data {
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
        })
        .collect()
}

fn color_stops(sequence: &ColorSequence) -> Vec<Stop> {
    sequence
        .keypoints
        .iter()
        .map(|k| Stop {
            time: k.time,
            value: 0.0,
            envelope: k.envelope,
            color: k.color,
        })
        .collect()
}

fn lerp(from: f32, to: f32, f: f32) -> f32 {
    from + (to - from) * f
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
#[path = "sequence_editor/tests.rs"]
mod tests;
