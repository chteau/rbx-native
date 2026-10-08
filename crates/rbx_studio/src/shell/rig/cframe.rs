//! Just enough rigid-transform maths to lay a rig out through its joints:
//! `part1 = part0 * C0 * C1^-1`, the rule a `Motor6D`/`Weld` follows.

use rbx_dom::{CFrameData, Vector3Data};

pub(crate) type V3 = [f32; 3];

/// A position and a row-major rotation, the way `CFrameData` stores them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cf {
    pub(crate) r: [f32; 9],
    pub(crate) p: V3,
}

const IDENTITY: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];

impl Cf {
    pub(crate) const fn at(p: V3) -> Cf {
        Cf { r: IDENTITY, p }
    }

    pub(crate) const fn with(p: V3, r: [f32; 9]) -> Cf {
        Cf { r, p }
    }

    /// A quarter turn about X, which is how a hand's grip attachment sits.
    pub(crate) const fn quarter_x(p: V3) -> Cf {
        Cf {
            r: [1., 0., 0., 0., 0., -1., 0., 1., 0.],
            p,
        }
    }

    pub(crate) fn from_data(data: &CFrameData) -> Cf {
        Cf {
            r: data.rotation,
            p: [data.position.x, data.position.y, data.position.z],
        }
    }

    pub(crate) fn data(&self) -> CFrameData {
        CFrameData {
            position: vec3(self.p),
            rotation: self.r,
        }
    }

    pub(crate) fn mul(&self, other: &Cf) -> Cf {
        let (a, b) = (&self.r, &other.r);
        let mut r = [0.; 9];
        for row in 0..3 {
            for col in 0..3 {
                r[row * 3 + col] = (0..3).map(|k| a[row * 3 + k] * b[k * 3 + col]).sum();
            }
        }
        Cf {
            r,
            p: add(self.rotate(other.p), self.p),
        }
    }

    pub(crate) fn inverse(&self) -> Cf {
        let r = &self.r;
        let t = [r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]];
        let back = Cf { r: t, p: [0.; 3] }.rotate(self.p);
        Cf {
            r: t,
            p: [-back[0], -back[1], -back[2]],
        }
    }

    pub(crate) fn rotate(&self, v: V3) -> V3 {
        let r = &self.r;
        [
            r[0] * v[0] + r[1] * v[1] + r[2] * v[2],
            r[3] * v[0] + r[4] * v[1] + r[5] * v[2],
            r[6] * v[0] + r[7] * v[1] + r[8] * v[2],
        ]
    }

    /// Where this transform places `child`, relative to a parent that is
    /// joined to it by `c0` (on the parent) and `c1` (on the child).
    pub(crate) fn joined(&self, c0: &Cf, c1: &Cf) -> Cf {
        self.mul(c0).mul(&c1.inverse())
    }
}

pub(crate) fn vec3(v: V3) -> Vector3Data {
    Vector3Data {
        x: v[0],
        y: v[1],
        z: v[2],
    }
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
