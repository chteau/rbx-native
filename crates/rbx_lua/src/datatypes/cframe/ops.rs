//! Extended `CFrame` API: constructors, space conversions, angle and
//! quaternion helpers. Rotation matrices are row-major with the axes as columns.

use mlua::{
    Lua, MetaMethod, MultiValue, Result, Table, UserDataFields, UserDataMethods, Value, Variadic,
};
use rbx_dom::{CFrameData, Vector3Data};

use super::{LuaCFrame, IDENTITY};
use crate::datatypes::vector3::LuaVector3;
use crate::datatypes::{from_userdata, number_arg};

fn v3(x: f32, y: f32, z: f32) -> Vector3Data {
    Vector3Data { x, y, z }
}

fn dot(a: Vector3Data, b: Vector3Data) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: Vector3Data, b: Vector3Data) -> Vector3Data {
    v3(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn scale(a: Vector3Data, s: f32) -> Vector3Data {
    v3(a.x * s, a.y * s, a.z * s)
}

fn sub(a: Vector3Data, b: Vector3Data) -> Vector3Data {
    v3(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn unit(a: Vector3Data) -> Option<Vector3Data> {
    let m = dot(a, a).sqrt();
    (m > 1e-12).then(|| scale(a, 1.0 / m))
}

fn from_columns(
    position: Vector3Data,
    x: Vector3Data,
    y: Vector3Data,
    z: Vector3Data,
) -> LuaCFrame {
    LuaCFrame(CFrameData {
        position,
        rotation: [x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z],
    })
}

fn column(c: &LuaCFrame, i: usize) -> Vector3Data {
    let r = &c.0.rotation;
    v3(r[i], r[3 + i], r[6 + i])
}

fn look_at(at: Vector3Data, target: Vector3Data, up: Vector3Data) -> LuaCFrame {
    let Some(back) = unit(sub(at, target)) else {
        return LuaCFrame::at(at);
    };
    let right = unit(cross(up, back))
        .unwrap_or_else(|| v3(0.0, 0.0, if back.y > 0.0 { -1.0 } else { 1.0 }));
    let new_up = cross(back, right);
    from_columns(at, right, new_up, back)
}

fn quat_to_matrix(q: [f32; 4], position: Vector3Data) -> LuaCFrame {
    let n = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    let [x, y, z, w] = if n > 1e-12 {
        q.map(|c| c / n)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    LuaCFrame(CFrameData {
        position,
        rotation: [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    })
}

fn matrix_to_quat(c: &LuaCFrame) -> [f32; 4] {
    let m = &c.0.rotation;
    let t = m[0] + m[4] + m[8];
    if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        [
            (m[7] - m[5]) / s,
            (m[2] - m[6]) / s,
            (m[3] - m[1]) / s,
            s / 4.0,
        ]
    } else if m[0] > m[4] && m[0] > m[8] {
        let s = (1.0 + m[0] - m[4] - m[8]).sqrt() * 2.0;
        [
            s / 4.0,
            (m[1] + m[3]) / s,
            (m[2] + m[6]) / s,
            (m[7] - m[5]) / s,
        ]
    } else if m[4] > m[8] {
        let s = (1.0 + m[4] - m[0] - m[8]).sqrt() * 2.0;
        [
            (m[1] + m[3]) / s,
            s / 4.0,
            (m[5] + m[7]) / s,
            (m[2] - m[6]) / s,
        ]
    } else {
        let s = (1.0 + m[8] - m[0] - m[4]).sqrt() * 2.0;
        [
            (m[2] + m[6]) / s,
            (m[5] + m[7]) / s,
            s / 4.0,
            (m[3] - m[1]) / s,
        ]
    }
}

fn from_axis_angle(axis: Vector3Data, angle: f32) -> LuaCFrame {
    let axis = unit(axis).unwrap_or(v3(1.0, 0.0, 0.0));
    let (s, c) = (angle / 2.0).sin_cos();
    quat_to_matrix([axis.x * s, axis.y * s, axis.z * s, c], v3(0.0, 0.0, 0.0))
}

fn to_axis_angle(c: &LuaCFrame) -> (Vector3Data, f32) {
    let mut q = matrix_to_quat(c);
    if q[3] < 0.0 {
        q = q.map(|v| -v);
    }
    let w = q[3].clamp(-1.0, 1.0);
    let s = (1.0 - w * w).sqrt();
    if s < 1e-6 {
        (v3(1.0, 0.0, 0.0), 0.0)
    } else {
        (v3(q[0] / s, q[1] / s, q[2] / s), 2.0 * w.acos())
    }
}

fn to_euler_xyz(c: &LuaCFrame) -> (f32, f32, f32) {
    let r = &c.0.rotation;
    let ry = r[2].clamp(-1.0, 1.0).asin();
    if r[2].abs() < 0.99999 {
        ((-r[5]).atan2(r[8]), ry, (-r[1]).atan2(r[0]))
    } else {
        (r[7].atan2(r[4]), ry, 0.0)
    }
}

fn lerp(a: &LuaCFrame, b: &LuaCFrame, t: f32) -> LuaCFrame {
    let (pa, pb) = (a.0.position, b.0.position);
    let position = v3(
        pa.x + (pb.x - pa.x) * t,
        pa.y + (pb.y - pa.y) * t,
        pa.z + (pb.z - pa.z) * t,
    );
    let qa = matrix_to_quat(a);
    let mut qb = matrix_to_quat(b);
    let mut d: f32 = qa.iter().zip(&qb).map(|(x, y)| x * y).sum();
    if d < 0.0 {
        qb = qb.map(|v| -v);
        d = -d;
    }
    let (wa, wb) = if d > 0.9995 {
        (1.0 - t, t)
    } else {
        let theta = d.clamp(-1.0, 1.0).acos();
        let s = theta.sin();
        (((1.0 - t) * theta).sin() / s, (t * theta).sin() / s)
    };
    let q = [0, 1, 2, 3].map(|i| qa[i] * wa + qb[i] * wb);
    quat_to_matrix(q, position)
}

fn orthonormalize(c: &LuaCFrame) -> LuaCFrame {
    let x = unit(column(c, 0)).unwrap_or(v3(1.0, 0.0, 0.0));
    let y0 = column(c, 1);
    let y = unit(sub(y0, scale(x, dot(x, y0)))).unwrap_or_else(|| {
        let helper = if x.y.abs() < 0.9 {
            v3(0.0, 1.0, 0.0)
        } else {
            v3(0.0, 0.0, 1.0)
        };
        unit(sub(helper, scale(x, dot(x, helper)))).unwrap_or(v3(0.0, 1.0, 0.0))
    });
    from_columns(c.0.position, x, y, cross(x, y))
}

fn components(c: &LuaCFrame) -> Vec<f32> {
    let p = c.0.position;
    let mut out = vec![p.x, p.y, p.z];
    out.extend_from_slice(&c.0.rotation);
    out
}

fn vec_arg(value: &Value) -> Result<Vector3Data> {
    Ok(from_userdata::<LuaVector3>(value, "Vector3")?.0)
}

fn num_args(args: &[Value]) -> Result<Vec<f32>> {
    args.iter()
        .map(|v| number_arg(v).ok_or_else(|| mlua::Error::runtime("CFrame.new expected numbers")))
        .collect()
}

fn construct(args: Vec<Value>) -> Result<LuaCFrame> {
    let is_vec = |v: &Value| matches!(v, Value::UserData(_));
    match args.len() {
        0 => Ok(LuaCFrame::at(v3(0.0, 0.0, 0.0))),
        1 => Ok(LuaCFrame::at(vec_arg(&args[0])?)),
        2 if is_vec(&args[0]) => Ok(look_at(
            vec_arg(&args[0])?,
            vec_arg(&args[1])?,
            v3(0.0, 1.0, 0.0),
        )),
        3 => {
            let n = num_args(&args)?;
            Ok(LuaCFrame::at(v3(n[0], n[1], n[2])))
        }
        7 => {
            let n = num_args(&args)?;
            Ok(quat_to_matrix(
                [n[3], n[4], n[5], n[6]],
                v3(n[0], n[1], n[2]),
            ))
        }
        12 => {
            let n = num_args(&args)?;
            let mut rotation = IDENTITY;
            rotation.copy_from_slice(&n[3..]);
            Ok(LuaCFrame(CFrameData {
                position: v3(n[0], n[1], n[2]),
                rotation,
            }))
        }
        _ => Err(mlua::Error::runtime(
            "CFrame.new called with an invalid argument list",
        )),
    }
}

pub(super) fn register(lua: &Lua, table: &Table) -> Result<()> {
    table.set(
        "new",
        lua.create_function(|_, args: MultiValue| construct(args.into_iter().collect()))?,
    )?;
    table.set(
        "lookAt",
        lua.create_function(
            |_, (at, target, up): (LuaVector3, LuaVector3, Option<LuaVector3>)| {
                Ok(look_at(
                    at.0,
                    target.0,
                    up.map_or(v3(0.0, 1.0, 0.0), |u| u.0),
                ))
            },
        )?,
    )?;
    table.set(
        "fromMatrix",
        lua.create_function(
            |_, (pos, vx, vy, vz): (LuaVector3, LuaVector3, LuaVector3, Option<LuaVector3>)| {
                let z = match vz {
                    Some(z) => z.0,
                    None => unit(cross(vx.0, vy.0)).unwrap_or(v3(0.0, 0.0, 1.0)),
                };
                Ok(from_columns(pos.0, vx.0, vy.0, z))
            },
        )?,
    )?;
    let orientation = lua.create_function(|_, (rx, ry, rz): (f32, f32, f32)| {
        Ok(LuaCFrame::from_euler_angles_yxz(rx, ry, rz))
    })?;
    table.set("fromOrientation", orientation.clone())?;
    table.set("fromEulerAnglesYXZ", orientation)?;
    table.set(
        "fromAxisAngle",
        lua.create_function(|_, (axis, angle): (LuaVector3, f32)| {
            Ok(from_axis_angle(axis.0, angle))
        })?,
    )?;
    Ok(())
}

pub(super) fn add_fields<F: UserDataFields<LuaCFrame>>(fields: &mut F) {
    fields.add_field_method_get("Rotation", |_, this| {
        Ok(LuaCFrame(CFrameData {
            position: v3(0.0, 0.0, 0.0),
            rotation: this.0.rotation,
        }))
    });
    for (name, i) in [("XVector", 0), ("YVector", 1), ("ZVector", 2)] {
        fields.add_field_method_get(name, move |_, this| Ok(LuaVector3(column(this, i))));
    }
}

pub(super) fn add_methods<M: UserDataMethods<LuaCFrame>>(methods: &mut M) {
    methods.add_method("ToWorldSpace", |_, this, args: Variadic<LuaCFrame>| {
        Ok(args
            .iter()
            .map(|c| this.compose(c))
            .collect::<Variadic<_>>())
    });
    methods.add_method("ToObjectSpace", |_, this, args: Variadic<LuaCFrame>| {
        let inv = this.inverse();
        Ok(args.iter().map(|c| inv.compose(c)).collect::<Variadic<_>>())
    });
    methods.add_method(
        "PointToWorldSpace",
        |_, this, args: Variadic<LuaVector3>| {
            Ok(args
                .iter()
                .map(|v| LuaVector3(this.transform(v.0)))
                .collect::<Variadic<_>>())
        },
    );
    methods.add_method(
        "PointToObjectSpace",
        |_, this, args: Variadic<LuaVector3>| {
            let inv = this.inverse();
            Ok(args
                .iter()
                .map(|v| LuaVector3(inv.transform(v.0)))
                .collect::<Variadic<_>>())
        },
    );
    methods.add_method(
        "VectorToWorldSpace",
        |_, this, args: Variadic<LuaVector3>| {
            Ok(args
                .iter()
                .map(|v| LuaVector3(this.rotate(v.0)))
                .collect::<Variadic<_>>())
        },
    );
    methods.add_method(
        "VectorToObjectSpace",
        |_, this, args: Variadic<LuaVector3>| {
            let inv = this.inverse();
            Ok(args
                .iter()
                .map(|v| LuaVector3(inv.rotate(v.0)))
                .collect::<Variadic<_>>())
        },
    );
    for name in ["GetComponents", "components"] {
        methods.add_method(name, |_, this, ()| {
            Ok(components(this).into_iter().collect::<Variadic<f32>>())
        });
    }
    methods.add_method("ToEulerAnglesXYZ", |_, this, ()| Ok(to_euler_xyz(this)));
    for name in ["ToEulerAnglesYXZ", "ToOrientation"] {
        methods.add_method(name, |_, this, ()| Ok(this.to_euler_angles_yxz()));
    }
    methods.add_method("ToAxisAngle", |_, this, ()| {
        let (axis, angle) = to_axis_angle(this);
        Ok((LuaVector3(axis), angle))
    });
    methods.add_method("Lerp", |_, this, (goal, alpha): (LuaCFrame, f32)| {
        Ok(lerp(this, &goal, alpha))
    });
    methods.add_method("Orthonormalize", |_, this, ()| Ok(orthonormalize(this)));
    methods.add_method(
        "FuzzyEq",
        |_, this, (other, eps): (LuaCFrame, Option<f32>)| {
            let eps = eps.unwrap_or(1e-5);
            Ok(components(this)
                .iter()
                .zip(components(&other))
                .all(|(a, b)| (a - b).abs() <= eps))
        },
    );
    methods.add_meta_method(MetaMethod::Add, |_, this, v: LuaVector3| {
        Ok(translate(this, v.0, 1.0))
    });
    methods.add_meta_method(MetaMethod::Sub, |_, this, v: LuaVector3| {
        Ok(translate(this, v.0, -1.0))
    });
}

fn translate(c: &LuaCFrame, v: Vector3Data, sign: f32) -> LuaCFrame {
    let p = c.0.position;
    LuaCFrame(CFrameData {
        position: v3(p.x + sign * v.x, p.y + sign * v.y, p.z + sign * v.z),
        rotation: c.0.rotation,
    })
}
