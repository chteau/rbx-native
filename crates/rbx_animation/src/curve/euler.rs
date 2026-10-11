//! `Enum.RotationOrder` and the Euler maths an `EulerRotationCurve` needs.
//! Angles are always held as (about X, about Y, about Z) whatever the order;
//! the order only says how the three turns compose.

use std::f32::consts::{PI, TAU};

use glam::{EulerRot, Quat, Vec3};

/// The order is the left-to-right matrix product, so `XYZ` is `Rx·Ry·Rz`,
/// the same as `CFrame.Angles` and [`Frame::angles`](crate::frame::Frame::angles).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RotationOrder {
    #[default]
    XYZ,
    XZY,
    YZX,
    YXZ,
    ZXY,
    ZYX,
}

impl RotationOrder {
    /// In the enum's own value order, which is also the editor's menu order.
    pub const ALL: [RotationOrder; 6] = [
        RotationOrder::XYZ,
        RotationOrder::XZY,
        RotationOrder::YZX,
        RotationOrder::YXZ,
        RotationOrder::ZXY,
        RotationOrder::ZYX,
    ];

    pub fn value(self) -> u32 {
        match self {
            RotationOrder::XYZ => 0,
            RotationOrder::XZY => 1,
            RotationOrder::YZX => 2,
            RotationOrder::YXZ => 3,
            RotationOrder::ZXY => 4,
            RotationOrder::ZYX => 5,
        }
    }

    pub fn from_value(value: u32) -> RotationOrder {
        RotationOrder::ALL
            .into_iter()
            .find(|o| o.value() == value)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            RotationOrder::XYZ => "XYZ",
            RotationOrder::XZY => "XZY",
            RotationOrder::YZX => "YZX",
            RotationOrder::YXZ => "YXZ",
            RotationOrder::ZXY => "ZXY",
            RotationOrder::ZYX => "ZYX",
        }
    }

    /// glam's intrinsic `EulerRot` of the same name is the same product, with
    /// its three angles given in sequence order rather than X, Y, Z.
    fn glam(self) -> (EulerRot, [usize; 3]) {
        match self {
            RotationOrder::XYZ => (EulerRot::XYZ, [0, 1, 2]),
            RotationOrder::XZY => (EulerRot::XZY, [0, 2, 1]),
            RotationOrder::YZX => (EulerRot::YZX, [1, 2, 0]),
            RotationOrder::YXZ => (EulerRot::YXZ, [1, 0, 2]),
            RotationOrder::ZXY => (EulerRot::ZXY, [2, 0, 1]),
            RotationOrder::ZYX => (EulerRot::ZYX, [2, 1, 0]),
        }
    }

    /// `CFrame.fromEulerAngles(x, y, z, order)`.
    pub fn compose(self, angles: Vec3) -> Quat {
        let (rot, axes) = self.glam();
        let a = angles.to_array();
        Quat::from_euler(rot, a[axes[0]], a[axes[1]], a[axes[2]])
    }

    /// One (X, Y, Z) triple that [`compose`](Self::compose)s back to
    /// `rotation`, each angle in -π..π.
    pub fn decompose(self, rotation: Quat) -> Vec3 {
        let (rot, axes) = self.glam();
        let (p, q, r) = rotation.normalize().to_euler(rot);
        let mut out = [0.; 3];
        out[axes[0]] = p;
        out[axes[1]] = q;
        out[axes[2]] = r;
        Vec3::from_array(out)
    }

    /// The triple equivalent to `angles` (the same rotation) that lies
    /// nearest `near`, so a curve keyed from successive orientations moves
    /// continuously instead of jumping by 2π or flipping through the
    /// sequence's other solution.
    pub fn unwrap(self, angles: Vec3, near: Vec3) -> Vec3 {
        let (_, axes) = self.glam();
        // Every Tait-Bryan sequence has a second solution: the outer turns
        // plus π and the middle one mirrored about π/2.
        let mut flipped = angles.to_array();
        flipped[axes[0]] += PI;
        flipped[axes[1]] = PI - flipped[axes[1]];
        flipped[axes[2]] += PI;
        let candidates = [angles, Vec3::from_array(flipped)].map(|c| nearest_turns(c, near));
        if candidates[0].distance_squared(near) <= candidates[1].distance_squared(near) {
            candidates[0]
        } else {
            candidates[1]
        }
    }
}

/// Each angle shifted by whole turns to sit within π of the reference.
fn nearest_turns(angles: Vec3, near: Vec3) -> Vec3 {
    let shift = |a: f32, n: f32| a + TAU * ((n - a) / TAU).round();
    Vec3::new(
        shift(angles.x, near.x),
        shift(angles.y, near.y),
        shift(angles.z, near.z),
    )
}
