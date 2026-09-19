//! A `CFrame`'s rotation, as the three angles people actually type.
//!
//! The DOM stores a rotation as nine floats — a 3×3 matrix, row-major, `R[i]
//! [j]` at `rotation[i * 3 + j]`. Nobody edits a matrix. Roblox itself
//! presents the same thing as an `Orientation` of three degrees, and that is
//! what this converts to and from.
//!
//! **The order is `Y` then `X` then `Z`**, matching Roblox's own
//! `CFrame.fromOrientation`: `R = Ry(y) · Rx(x) · Rz(z)`. Extracting in a
//! different order produces angles that look plausible and put the part
//! somewhere else.
//!
//! # The lossy direction
//!
//! Nine numbers do not fit in three. Every rotation *matrix* has an Euler
//! triple, but the trip back is only exact if the matrix was orthonormal to
//! begin with — and a file written by another tool need not be. Worse, at
//! `x = ±90°` (gimbal lock) the `Y` and `Z` angles stop being separable and
//! an infinity of pairs describe the same rotation, so the round trip picks
//! one arbitrarily.
//!
//! So the panel never round-trips a rotation it was not asked to change —
//! see `edit::parse_cframe`, which keeps the stored matrix byte-identical
//! when only the position fields were edited. This module is pure maths and
//! has no opinion about that; it just has to be exact enough that the rule
//! is worth having.

/// `R = Ry(y) · Rx(x) · Rz(z)`, expanded once so both directions below can
/// be read against it:
///
/// ```text
/// ┌                                                             ┐
/// │  cy·cz + sy·sx·sz   -cy·sz + sy·sx·cz    sy·cx               │
/// │  cx·sz               cx·cz              -sx                  │
/// │ -sy·cz + cy·sx·sz    sy·sz + cy·sx·cz    cy·cx               │
/// └                                                             ┘
/// ```
const _: () = ();

/// The rotation's `X`, `Y`, `Z` angles in degrees, in Roblox's order.
pub(super) fn to_degrees(rotation: &[f32; 9]) -> [f32; 3] {
    let at = |row: usize, col: usize| rotation[row * 3 + col];

    // `R[1][2] = -sin(x)`, clamped because a matrix that is not quite
    // orthonormal can push this a hair outside asin's domain and yield NaN.
    let x = (-at(1, 2)).clamp(-1., 1.).asin();
    let cos_x = x.cos();

    // At gimbal lock `cos(x)` vanishes: `Y` and `Z` collapse into one
    // degree of freedom, so pin `Z` at zero and put the whole turn in `Y`,
    // which is the convention Roblox's own `ToOrientation` follows.
    let (y, z) = if cos_x.abs() < 1e-6 {
        // With `cos(x) = 0` the top-left 2×2 collapses to `cos(y ∓ z)` and
        // `±sin(y ∓ z)`, the sign following `sin(x)` — so folding `Z` into
        // `Y` means flipping that argument, not negating the result.
        ((at(0, 1) * x.signum()).atan2(at(0, 0)), 0.)
    } else {
        (at(0, 2).atan2(at(2, 2)), at(1, 0).atan2(at(1, 1)))
    };

    // `+ 0.` folds negative zero away: `asin(-0.0)` is `-0.0`, and an
    // unrotated part reading "-0, 0, 0" looks like a bug to anyone who sees
    // it. The two compare equal, so nothing downstream changes.
    [x.to_degrees(), y.to_degrees(), z.to_degrees()].map(|angle| angle + 0.)
}

/// The matrix those three angles describe.
pub(super) fn from_degrees(angles: [f32; 3]) -> [f32; 9] {
    let [x, y, z] = angles.map(f32::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();

    [
        cy * cz + sy * sx * sz,
        -cy * sz + sy * sx * cz,
        sy * cx,
        cx * sz,
        cx * cz,
        -sx,
        -sy * cz + cy * sx * sz,
        sy * sz + cy * sx * cz,
        cy * cx,
    ]
}

/// Whether two angle triples name the same orientation, to the precision
/// the panel displays them at.
///
/// This is what lets `parse_cframe` tell "the user edited the position" from
/// "the user edited the rotation": the fields always submit all six numbers,
/// so the only way to know whether the rotation was touched is to compare it
/// with what was shown.
pub(super) fn same_angles(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(a, b)| (a - b).abs() < ANGLE_EPSILON)
}

/// Half of the last digit the panel prints, so a value that round-trips
/// through the display unchanged compares equal.
const ANGLE_EPSILON: f32 = 5e-4;

#[cfg(test)]
#[path = "orientation/tests.rs"]
mod tests;
