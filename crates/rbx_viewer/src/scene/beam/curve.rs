//! A `Beam`'s cubic Bézier curve: `P0`/`P3` are the two attachments' world
//! positions, `P1`/`P2` sit `CurveSize0`/`CurveSize1` studs from them along
//! their own **X** axis (`Attachment.Axis`) — see the `Beam` docs' "Beam
//! Curvature" section, which the task brief's "look vector" was a
//! simplification of.

use glam::Vec3;

/// A beam's curve, control points already resolved into world space so
/// [`Curve::position`]/[`Curve::tangent`] never touch the DOM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Curve {
    p0: Vec3,
    p1: Vec3,
    p2: Vec3,
    p3: Vec3,
}

impl Curve {
    /// `axis0`/`axis1` are `Attachment0`/`Attachment1`'s world-space X axis,
    /// expected already normalized (see `scene::beam::instance::build`).
    pub(crate) fn new(
        p0: Vec3,
        axis0: Vec3,
        curve_size0: f32,
        p3: Vec3,
        axis1: Vec3,
        curve_size1: f32,
    ) -> Self {
        Curve {
            p0,
            p1: p0 + axis0 * curve_size0,
            // P2 sits in the *negative* X direction of Attachment1.
            p2: p3 - axis1 * curve_size1,
            p3,
        }
    }

    pub(crate) fn position(&self, t: f32) -> Vec3 {
        let t = t.clamp(0.0, 1.0);
        let mt = 1.0 - t;
        self.p0 * (mt * mt * mt)
            + self.p1 * (3.0 * mt * mt * t)
            + self.p2 * (3.0 * mt * t * t)
            + self.p3 * (t * t * t)
    }

    /// Unit tangent at `t`.
    ///
    /// Falls back to the straight chord `p3 - p0` where the analytic
    /// derivative vanishes, which it does exactly at `t = 0`/`1` whenever both
    /// curve sizes are 0 (the default, straight-beam case): with `P1 = P0` and
    /// `P2 = P3` the curve is geometrically the straight segment between them,
    /// just reparametrized with zero speed at its own endpoints (an
    /// ease-in-out weighting), so the direction is still well defined even
    /// though the derivative formula alone is not.
    pub(crate) fn tangent(&self, t: f32) -> Vec3 {
        let t = t.clamp(0.0, 1.0);
        let mt = 1.0 - t;
        let derivative = (self.p1 - self.p0) * (3.0 * mt * mt)
            + (self.p2 - self.p1) * (6.0 * mt * t)
            + (self.p3 - self.p2) * (3.0 * t * t);
        let unit = derivative.normalize_or_zero();
        if unit != Vec3::ZERO {
            return unit;
        }
        let chord = (self.p3 - self.p0).normalize_or_zero();
        if chord != Vec3::ZERO {
            return chord;
        }
        // Both endpoints coincide and curve sizes are 0: a zero-length beam has
        // no meaningful direction, so pick a stable arbitrary one rather than NaN.
        Vec3::Z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < EPSILON
    }

    #[test]
    fn position_reaches_both_endpoints_exactly() {
        let curve = Curve::new(
            Vec3::ZERO,
            Vec3::X,
            3.0,
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::NEG_X,
            2.0,
        );
        assert!(close(curve.position(0.0), Vec3::ZERO));
        assert!(close(curve.position(1.0), Vec3::new(10.0, 0.0, 0.0)));
    }

    #[test]
    fn zero_curve_sizes_trace_the_straight_chord() {
        // P1 = P0 and P2 = P3 collapse the curve onto the P0-P3 segment, just
        // reached at a smoothstep pace (`t^2(3-2t)`) rather than a linear one
        // — see `Curve::tangent`'s doc comment.
        let curve = Curve::new(
            Vec3::ZERO,
            Vec3::X,
            0.0,
            Vec3::new(4.0, 8.0, 0.0),
            Vec3::X,
            0.0,
        );
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let pace = t * t * (3.0 - 2.0 * t);
            let expected = Vec3::ZERO.lerp(Vec3::new(4.0, 8.0, 0.0), pace);
            assert!(close(curve.position(t), expected), "t={t}");
        }
    }

    #[test]
    fn tangent_at_the_start_follows_attachment0s_axis_when_curved() {
        let curve = Curve::new(
            Vec3::ZERO,
            Vec3::Y,
            5.0,
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::NEG_X,
            5.0,
        );
        assert!(close(curve.tangent(0.0), Vec3::Y));
    }

    #[test]
    fn tangent_falls_back_to_the_chord_when_the_derivative_vanishes() {
        let curve = Curve::new(
            Vec3::ZERO,
            Vec3::X,
            0.0,
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::X,
            0.0,
        );
        assert!(close(curve.tangent(0.0), Vec3::Y));
        assert!(close(curve.tangent(1.0), Vec3::Y));
    }

    #[test]
    fn a_zero_length_beam_has_a_stable_non_nan_tangent() {
        let curve = Curve::new(Vec3::ZERO, Vec3::X, 0.0, Vec3::ZERO, Vec3::X, 0.0);
        let tangent = curve.tangent(0.5);
        assert!(tangent.is_finite());
        assert!((tangent.length() - 1.0).abs() < EPSILON);
    }
}
