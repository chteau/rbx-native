//! Where the Curve Editor draws things: time across on the shared
//! [`Timeline`], value up the plot. Position channels (studs) and rotation
//! channels (degrees) each fit their own range to the plot's height, read
//! off the left and right rulers, so a joint's six curves all fill the view
//! whatever their units.

use rbx_animation::curve::{value_range, Channel, FloatKey, Interpolation, JointCurves, Ranges};

use crate::timeline::Timeline;

/// The space kept above and below the curves, in pixels.
const PAD: f32 = 14.0;
/// The smallest span a ruler shows, so a flat curve is not blown up.
const MIN_STUDS: f32 = 0.5;
const MIN_RADIANS: f32 = 0.17;
/// A tangent handle's length on screen.
pub(super) const HANDLE: f32 = 36.0;
/// How near the pointer must come to a key or handle to pick it.
pub(super) const HIT: f32 = 6.0;

/// Where a key's tangent handles end on screen, `(left, right)`; `None`
/// where that side has no handle.
pub(super) type Handles = (Option<(f32, f32)>, Option<(f32, f32)>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Plot {
    pub(super) timeline: Timeline,
    pub(super) height: f32,
    /// `(low, high)` of the position and rotation rulers.
    pub(super) position: (f32, f32),
    pub(super) rotation: (f32, f32),
}

/// The value ranges to fit, for `channels` of one joint: what they span,
/// widened a little, and never thinner than the minimum span.
pub(super) fn fit(curves: Option<&JointCurves>, channels: &[Channel]) -> Ranges {
    let Some(curves) = curves else {
        return Ranges::default();
    };
    let shown = JointCurves {
        position: std::array::from_fn(|i| {
            if channels.contains(&Channel::ALL[i]) {
                curves.position[i].clone()
            } else {
                Default::default()
            }
        }),
        rotation: std::array::from_fn(|i| {
            if channels.contains(&Channel::ALL[i + 3]) {
                curves.rotation[i].clone()
            } else {
                Default::default()
            }
        }),
        order: curves.order,
    };
    value_range(&shown)
}

fn span((lo, hi): (f32, f32), min: f32) -> (f32, f32) {
    let mid = (lo + hi) * 0.5;
    let half = ((hi - lo) * 0.55).max(min * 0.5);
    (mid - half, mid + half)
}

impl Plot {
    pub(super) fn new(timeline: Timeline, height: f32, ranges: Ranges) -> Plot {
        Plot {
            timeline,
            height,
            position: span(ranges.position.unwrap_or((0.0, 0.0)), MIN_STUDS),
            rotation: span(ranges.rotation.unwrap_or((0.0, 0.0)), MIN_RADIANS),
        }
    }

    fn range(&self, channel: Channel) -> (f32, f32) {
        if channel.is_rotation() {
            self.rotation
        } else {
            self.position
        }
    }

    pub(super) fn y_of(&self, channel: Channel, value: f32) -> f32 {
        let (lo, hi) = self.range(channel);
        PAD + (hi - value) / (hi - lo) * (self.height - 2.0 * PAD).max(1.0)
    }

    pub(super) fn value_at(&self, channel: Channel, y: f32) -> f32 {
        let (lo, hi) = self.range(channel);
        hi - (y - PAD) / (self.height - 2.0 * PAD).max(1.0) * (hi - lo)
    }

    /// How far one pixel up moves a value on `channel`.
    pub(super) fn per_pixel(&self, channel: Channel) -> f32 {
        let (lo, hi) = self.range(channel);
        (hi - lo) / (self.height - 2.0 * PAD).max(1.0)
    }

    pub(super) fn point(&self, channel: Channel, time: f32, value: f32) -> (f32, f32) {
        (self.timeline.x_of(time), self.y_of(channel, value))
    }

    /// A key's tangent handle ends, `(left, right)`, on screen. Only a Cubic
    /// key shows its right handle; the left one shows when the segment
    /// coming in is Cubic too.
    pub(super) fn handles(
        &self,
        channel: Channel,
        key: &FloatKey,
        incoming_cubic: bool,
    ) -> Handles {
        let (left, right) = key.tangents();
        let (x, y) = self.point(channel, key.time, key.value);
        let towards = |slope: f32, sign: f32| {
            // One second along, and `slope` up, in pixels.
            let dx = self.timeline.x_of(key.time + 1.0) - x;
            let dy = y - self.y_of(channel, key.value + slope);
            let length = (dx * dx + dy * dy).sqrt().max(1e-6);
            (
                x + sign * dx / length * HANDLE,
                y - sign * dy / length * HANDLE,
            )
        };
        (
            incoming_cubic.then(|| towards(left, -1.0)),
            (key.interpolation == Interpolation::Cubic).then(|| towards(right, 1.0)),
        )
    }

    /// The slope a handle dragged to `(x, y)` gives the key, per second.
    pub(super) fn slope_to(&self, channel: Channel, key: &FloatKey, x: f32, y: f32) -> f32 {
        let dt = self.timeline.time_at(x) - key.time;
        if dt.abs() < 1e-4 {
            return if y < self.y_of(channel, key.value) {
                1e3
            } else {
                -1e3
            };
        }
        (self.value_at(channel, y) - key.value) / dt
    }

    /// Ruler values: `count` evenly spaced, top to bottom.
    pub(super) fn ruler(&self, rotation: bool, count: usize) -> Vec<(f32, f32)> {
        let channel = if rotation {
            Channel::RotationX
        } else {
            Channel::PositionX
        };
        (0..count)
            .map(|i| {
                let y = PAD + (self.height - 2.0 * PAD) * i as f32 / (count - 1).max(1) as f32;
                (y, self.value_at(channel, y))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_animation::curve::FloatCurve;

    fn plot() -> Plot {
        let timeline = Timeline {
            start: 0.0,
            scale: 100.0,
            fps: 30,
        };
        Plot::new(
            timeline,
            228.0,
            Ranges {
                position: Some((0.0, 2.0)),
                rotation: Some((-1.0, 1.0)),
            },
        )
    }

    #[test]
    fn values_map_both_ways_and_each_ruler_fills_the_height() {
        let p = plot();
        for (c, v) in [(Channel::PositionY, 1.3), (Channel::RotationZ, -0.4)] {
            let y = p.y_of(c, v);
            assert!((p.value_at(c, y) - v).abs() < 1e-4);
        }
        // Both mids land on the same line: the two rulers share the plot.
        assert!((p.y_of(Channel::PositionX, 1.0) - p.y_of(Channel::RotationX, 0.0)).abs() < 1e-3);
        assert!(p.y_of(Channel::PositionX, 2.0) > 0.0);
        assert!(p.y_of(Channel::PositionX, 0.0) < 228.0);
    }

    #[test]
    fn a_flat_curve_keeps_a_minimum_span() {
        let p = Plot::new(
            plot().timeline,
            200.0,
            Ranges {
                position: Some((1.0, 1.0)),
                rotation: None,
            },
        );
        assert!(p.position.1 - p.position.0 >= MIN_STUDS - 1e-6);
        assert!(p.rotation.1 - p.rotation.0 >= MIN_RADIANS - 1e-6);
    }

    #[test]
    fn a_handle_points_along_its_slope_and_reads_back() {
        let p = plot();
        let mut key = FloatKey::new(1.0, 1.0, Interpolation::Cubic);
        key.right_tangent = Some(0.5);
        let (left, right) = p.handles(Channel::PositionX, &key, false);
        assert!(left.is_none(), "the incoming segment is not cubic");
        let (hx, hy) = right.unwrap();
        let (kx, ky) = p.point(Channel::PositionX, 1.0, 1.0);
        assert!(((hx - kx).hypot(hy - ky) - HANDLE).abs() < 1e-3);
        assert!(hx > kx && hy < ky, "right and up for a rising slope");
        let slope = p.slope_to(Channel::PositionX, &key, hx, hy);
        assert!((slope - 0.5).abs() < 1e-3, "{slope}");
    }

    #[test]
    fn fitting_covers_only_the_channels_shown() {
        let mut joint = JointCurves::default();
        joint.position[0] = FloatCurve::new(vec![
            FloatKey::new(0.0, -3.0, Interpolation::Linear),
            FloatKey::new(1.0, 5.0, Interpolation::Linear),
        ]);
        joint.position[1] = FloatCurve::new(vec![FloatKey::new(0.0, 40.0, Interpolation::Linear)]);
        let r = fit(Some(&joint), &[Channel::PositionX]);
        assert_eq!(r.position, Some((-3.0, 5.0)));
        assert_eq!(r.rotation, None);
        assert_eq!(
            fit(Some(&joint), &Channel::ALL).position,
            Some((-3.0, 40.0))
        );
    }
}
