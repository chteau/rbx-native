//! Which properties a slider can span, and how far.
//!
//! The reflection dump carries no bounds — `Transparency` and `Brightness`
//! are both a bare `float` to it — so the floors and ceilings here are
//! named by hand, from what the value *means* rather than from what the
//! engine will accept.
//!
//! That distinction is the whole safety argument. A range here only decides
//! how far the rail reaches: the number beside it still commits through
//! `edit::parse` like any typed value, so a `GuiObject` really can be
//! rotated 400° and a light really can be brighter than 10. Getting a range
//! wrong costs reach, never correctness — which is why a property whose
//! sensible span is genuinely open-ended (`FogEnd`, a `Size`) is simply
//! absent rather than given a guess.
//!
//! Matched on the property's name alone, with no class beside it. Every
//! name below means the same thing wherever it appears — a `Transparency`
//! is 0–1 on a `BasePart`, a `Decal` and a `UIStroke` alike — and the one
//! name that spans two classes, `Rotation`, wants the same 0–360 on a
//! `GuiObject` as on a `UIGradient`.

/// How far a slider reaches for one property, and how finely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SliderSpan {
    pub(crate) min: f32,
    pub(crate) max: f32,
    /// The smallest move the rail can make.
    ///
    /// Named per property rather than derived from the span, because the
    /// useful increment is a fact about the unit and not about the
    /// distance: a rotation steps by a degree whether it runs to 180 or to
    /// 360, and a hundredth of a `Transparency` is a visible change where a
    /// hundredth of a `ClockTime` is thirty-six seconds.
    ///
    /// It never goes below a thousandth, which is what the field beside the
    /// rail prints (see `shell::scrub::format`) — a step finer than that
    /// moves a value nobody can see and commits a write nobody asked for.
    pub(crate) step: f32,
}

/// The span a slider should offer for `name`, if offering one makes sense.
pub(crate) fn slider_range(name: &str) -> Option<SliderSpan> {
    RANGES
        .iter()
        .find(|(property, ..)| *property == name)
        .map(|&(_, min, max, step)| SliderSpan { min, max, step })
}

/// Property, floor, ceiling, step. Grouped by what the number is, since
/// that is what decides all three.
const RANGES: &[(&str, f32, f32, f32)] = &[
    // A fraction of one, in every case.
    ("Transparency", 0., 1., 0.01),
    ("BackgroundTransparency", 0., 1., 0.01),
    ("TextTransparency", 0., 1., 0.01),
    ("TextStrokeTransparency", 0., 1., 0.01),
    ("ImageTransparency", 0., 1., 0.01),
    ("GroupTransparency", 0., 1., 0.01),
    ("ScrollBarImageTransparency", 0., 1., 0.01),
    ("Reflectance", 0., 1., 0.01),
    ("LightInfluence", 0., 1., 0.01),
    ("ShadowSoftness", 0., 1., 0.01),
    ("EnvironmentDiffuseScale", 0., 1., 0.01),
    ("EnvironmentSpecularScale", 0., 1., 0.01),
    ("Density", 0., 1., 0.01),
    ("WaterTransparency", 0., 1., 0.01),
    ("WaterReflectance", 0., 1., 0.01),
    ("WaterWaveSize", 0., 1., 0.01),
    // Angles, in the units the panel shows.
    ("Rotation", 0., 360., 1.),
    ("Angle", 0., 180., 1.),
    ("GeographicLatitude", -90., 90., 1.),
    // An hour of the day, which is the one range Roblox itself wraps rather
    // than clamps — a `ClockTime` of 25 is 1am. The rail stops at 24 so a
    // drag walks one day; typing past it still wraps the way it always did.
    ("ClockTime", 0., 24., 0.1),
    // Open-ended in the engine, but not in practice: past these the scene
    // is blown out rather than brighter, and the field still takes more.
    ("Brightness", 0., 10., 0.1),
    ("ExposureCompensation", -5., 5., 0.1),
    ("Glare", 0., 10., 0.1),
    ("Haze", 0., 10., 0.1),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A slider is offered for the values that have a real ceiling and for
    /// nothing else — a `Size` or a `FogEnd` has no end to put one at.
    #[test]
    fn only_bounded_properties_get_a_range() {
        assert_eq!(
            slider_range("Transparency"),
            Some(SliderSpan {
                min: 0.,
                max: 1.,
                step: 0.01
            })
        );
        assert_eq!(
            slider_range("ClockTime"),
            Some(SliderSpan {
                min: 0.,
                max: 24.,
                step: 0.1
            })
        );

        assert_eq!(slider_range("FogEnd"), None);
        assert_eq!(slider_range("Name"), None);
        assert_eq!(slider_range(""), None);
    }

    /// Every range runs upward, and every step fits inside it without
    /// going under what the field beside the rail can print — a `step` left
    /// at the toolkit's own default of 1 would turn a `Transparency` into a
    /// two-position switch.
    #[test]
    fn every_range_runs_upward_in_steps_it_can_print() {
        for (property, min, max, step) in RANGES {
            assert!(min < max, "{property}: {min} is not below {max}");
            assert!(*step >= 0.001, "{property}: {step} is finer than 3dp");
            assert!(
                step < &(max - min),
                "{property}: {step} does not fit in {min}..{max}"
            );
        }
    }
}
