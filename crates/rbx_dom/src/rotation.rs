//! The compressed axis-aligned rotations Roblox's formats share.
//!
//! A `CFrame` whose rotation is a multiple of 90 degrees about each axis is
//! written as one byte instead of nine floats — in the binary format's
//! `CFrame` property and in the attribute blob's `CFrame` alike. The table
//! lives here, once, because both readers and both writers must agree on it
//! exactly: a disagreement does not fail, it silently turns a part.

/// Roblox's `NormalId` order: +X, +Y, +Z, -X, -Y, -Z.
const AXES: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [-1.0, 0.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, -1.0],
];

/// The rotation id that says "no compressed form: nine raw floats follow".
pub const RAW_ROTATION_ID: u8 = 0;

/// No rotation, as the row-major matrix `CFrameData::rotation` holds.
pub const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// Reconstructs an axis-aligned 3×3 rotation matrix from a compressed byte
/// id.
///
/// `id - 1` is split into the `NormalId` of the right vector (first column)
/// and of the up vector (second column); the back vector is their cross
/// product. `None` if the two axes collide (which is invalid), since only 24
/// of the 36 possible combinations are valid.
pub fn basic_rotation(id: u8) -> Option<[f32; 9]> {
    let index = usize::from(id.checked_sub(1)?);
    let (right_axis, up_axis) = (index / 6, index % 6);
    if right_axis >= AXES.len() || up_axis >= AXES.len() || right_axis % 3 == up_axis % 3 {
        return None;
    }

    let right = AXES[right_axis];
    let up = AXES[up_axis];
    let back = [
        right[1] * up[2] - right[2] * up[1],
        right[2] * up[0] - right[0] * up[2],
        right[0] * up[1] - right[1] * up[0],
    ];

    // Stored row-major, with the three basis vectors as columns.
    Some([
        right[0], up[0], back[0], right[1], up[1], back[1], right[2], up[2], back[2],
    ])
}

/// The inverse of [`basic_rotation`]: the compressed id for `matrix`, or
/// `None` if it is not one of the 24 axis-aligned rotations exactly. A linear
/// scan, since it runs once per written `CFrame` over a table of 36.
pub fn basic_rotation_id(matrix: &[f32; 9]) -> Option<u8> {
    (1u8..=36).find(|&id| basic_rotation(id).is_some_and(|candidate| candidate == *matrix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_two_is_the_identity() {
        assert_eq!(basic_rotation(2), Some(IDENTITY));
    }

    #[test]
    fn id_three_is_a_quarter_turn_around_x() {
        assert_eq!(
            basic_rotation(3),
            Some([1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0])
        );
    }

    #[test]
    fn colliding_axes_are_rejected() {
        // id 1 -> right = +X, up = +X; id 4 -> right = +X, up = -X.
        assert_eq!(basic_rotation(1), None);
        assert_eq!(basic_rotation(4), None);
        assert_eq!(basic_rotation(0x24), None);
    }

    #[test]
    fn every_valid_id_is_orthonormal_and_right_handed() {
        assert_eq!((1u8..=36).filter_map(basic_rotation).count(), 24);

        for id in 1u8..=36 {
            let Some(m) = basic_rotation(id) else {
                continue;
            };
            let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
                + m[2] * (m[3] * m[7] - m[4] * m[6]);
            assert!((det - 1.0).abs() < 1e-6, "id {id} has determinant {det}");
        }
    }

    #[test]
    fn every_valid_id_survives_a_round_trip_through_its_matrix() {
        for id in (1u8..=36).filter(|&id| basic_rotation(id).is_some()) {
            assert_eq!(basic_rotation_id(&basic_rotation(id).unwrap()), Some(id));
        }
    }

    #[test]
    fn identity_compresses_to_id_two() {
        assert_eq!(basic_rotation_id(&IDENTITY), Some(2));
    }

    #[test]
    fn a_non_axis_aligned_matrix_has_no_compressed_id() {
        let tilted = [0.9, 0.1, 0.0, -0.1, 0.9, 0.0, 0.0, 0.0, 1.0];
        assert_eq!(basic_rotation_id(&tilted), None);
    }
}
