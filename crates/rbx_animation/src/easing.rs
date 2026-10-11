//! `Enum.PoseEasingStyle` and `Enum.PoseEasingDirection`: how a joint moves
//! from one pose to the next.
//!
//! Roblox's sources disagree on the directions. The `PoseEasingDirection`
//! enum doc says pose `In`/`Out` are "backwards from TweenService" for legacy
//! reasons. The Animation Editor's own docs give them the standard meaning
//! (`In` starts slow, `Out` ends slow). So does the widely used Blender
//! exporter, whose authors checked it against `Animator:StepAnimations`. The
//! `PoseEasingStyle` doc deprecates `Cubic` because its "direction is
//! reversed between the editor and runtime". Read together, the legacy
//! `Cubic` is the backwards one, and `CubicV2`, `Elastic` and `Bounce` follow
//! TweenService. That is what this module does. It has not been checked
//! against a running Roblox client.
//!
//! Roblox does not publish the curves themselves. Cubic, Elastic and Bounce
//! use the Penner shapes that TweenService's styles of the same names follow.
//! For `Constant`, the enum says it "snaps to the keyframe indicated by the
//! direction". Here `Out` holds the earlier pose, `In` jumps to the next one
//! straight away (the exporter's reading) and `InOut` swaps at the midpoint.

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Style {
    #[default]
    Linear,
    Constant,
    Elastic,
    Cubic,
    Bounce,
    CubicV2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    In,
    Out,
    InOut,
}

impl Style {
    /// Every style, in the order a picker should list them. The deprecated
    /// `Cubic` is left out.
    pub const PICKABLE: [Style; 5] = [
        Style::Linear,
        Style::Constant,
        Style::CubicV2,
        Style::Elastic,
        Style::Bounce,
    ];

    pub fn value(self) -> u32 {
        match self {
            Style::Linear => 0,
            Style::Constant => 1,
            Style::Elastic => 2,
            Style::Cubic => 3,
            Style::Bounce => 4,
            Style::CubicV2 => 5,
        }
    }

    pub fn from_value(value: u32) -> Style {
        match value {
            1 => Style::Constant,
            2 => Style::Elastic,
            3 => Style::Cubic,
            4 => Style::Bounce,
            5 => Style::CubicV2,
            _ => Style::Linear,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Style::Linear => "Linear",
            Style::Constant => "Constant",
            Style::Elastic => "Elastic",
            Style::Cubic => "Cubic",
            Style::Bounce => "Bounce",
            Style::CubicV2 => "Cubic",
        }
    }

    /// The `In` curve, as TweenService draws the style of the same name.
    fn forward(self, t: f32) -> f32 {
        match self {
            Style::Linear => t,
            Style::Constant => 0.,
            Style::Cubic | Style::CubicV2 => t * t * t,
            Style::Elastic => {
                if t <= 0. || t >= 1. {
                    return t.clamp(0., 1.);
                }
                // Penner's elastic with period 0.3 and amplitude 1.
                let p = 0.3;
                let s = p / 4.;
                let u = t - 1.;
                -(2f32.powf(10. * u)) * ((u - s) * (2. * PI) / p).sin()
            }
            Style::Bounce => 1. - bounce_out(1. - t),
        }
    }
}

fn bounce_out(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1. / D {
        N * t * t
    } else if t < 2. / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984375
    }
}

impl Direction {
    pub const ALL: [Direction; 3] = [Direction::In, Direction::Out, Direction::InOut];

    pub fn value(self) -> u32 {
        match self {
            Direction::In => 0,
            Direction::Out => 1,
            Direction::InOut => 2,
        }
    }

    pub fn from_value(value: u32) -> Direction {
        match value {
            1 => Direction::Out,
            2 => Direction::InOut,
            _ => Direction::In,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Direction::In => "In",
            Direction::Out => "Out",
            Direction::InOut => "InOut",
        }
    }
}

/// How far (0 at this pose, 1 at the next) a joint has travelled at `t`, the
/// fraction of the time between the two keyframes. Elastic overshoots both
/// ends, so the result is not confined to 0..1.
pub fn ease(style: Style, direction: Direction, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    if style == Style::Constant {
        return match direction {
            Direction::In if t > 0. => 1.,
            Direction::InOut if t >= 0.5 => 1.,
            _ if t >= 1. => 1.,
            _ => 0.,
        };
    }
    // The deprecated Cubic plays with its direction reversed.
    let direction = match (style, direction) {
        (Style::Cubic, Direction::In) => Direction::Out,
        (Style::Cubic, Direction::Out) => Direction::In,
        _ => direction,
    };
    match direction {
        Direction::In => style.forward(t),
        Direction::Out => 1. - style.forward(1. - t),
        Direction::InOut => {
            if t < 0.5 {
                style.forward(2. * t) / 2.
            } else {
                1. - style.forward(2. - 2. * t) / 2.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: [Style; 6] = [
        Style::Linear,
        Style::Constant,
        Style::Elastic,
        Style::Cubic,
        Style::Bounce,
        Style::CubicV2,
    ];

    #[test]
    fn enum_values_round_trip() {
        for s in STYLES {
            assert_eq!(Style::from_value(s.value()), s);
        }
        for d in Direction::ALL {
            assert_eq!(Direction::from_value(d.value()), d);
        }
        assert_eq!(Style::CubicV2.value(), 5);
        assert_eq!(Direction::InOut.value(), 2);
    }

    #[test]
    fn every_curve_starts_and_ends_on_its_poses() {
        for s in STYLES {
            for d in Direction::ALL {
                assert!(ease(s, d, 0.).abs() < 1e-5, "{s:?} {d:?} at 0");
                assert!((ease(s, d, 1.) - 1.).abs() < 1e-5, "{s:?} {d:?} at 1");
            }
        }
    }

    #[test]
    fn cubic_v2_follows_tweenservice_and_legacy_cubic_is_reversed() {
        assert!((ease(Style::CubicV2, Direction::In, 0.5) - 0.125).abs() < 1e-6);
        assert!((ease(Style::CubicV2, Direction::Out, 0.5) - 0.875).abs() < 1e-6);
        assert!((ease(Style::Cubic, Direction::In, 0.5) - 0.875).abs() < 1e-6);
        assert!((ease(Style::CubicV2, Direction::InOut, 0.5) - 0.5).abs() < 1e-6);
        assert!(ease(Style::CubicV2, Direction::InOut, 0.25) < 0.25);
    }

    #[test]
    fn constant_holds_then_snaps() {
        assert_eq!(ease(Style::Constant, Direction::Out, 0.99), 0.);
        assert_eq!(ease(Style::Constant, Direction::In, 0.01), 1.);
        assert_eq!(ease(Style::Constant, Direction::InOut, 0.4), 0.);
        assert_eq!(ease(Style::Constant, Direction::InOut, 0.6), 1.);
    }

    #[test]
    fn elastic_overshoots_and_bounce_stays_inside() {
        let samples = (1..100).map(|i| i as f32 / 100.);
        assert!(samples
            .clone()
            .any(|t| ease(Style::Elastic, Direction::Out, t) > 1.));
        for t in samples {
            let b = ease(Style::Bounce, Direction::In, t);
            assert!((0. ..=1.0001).contains(&b), "{t} -> {b}");
        }
    }

    #[test]
    fn linear_ignores_direction() {
        for d in Direction::ALL {
            assert!((ease(Style::Linear, d, 0.3) - 0.3).abs() < 1e-6);
        }
    }
}
