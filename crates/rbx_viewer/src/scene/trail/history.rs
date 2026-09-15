//! A trail's segment history: what `Trail.MinLength` and `Trail.Lifetime`
//! actually mean, kept apart from appearance sampling ([`super::ribbon`]) so
//! it is testable with nothing but positions and a clock — no DOM, no GPU.

use std::collections::VecDeque;

use glam::Vec3;

/// One recorded slice of a trail: both attachments' world positions at the
/// moment it was recorded, or last nudged (see [`Recorder::record`]), and
/// when.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sample {
    pub(crate) time: f32,
    pub(crate) position0: Vec3,
    pub(crate) position1: Vec3,
}

/// The running history one `Trail` keeps between frames.
///
/// Oldest first: [`Recorder::record`] pushes onto the back,
/// [`Recorder::expire`] drops from the front — `renderer::trail` owns one
/// instance per live `Trail`, the same way `renderer::beam::Beams` owns its
/// wall clock.
#[derive(Debug, Clone, Default)]
pub(crate) struct Recorder {
    samples: VecDeque<Sample>,
}

impl Recorder {
    pub(crate) fn new() -> Self {
        Recorder {
            samples: VecDeque::new(),
        }
    }

    /// Records one frame's attachment positions at `time`.
    ///
    /// `Trail.MinLength`'s own wording: a new segment is only appended once
    /// *either* attachment has moved at least `min_length` studs since the
    /// last recorded sample; short of that, "the endpoints of the current
    /// segment [are] moved to the current position of the attachments"
    /// instead of growing the history, which is why the second arm below
    /// overwrites in place rather than pushing.
    pub(crate) fn record(&mut self, time: f32, position0: Vec3, position1: Vec3, min_length: f32) {
        match self.samples.back_mut() {
            Some(last) if moved(last, position0, position1, min_length) => {
                self.samples.push_back(Sample {
                    time,
                    position0,
                    position1,
                });
            }
            Some(last) => {
                last.position0 = position0;
                last.position1 = position1;
            }
            None => self.samples.push_back(Sample {
                time,
                position0,
                position1,
            }),
        }
    }

    /// Drops every sample whose age relative to `now` exceeds `lifetime` —
    /// `Trail.Lifetime`'s per-segment expiry. Oldest-first storage means this
    /// is always a prefix.
    pub(crate) fn expire(&mut self, now: f32, lifetime: f32) {
        while let Some(front) = self.samples.front() {
            if now - front.time > lifetime {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub(crate) fn samples(&self) -> &VecDeque<Sample> {
        &self.samples
    }
}

/// Either attachment moved at least `min_length` studs since `last`.
///
/// Perfectly stationary attachments (`delta == 0.0` on both) never start a
/// new segment, whatever `min_length` is — even at its allowed floor of 0
/// studs, a zero-length segment adds nothing to the trail's shape, only
/// churn, and this is exactly the case a viewer with no scripted motion hits
/// every single frame (see `scene::trail`'s module doc): treating it as "no
/// movement" is what keeps that a true no-op instead of an unbounded, purely
/// cosmetic history of coincident points.
fn moved(last: &Sample, position0: Vec3, position1: Vec3, min_length: f32) -> bool {
    let delta0 = (position0 - last.position0).length();
    let delta1 = (position1 - last.position1).length();
    if delta0 == 0.0 && delta1 == 0.0 {
        return false;
    }
    let min_length = min_length.max(0.0);
    delta0 >= min_length || delta1 >= min_length
}

#[cfg(test)]
#[path = "history/tests.rs"]
mod tests;
