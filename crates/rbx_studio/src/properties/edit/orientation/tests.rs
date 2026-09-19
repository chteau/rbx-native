use super::*;

const IDENTITY: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];

fn assert_angles(actual: [f32; 3], expected: [f32; 3]) {
    for (axis, (actual, expected)) in ["X", "Y", "Z"]
        .iter()
        .zip(actual.iter().zip(expected.iter()))
    {
        assert!(
            (actual - expected).abs() < 1e-3,
            "{axis}: {actual} != {expected} (in {actual:?} vs {expected:?})"
        );
    }
}

fn assert_matrix(actual: [f32; 9], expected: [f32; 9]) {
    for (index, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(
            (actual - expected).abs() < 1e-4,
            "term {index}: {actual} != {expected}"
        );
    }
}

#[test]
fn an_unrotated_part_reads_as_three_zeroes() {
    assert_angles(to_degrees(&IDENTITY), [0., 0., 0.]);
    assert_matrix(from_degrees([0., 0., 0.]), IDENTITY);
}

/// The single-axis cases, because getting the order wrong still produces
/// plausible-looking angles — it just puts the part somewhere else. Each of
/// these has an independently obvious matrix.
#[test]
fn a_quarter_turn_about_each_axis_is_the_matrix_it_should_be() {
    // +90° about Y: +X now points along -Z, +Z points along +X.
    assert_matrix(
        from_degrees([0., 90., 0.]),
        [0., 0., 1., 0., 1., 0., -1., 0., 0.],
    );
    // +90° about X: +Y points along +Z (R[1][2] = -sin x = -1).
    assert_matrix(
        from_degrees([90., 0., 0.]),
        [1., 0., 0., 0., 0., -1., 0., 1., 0.],
    );
    // +90° about Z: +X points along +Y.
    assert_matrix(
        from_degrees([0., 0., 90.]),
        [0., -1., 0., 1., 0., 0., 0., 0., 1.],
    );
}

/// The property that actually matters for editing: whatever is typed comes
/// back out. A handful of awkward triples rather than one tidy one.
#[test]
fn every_angle_triple_survives_the_round_trip() {
    let cases = [
        [30., 45., 60.],
        [-15., 170., -95.],
        [0., 180., 0.],
        [12.5, -37.25, 88.125],
        [-89., 0., 0.],
        [0., -179.9, 179.9],
    ];

    for angles in cases {
        let back = to_degrees(&from_degrees(angles));
        assert_matrix(from_degrees(back), from_degrees(angles));
    }
}

/// Gimbal lock: at `x = ±90°` the Y and Z rotations become the same degree
/// of freedom, so the angles that come back need not be the ones that went
/// in — but the *rotation* must still be the one that was asked for, and
/// the extraction must not produce NaN.
#[test]
fn a_gimbal_locked_rotation_still_round_trips_to_the_same_orientation() {
    for angles in [[90., 40., 0.], [-90., 0., 25.], [90., 33., 17.]] {
        let matrix = from_degrees(angles);
        let back = to_degrees(&matrix);

        assert!(
            back.iter().all(|angle| angle.is_finite()),
            "{back:?} is not a set of angles"
        );
        assert_matrix(from_degrees(back), matrix);
    }
}

/// A matrix from another tool need not be perfectly orthonormal, and `asin`
/// outside `[-1, 1]` is NaN. Clamping is the difference between a slightly
/// wrong angle and a property row full of `NaN`.
#[test]
fn a_matrix_that_is_not_quite_orthonormal_does_not_produce_nan() {
    let mut rotation = IDENTITY;
    rotation[5] = -1.0000004;

    let angles = to_degrees(&rotation);
    assert!(
        angles.iter().all(|angle| angle.is_finite()),
        "{angles:?} is not a set of angles"
    );
}

/// What `parse_cframe` leans on to tell an edited position from an edited
/// rotation.
#[test]
fn angles_compare_equal_at_the_precision_they_are_displayed_at() {
    assert!(same_angles([30., 45., 60.], [30., 45., 60.]));
    assert!(
        same_angles([30., 45., 60.], [30.0001, 45., 60.]),
        "a difference below the displayed precision is not an edit"
    );
    assert!(
        !same_angles([30., 45., 60.], [30.01, 45., 60.]),
        "a difference the panel can show is an edit"
    );
}
