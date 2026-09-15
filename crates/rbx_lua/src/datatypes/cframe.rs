//! `CFrame` datatype.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::{CFrameData, Vector3Data};

use super::from_userdata;
use super::vector3::LuaVector3;

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaCFrame(pub(crate) CFrameData);

impl LuaCFrame {
    fn at(position: Vector3Data) -> Self {
        LuaCFrame(CFrameData {
            position,
            rotation: IDENTITY,
        })
    }

    // Roblox applies the three rotations in Z, Y, X order, i.e. R = Rx · Ry · Rz.
    fn angles(rx: f32, ry: f32, rz: f32) -> Self {
        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();
        let rotation = [
            cy * cz,
            -cy * sz,
            sy,
            sx * sy * cz + cx * sz,
            -sx * sy * sz + cx * cz,
            -sx * cy,
            -cx * sy * cz + sx * sz,
            cx * sy * sz + sx * cz,
            cx * cy,
        ];
        LuaCFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation,
        })
    }

    fn rotate(&self, v: Vector3Data) -> Vector3Data {
        let r = &self.0.rotation;
        Vector3Data {
            x: r[0] * v.x + r[1] * v.y + r[2] * v.z,
            y: r[3] * v.x + r[4] * v.y + r[5] * v.z,
            z: r[6] * v.x + r[7] * v.y + r[8] * v.z,
        }
    }

    fn transform(&self, v: Vector3Data) -> Vector3Data {
        let rotated = self.rotate(v);
        Vector3Data {
            x: rotated.x + self.0.position.x,
            y: rotated.y + self.0.position.y,
            z: rotated.z + self.0.position.z,
        }
    }

    fn compose(&self, other: &LuaCFrame) -> LuaCFrame {
        let (a, b) = (&self.0.rotation, &other.0.rotation);
        let mut rotation = [0.0f32; 9];
        for row in 0..3 {
            for col in 0..3 {
                rotation[row * 3 + col] =
                    (0..3).map(|k| a[row * 3 + k] * b[k * 3 + col]).sum::<f32>();
            }
        }
        LuaCFrame(CFrameData {
            position: self.transform(other.0.position),
            rotation,
        })
    }

    /// Builds the rotation `BasePart.Orientation`'s setter uses: Roblox composes
    /// it Y, then X, then Z (`R = Ry · Rx · Rz`), unlike `CFrame.Angles`'s Z, Y, X.
    /// Reuses `angles`'s single-axis case (`rx`/`ry`/`rz` in radians).
    pub(crate) fn from_euler_angles_yxz(rx: f32, ry: f32, rz: f32) -> LuaCFrame {
        LuaCFrame::angles(0.0, ry, 0.0)
            .compose(&LuaCFrame::angles(rx, 0.0, 0.0))
            .compose(&LuaCFrame::angles(0.0, 0.0, rz))
    }

    /// Inverse of `from_euler_angles_yxz`: decomposes this rotation into (rx, ry,
    /// rz) radians, the convention `Orientation`'s getter reads back.
    pub(crate) fn to_euler_angles_yxz(self) -> (f32, f32, f32) {
        let r = &self.0.rotation;
        let sx = (-r[5]).clamp(-1.0, 1.0);
        let rx = sx.asin();
        if rx.cos().abs() > 1e-5 {
            (rx, r[2].atan2(r[8]), r[3].atan2(r[4]))
        } else {
            // Gimbal lock at rx = +/-90 deg: ry and rz aren't independently
            // observable, so fold everything into ry and leave rz at 0.
            (rx, (-r[6]).atan2(r[0]), 0.0)
        }
    }

    fn inverse(&self) -> LuaCFrame {
        let r = &self.0.rotation;
        // A rotation matrix is orthonormal, so its transpose is its inverse.
        let rotation = [r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]];
        let transposed = LuaCFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation,
        });
        let p = transposed.rotate(self.0.position);
        LuaCFrame(CFrameData {
            position: Vector3Data {
                x: -p.x,
                y: -p.y,
                z: -p.z,
            },
            rotation,
        })
    }
}

impl UserData for LuaCFrame {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("X", |_, this| Ok(this.0.position.x));
        fields.add_field_method_get("Y", |_, this| Ok(this.0.position.y));
        fields.add_field_method_get("Z", |_, this| Ok(this.0.position.z));
        fields.add_field_method_get("Position", |_, this| Ok(LuaVector3(this.0.position)));
        fields.add_field_method_get("LookVector", |_, this| {
            let r = &this.0.rotation;
            Ok(LuaVector3::new(-r[2], -r[5], -r[8]))
        });
        fields.add_field_method_get("RightVector", |_, this| {
            let r = &this.0.rotation;
            Ok(LuaVector3::new(r[0], r[3], r[6]))
        });
        fields.add_field_method_get("UpVector", |_, this| {
            let r = &this.0.rotation;
            Ok(LuaVector3::new(r[1], r[4], r[7]))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Inverse", |_, this, ()| Ok(this.inverse()));
        methods.add_method("PointToWorldSpace", |_, this, v: LuaVector3| {
            Ok(LuaVector3(this.transform(v.0)))
        });
        methods.add_meta_method(MetaMethod::Mul, |lua, this, rhs: Value| {
            if let Value::UserData(data) = &rhs {
                if let Ok(other) = data.borrow::<LuaCFrame>() {
                    return lua
                        .create_userdata(this.compose(&other))
                        .map(Value::UserData);
                }
            }
            let point: LuaVector3 = from_userdata(&rhs, "CFrame or Vector3")?;
            lua.create_userdata(LuaVector3(this.transform(point.0)))
                .map(Value::UserData)
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaCFrame| {
            Ok(*this == other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let p = this.0.position;
            let r = this.0.rotation;
            Ok(format!(
                "{}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}",
                p.x, p.y, p.z, r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8]
            ))
        });
    }
}

impl mlua::FromLua for LuaCFrame {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "CFrame")
    }
}

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            |_, (first, y, z): (Option<Value>, Option<f32>, Option<f32>)| match first {
                None | Some(Value::Nil) => Ok(LuaCFrame::at(Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                })),
                Some(Value::UserData(data)) => {
                    let position = data.borrow::<LuaVector3>()?.0;
                    Ok(LuaCFrame::at(position))
                }
                Some(other) => {
                    let x = super::number_arg(&other).ok_or_else(|| {
                        mlua::Error::runtime("CFrame.new expected a number or a Vector3")
                    })?;
                    Ok(LuaCFrame::at(Vector3Data {
                        x,
                        y: y.unwrap_or_default(),
                        z: z.unwrap_or_default(),
                    }))
                }
            },
        )?,
    )?;
    let angles =
        lua.create_function(|_, (rx, ry, rz): (f32, f32, f32)| Ok(LuaCFrame::angles(rx, ry, rz)))?;
    table.set("Angles", angles.clone())?;
    table.set("fromEulerAnglesXYZ", angles)?;
    table.set(
        "identity",
        LuaCFrame::at(Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
    )?;
    Ok(table)
}
