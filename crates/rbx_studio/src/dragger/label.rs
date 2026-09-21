//! The dark distance label a handle drag shows beside the dragged arrow
//! (`MoveHandles:_renderActiveMoveMeasurement`, `FloatingValueInput`), and the
//! number in it.

use glam::{Vec2, Vec3};

/// `conciseNumberFormat`, legacy: the fewest decimals that show `value` to
/// within a thousandth — `"0"`, `"4"`, `"2.5"`, `"0.125"`.
pub(crate) fn concise(value: f32) -> String {
    let within = |places: i32| {
        let step = 10f32.powi(places);
        (value - (value * step).round() / step).abs() < 0.001
    };
    if within(0) {
        // Luau's `%d` of a whole number; `as i64` also drops the sign of a
        // negative zero, as `%d` does.
        format!("{}", value.round() as i64)
    } else if within(1) {
        format!("{value:.1}")
    } else if within(2) {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

/// A Move arrow as the label placement sees it: its direction and where it
/// shows on screen, base and tip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Arrow {
    pub(crate) direction: Vec3,
    pub(crate) base: Vec2,
    pub(crate) tip: Vec2,
}

/// Which of `others` — every arrow but the dragged one and its opposite —
/// the label leans towards: the one that looks most square to the dragged
/// arrow on screen, and of two that look equally square (within a
/// thousandth), the one nearer the middle of the screen.
///
/// "Nearer the middle" is measured from the middle of the arrow as drawn.
/// [inferred: Studio compares its handles' own positions, which the
/// decompiled code does not show sit anywhere but at the pivot.]
pub(crate) fn lagging(dragged: Arrow, others: &[Arrow], middle: Vec2) -> Option<Arrow> {
    let along = (dragged.tip - dragged.base).normalize_or_zero();
    let square = |arrow: &Arrow| {
        (arrow.tip - arrow.base)
            .normalize_or_zero()
            .dot(along)
            .abs()
    };
    let from_middle = |arrow: &Arrow| ((arrow.base + arrow.tip) * 0.5 - middle).length();
    others.iter().copied().reduce(|best, arrow| {
        let (a, b) = (square(&arrow), square(&best));
        if (a - b).abs() < 0.001 {
            if from_middle(&arrow) < from_middle(&best) {
                arrow
            } else {
                best
            }
        } else if a < b {
            arrow
        } else {
            best
        }
    })
}

/// How far off the arrow the label stands, towards the lagging arrow, in
/// handle scales (`(3.5 + outset)*s`, halved; the legacy arrows have no
/// outset). A screen-constant distance, as Studio's is, so the label clears
/// the shaft however large this editor draws its arrows.
pub(crate) const LABEL_OFFSET: f32 = 3.5 * 0.5;

/// Where along its shaft the label sits, as a share of the shaft: where the
/// arrow was grabbed, held to the outer 60%.
const GRAB_RANGE: (f32, f32) = (0.4, 1.0);

/// Where the label goes for an arrow that starts at `shaft.0` and ends at
/// `shaft.1` studs out from `base` along `direction`, grabbed `grabbed` studs
/// out: beside the grab, `offset` studs off towards `lagging`.
///
/// Studio measures the grab against its own arrow (`0.6*s` out, `5*s` long);
/// here it is measured against this editor's, so the label stays beside the
/// shaft actually drawn.
pub(crate) fn move_label(
    base: Vec3,
    direction: Vec3,
    shaft: (f32, f32),
    grabbed: f32,
    lagging: Vec3,
    offset: f32,
) -> Vec3 {
    let (start, end) = shaft;
    let length = end - start;
    let share = ((grabbed - start) / length).clamp(GRAB_RANGE.0, GRAB_RANGE.1);
    base + direction * (start + share * length) + lagging * offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_numbers_read_bare() {
        assert_eq!(concise(0.0), "0");
        assert_eq!(concise(4.0), "4");
        assert_eq!(concise(-3.0004), "-3");
        assert_eq!(concise(-0.0002), "0");
        assert_eq!(concise(12.9996), "13");
    }

    #[test]
    fn fractions_read_with_the_fewest_places_that_hold_them() {
        assert_eq!(concise(2.5), "2.5");
        assert_eq!(concise(-0.25), "-0.25");
        assert_eq!(concise(0.125), "0.125");
        assert_eq!(concise(1.23456), "1.235");
        assert_eq!(concise(0.1004), "0.1");
    }

    fn arrow(direction: Vec3, base: Vec2, tip: Vec2) -> Arrow {
        Arrow {
            direction,
            base,
            tip,
        }
    }

    #[test]
    fn the_label_leans_towards_the_arrow_most_square_to_the_drag_on_screen() {
        let dragged = arrow(Vec3::X, Vec2::new(100.0, 100.0), Vec2::new(200.0, 110.0));
        let up = arrow(Vec3::Y, Vec2::new(100.0, 100.0), Vec2::new(100.0, 0.0));
        let slanted = arrow(Vec3::Z, Vec2::new(100.0, 100.0), Vec2::new(40.0, 140.0));
        let picked = lagging(dragged, &[slanted, up], Vec2::new(300.0, 200.0)).unwrap();
        assert_eq!(picked.direction, Vec3::Y);
    }

    #[test]
    fn of_two_equally_square_arrows_the_one_nearer_the_middle_wins() {
        let dragged = arrow(Vec3::X, Vec2::new(100.0, 300.0), Vec2::new(200.0, 300.0));
        let up = arrow(Vec3::Y, Vec2::new(100.0, 300.0), Vec2::new(100.0, 200.0));
        let down = arrow(
            Vec3::NEG_Y,
            Vec2::new(100.0, 300.0),
            Vec2::new(100.0, 400.0),
        );
        let middle = Vec2::new(300.0, 150.0);
        assert_eq!(
            lagging(dragged, &[down, up], middle).unwrap().direction,
            Vec3::Y
        );
        assert_eq!(
            lagging(dragged, &[up, down], middle).unwrap().direction,
            Vec3::Y
        );
    }

    #[test]
    fn the_label_sits_where_the_shaft_was_grabbed_and_off_to_one_side() {
        // A 10-stud arrow whose shaft starts 2 studs out, grabbed at 8.
        let at = move_label(Vec3::ZERO, Vec3::X, (2.0, 10.0), 8.0, Vec3::Y, 1.5);
        assert!((at.x - 8.0).abs() < 1e-5);
        assert!((at.y - 1.5).abs() < 1e-5);
        // Grabbed near the pivot, it still sits 40% of the way out.
        let near = move_label(Vec3::ZERO, Vec3::X, (2.0, 10.0), 2.5, Vec3::Y, 1.5);
        assert!((near.x - (2.0 + 0.4 * 8.0)).abs() < 1e-5);
    }
}
