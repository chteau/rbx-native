//! The model behind the Curve Editor: a `CurveAnimation`'s per-channel
//! `FloatCurve`s, evaluated the way Roblox documents `FloatCurveKey`s, plus
//! conversion to and from a [`Clip`](crate::clip::Clip). [`CurveClip`] is the
//! entry point; [`from_clip`] and [`to_clip`] move between the two forms.
//!
//! A saved `CurveAnimation` is read through [`file`], whose key bytes
//! [`values`] decodes (a layout read off real catalog emotes, as Roblox does
//! not publish it). [`values`] also writes those bytes back identically, but
//! the editor still saves curves by baking them to a `KeyframeSequence`:
//! the model drops the auto slopes Roblox stores for unset tangents, so a
//! `CurveAnimation` rebuilt from it would not match the file it came from.

use crate::clip::TIME_EPSILON;
use crate::easing::{ease, Direction, Style};

mod convert;
mod euler;
pub mod file;
mod joint;
mod rotation;
pub mod values;

pub use convert::{from_clip, from_clip_in_order, to_clip};
pub use euler::RotationOrder;
pub use joint::{value_range, Channel, CurveClip, JointCurves, Ranges};

/// `Enum.KeyInterpolationMode`: how a key travels to the *next* key.
///
/// The API's own default is `Cubic`, but the Curve Editor makes `Linear`
/// keys, and the editor is what this model serves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Interpolation {
    Constant,
    #[default]
    Linear,
    Cubic,
}

impl Interpolation {
    pub const ALL: [Interpolation; 3] = [
        Interpolation::Constant,
        Interpolation::Linear,
        Interpolation::Cubic,
    ];

    pub fn value(self) -> u32 {
        match self {
            Interpolation::Constant => 0,
            Interpolation::Linear => 1,
            Interpolation::Cubic => 2,
        }
    }

    pub fn from_value(value: u32) -> Interpolation {
        match value {
            0 => Interpolation::Constant,
            2 => Interpolation::Cubic,
            _ => Interpolation::Linear,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Interpolation::Constant => "Constant",
            Interpolation::Linear => "Linear",
            Interpolation::Cubic => "Cubic",
        }
    }
}

/// A `FloatCurveKey`. Tangents are slopes in value units **per second**, so
/// retiming keys keeps a handle's angle; `None` is an unset ("auto") tangent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatKey {
    pub time: f32,
    pub value: f32,
    pub interpolation: Interpolation,
    pub left_tangent: Option<f32>,
    /// Only meaningful on a `Cubic` key; the editing methods keep it `None`
    /// otherwise, as Roblox refuses to set it there.
    pub right_tangent: Option<f32>,
}

impl FloatKey {
    pub fn new(time: f32, value: f32, interpolation: Interpolation) -> FloatKey {
        FloatKey {
            time,
            value,
            interpolation,
            left_tangent: None,
            right_tangent: None,
        }
    }

    /// The slopes evaluation uses, `(left, right)`.
    ///
    /// Roblox's runtime doc says an unset tangent counts as 0, while the
    /// Curve Editor doc says an unset tangent lines up with its set opposite.
    /// Mirroring when only one is set, and 0 when neither is, satisfies both.
    pub fn tangents(&self) -> (f32, f32) {
        let left = self.left_tangent.or(self.right_tangent).unwrap_or(0.);
        let right = self.right_tangent.or(self.left_tangent).unwrap_or(0.);
        (left, right)
    }
}

/// A `FloatCurve`: keys sorted by time, never two within
/// [`TIME_EPSILON`] of each other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FloatCurve {
    pub keys: Vec<FloatKey>,
}

/// The cubic Hermite basis, with slopes per second scaled by the segment's
/// length `h` to slopes per unit of `u`.
pub(crate) fn hermite(v0: f32, v1: f32, m0: f32, m1: f32, h: f32, u: f32) -> f32 {
    let (u2, u3) = (u * u, u * u * u);
    (2. * u3 - 3. * u2 + 1.) * v0
        + (u3 - 2. * u2 + u) * h * m0
        + (-2. * u3 + 3. * u2) * v1
        + (u3 - u2) * h * m1
}

/// Evenly spaced times strictly between `t0` and `t1`, at most `1 / fps`
/// apart. Even spacing (rather than whole frames from `t0`) keeps a sliver
/// of a step from landing just before `t1`.
pub(crate) fn bake_times(t0: f32, t1: f32, fps: f32) -> impl Iterator<Item = f32> {
    let span = t1 - t0;
    let steps = if span > 0. && fps > 0. {
        (span * fps).ceil().max(1.) as usize
    } else {
        1
    };
    (1..steps).map(move |i| t0 + span * i as f32 / steps as f32)
}

impl FloatCurve {
    pub fn new(keys: Vec<FloatKey>) -> FloatCurve {
        let mut curve = FloatCurve::default();
        for key in keys {
            curve.insert(key);
        }
        curve
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The last key's time, which is what `FloatCurve.Length` reports.
    pub fn length(&self) -> f32 {
        self.keys.last().map_or(0., |k| k.time)
    }

    /// `None` for a curve with no keys, which Roblox evaluates to nil. Before
    /// the first key and after the last the curve holds that key's value.
    pub fn eval(&self, time: f32) -> Option<f32> {
        let first = self.keys.first()?;
        let i = self.keys.partition_point(|k| k.time <= time);
        if i == 0 {
            return Some(first.value);
        }
        let k0 = &self.keys[i - 1];
        let Some(k1) = self.keys.get(i) else {
            return Some(k0.value);
        };
        let h = k1.time - k0.time;
        let u = if h > 0. { (time - k0.time) / h } else { 1. };
        Some(match k0.interpolation {
            Interpolation::Constant => k0.value,
            Interpolation::Linear => k0.value + (k1.value - k0.value) * u,
            Interpolation::Cubic => hermite(
                k0.value,
                k1.value,
                self.slopes(i - 1).1,
                self.slopes(i).0,
                h,
                u,
            ),
        })
    }

    /// The `(left, right)` slopes key `i` plays with. An unset tangent is
    /// the slope to the next key (0 on the last one): that is what a file
    /// stores for it (see [`values`]) and what real Studio's Animator plays,
    /// matched within 1e-4 on a FACS curve (`tests/studio_graph.rs`), where
    /// reading it as 0 was 0.22 off.
    fn slopes(&self, i: usize) -> (f32, f32) {
        let k = &self.keys[i];
        let auto = self.keys.get(i + 1).map_or(0., |n| {
            let h = n.time - k.time;
            if h > 0. {
                (n.value - k.value) / h
            } else {
                0.
            }
        });
        (
            k.left_tangent.unwrap_or(auto),
            k.right_tangent.unwrap_or(auto),
        )
    }

    pub fn index_at(&self, time: f32) -> Option<usize> {
        self.keys
            .iter()
            .position(|k| (k.time - time).abs() < TIME_EPSILON)
    }

    /// Adds a key in time order, replacing any key already at its time (as
    /// `FloatCurve:InsertKey` does). Returns where it landed.
    pub fn insert(&mut self, mut key: FloatKey) -> usize {
        if key.interpolation != Interpolation::Cubic {
            key.right_tangent = None;
        }
        if let Some(i) = self.index_at(key.time) {
            self.keys[i] = key;
            return i;
        }
        let i = self.keys.partition_point(|k| k.time < key.time);
        self.keys.insert(i, key);
        i
    }

    pub fn remove(&mut self, index: usize) -> Option<FloatKey> {
        (index < self.keys.len()).then(|| self.keys.remove(index))
    }

    /// Moves a key to another time, replacing whatever key was already
    /// there. Returns the key's new index.
    pub fn move_key(&mut self, index: usize, time: f32) -> Option<usize> {
        let mut key = self.remove(index)?;
        key.time = time;
        Some(self.insert(key))
    }

    pub fn set_value(&mut self, index: usize, value: f32) -> bool {
        self.keys.get_mut(index).map(|k| k.value = value).is_some()
    }

    /// `None` puts a tangent back to auto ("Set Tangent To Zero" in the
    /// editor). A right tangent on a key that is not `Cubic` is dropped.
    pub fn set_tangents(&mut self, index: usize, left: Option<f32>, right: Option<f32>) -> bool {
        let Some(key) = self.keys.get_mut(index) else {
            return false;
        };
        key.left_tangent = left;
        key.right_tangent = right.filter(|_| key.interpolation == Interpolation::Cubic);
        true
    }

    /// Leaving `Cubic` clears the right tangent, as Roblox does.
    pub fn set_interpolation(&mut self, index: usize, mode: Interpolation) -> bool {
        let Some(key) = self.keys.get_mut(index) else {
            return false;
        };
        key.interpolation = mode;
        if mode != Interpolation::Cubic {
            key.right_tangent = None;
        }
        true
    }

    /// The editor's Generate Curve: replaces the keys between `from` and
    /// `to` with Linear keys baked from `style` at `fps`, keeping both ends.
    pub fn generate(
        &mut self,
        from: usize,
        to: usize,
        style: Style,
        direction: Direction,
        fps: f32,
    ) -> bool {
        if from >= to || to >= self.keys.len() {
            return false;
        }
        let (k0, k1) = (self.keys[from], self.keys[to]);
        let baked: Vec<FloatKey> = bake_times(k0.time, k1.time, fps)
            .map(|t| {
                let alpha = ease(style, direction, (t - k0.time) / (k1.time - k0.time));
                FloatKey::new(
                    t,
                    k0.value + (k1.value - k0.value) * alpha,
                    Interpolation::Linear,
                )
            })
            .collect();
        self.set_interpolation(from, Interpolation::Linear);
        self.keys.splice(from + 1..to, baked);
        true
    }

    /// The lowest and highest value the curve reaches, cubic overshoot
    /// included (found by sampling, which is plenty for fitting a plot).
    pub fn range(&self) -> Option<(f32, f32)> {
        let mut range: Option<(f32, f32)> = None;
        let mut widen = |v: f32| {
            range = Some(range.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v))));
        };
        for pair in self.keys.windows(2) {
            if pair[0].interpolation == Interpolation::Cubic {
                for i in 1..32 {
                    let t = pair[0].time + (pair[1].time - pair[0].time) * i as f32 / 32.;
                    widen(self.eval(t).unwrap_or(0.));
                }
            }
        }
        self.keys.iter().for_each(|k| widen(k.value));
        range
    }
}

#[cfg(test)]
mod convert_tests;
#[cfg(test)]
mod tests;
