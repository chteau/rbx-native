//! Rigid transforms as a quaternion and a translation: what a `CFrame` is once
//! scale and shear are ruled out, and the form keyframe interpolation wants
//! (slerp the rotation, lerp the position, the way `CFrame:Lerp` does).

use glam::{Mat3, Quat, Vec3};
use rbx_dom::{CFrameData, Vector3Data};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub rotation: Quat,
    pub position: Vec3,
}

impl Default for Frame {
    fn default() -> Self {
        Frame::IDENTITY
    }
}

impl Frame {
    pub const IDENTITY: Frame = Frame {
        rotation: Quat::IDENTITY,
        position: Vec3::ZERO,
    };

    pub fn new(position: Vec3, rotation: Quat) -> Frame {
        Frame { rotation, position }
    }

    pub fn at(position: Vec3) -> Frame {
        Frame::new(position, Quat::IDENTITY)
    }

    /// Roblox's `CFrame.Angles(x, y, z)`: X first, then Y, then Z, each about
    /// the frame's own (already turned) axes, so `Rx * Ry * Rz`.
    pub fn angles(x: f32, y: f32, z: f32) -> Frame {
        Frame::new(
            Vec3::ZERO,
            Quat::from_rotation_x(x) * Quat::from_rotation_y(y) * Quat::from_rotation_z(z),
        )
    }

    /// The inverse of [`Frame::angles`]: the `CFrame:ToEulerAnglesXYZ()` triple.
    pub fn to_angles(&self) -> Vec3 {
        let (x, y, z) = self.rotation.to_euler(glam::EulerRot::XYZ);
        Vec3::new(x, y, z)
    }

    /// Reads the row-major matrix a file stores. A matrix drifted off
    /// orthonormal (common after many saves) is re-orthonormalised by the
    /// quaternion conversion's normalisation rather than rejected.
    pub fn from_data(data: &CFrameData) -> Frame {
        let r = &data.rotation;
        let m = Mat3::from_cols(
            Vec3::new(r[0], r[3], r[6]),
            Vec3::new(r[1], r[4], r[7]),
            Vec3::new(r[2], r[5], r[8]),
        );
        let p = data.position;
        Frame::new(Vec3::new(p.x, p.y, p.z), Quat::from_mat3(&m).normalize())
    }

    pub fn to_data(&self) -> CFrameData {
        let m = Mat3::from_quat(self.rotation);
        let (c0, c1, c2) = (m.x_axis, m.y_axis, m.z_axis);
        CFrameData {
            position: Vector3Data {
                x: self.position.x,
                y: self.position.y,
                z: self.position.z,
            },
            rotation: [c0.x, c1.x, c2.x, c0.y, c1.y, c2.y, c0.z, c1.z, c2.z],
        }
    }

    pub fn mul(&self, other: &Frame) -> Frame {
        Frame::new(
            self.position + self.rotation * other.position,
            (self.rotation * other.rotation).normalize(),
        )
    }

    pub fn inverse(&self) -> Frame {
        let inv = self.rotation.conjugate();
        Frame::new(-(inv * self.position), inv)
    }

    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        self.position + self.rotation * point
    }

    /// `CFrame:Lerp`: shortest-arc slerp of the rotation, straight lerp of the
    /// position. `alpha` outside 0..1 extrapolates, which the overshooting
    /// easing styles (Elastic, Bounce's rebound) rely on.
    pub fn lerp(&self, other: &Frame, alpha: f32) -> Frame {
        let mut to = other.rotation;
        if self.rotation.dot(to) < 0. {
            to = -to;
        }
        let rotation = if (0. ..=1.).contains(&alpha) {
            self.rotation.slerp(to, alpha)
        } else {
            // glam's slerp clamps nothing but takes its shortcut through
            // nlerp near parallel; spell the extrapolation out instead.
            let delta = (self.rotation.conjugate() * to).normalize();
            let (axis, angle) = delta.to_axis_angle();
            self.rotation * Quat::from_axis_angle(axis, angle * alpha)
        };
        Frame::new(
            self.position.lerp(other.position, alpha),
            rotation.normalize(),
        )
    }

    pub fn approx_eq(&self, other: &Frame, epsilon: f32) -> bool {
        self.position.abs_diff_eq(other.position, epsilon)
            && (self.rotation.dot(other.rotation).abs() - 1.).abs() <= epsilon
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn data_round_trips() {
        let frame = Frame::new(Vec3::new(1., 2., 3.), Quat::from_rotation_y(0.7));
        assert!(Frame::from_data(&frame.to_data()).approx_eq(&frame, 1e-5));
    }

    #[test]
    fn row_major_matches_roblox_layout() {
        // CFrame.Angles(0, math.pi/2, 0) has R02 = 1 and R20 = -1.
        let data = Frame::angles(0., FRAC_PI_2, 0.).to_data();
        assert!((data.rotation[2] - 1.).abs() < 1e-6);
        assert!((data.rotation[6] + 1.).abs() < 1e-6);
    }

    #[test]
    fn inverse_cancels() {
        let frame = Frame::new(Vec3::new(4., -1., 2.), Quat::from_rotation_z(1.2));
        assert!(frame
            .mul(&frame.inverse())
            .approx_eq(&Frame::IDENTITY, 1e-5));
    }

    #[test]
    fn mul_applies_right_operand_in_left_space() {
        let turned = Frame::angles(0., FRAC_PI_2, 0.);
        let moved = turned.mul(&Frame::at(Vec3::X));
        // +X in a frame turned a quarter about Y points along world -Z.
        assert!(moved.position.abs_diff_eq(Vec3::new(0., 0., -1.), 1e-6));
    }

    #[test]
    fn lerp_halfway_and_extrapolates() {
        let a = Frame::IDENTITY;
        let b = Frame::new(Vec3::new(2., 0., 0.), Quat::from_rotation_y(1.));
        let half = a.lerp(&b, 0.5);
        assert!(half.position.abs_diff_eq(Vec3::X, 1e-6));
        assert!((half.rotation.angle_between(Quat::IDENTITY) - 0.5).abs() < 1e-5);
        let over = a.lerp(&b, 1.5);
        assert!((over.rotation.angle_between(Quat::IDENTITY) - 1.5).abs() < 1e-4);
    }

    #[test]
    fn angles_round_trip() {
        let frame = Frame::angles(0.3, -0.4, 0.5);
        assert!(frame
            .to_angles()
            .abs_diff_eq(Vec3::new(0.3, -0.4, 0.5), 1e-5));
    }
}
