//! The Roblox datatypes expressions may build and read: `Vector2`,
//! `Vector3`, `Color3` and `CFrame`, with the constructors, members and
//! arithmetic Roblox's expression docs list. No parameter holds one, so
//! they only live inside an expression, on the way to a number, bool or
//! string a property can take.

use glam::{DMat3, DVec2, DVec3};

use super::Val;

/// A `CFrame`: a position and a rotation whose columns are the right, up
/// and back vectors, as Roblox keeps them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CFrame {
    pub position: DVec3,
    pub rotation: DMat3,
}

impl CFrame {
    const IDENTITY: CFrame = CFrame {
        position: DVec3::ZERO,
        rotation: DMat3::IDENTITY,
    };

    fn mul(&self, other: &CFrame) -> CFrame {
        CFrame {
            position: self.position + self.rotation * other.position,
            rotation: self.rotation * other.rotation,
        }
    }

    fn inverse(&self) -> CFrame {
        let rotation = self.rotation.transpose();
        CFrame {
            position: -(rotation * self.position),
            rotation,
        }
    }

    /// `CFrame.new(at, lookAt)`: facing down −Z towards the target, world up
    /// kept where it can be.
    fn looking(at: DVec3, target: DVec3) -> CFrame {
        let look = (target - at).normalize_or_zero();
        if look == DVec3::ZERO {
            return CFrame {
                position: at,
                ..CFrame::IDENTITY
            };
        }
        let up = if look.cross(DVec3::Y).length_squared() < 1e-12 {
            DVec3::Z
        } else {
            DVec3::Y
        };
        let right = look.cross(up).normalize();
        let up = right.cross(look);
        CFrame {
            position: at,
            rotation: DMat3::from_cols(right, up, -look),
        }
    }
}

fn number(v: &Val) -> Result<f64, String> {
    v.number()
}

fn args<const N: usize>(a: &[Val]) -> Result<[f64; N], String> {
    let mut out = [0.; N];
    for (slot, v) in out.iter_mut().zip(a) {
        *slot = number(v)?;
    }
    Ok(out)
}

fn vector3(v: &Val) -> Result<DVec3, String> {
    match v {
        Val::Vector3(v) => Ok(*v),
        other => Err(format!("expected a Vector3, got {}", other.type_name())),
    }
}

/// `Library.member` read as a value (`Vector3.zero`, `CFrame.identity`) or
/// called with `args` (`Vector2.new(x, y)`).
pub(super) fn library(lib: &str, member: &str, call: Option<&[Val]>) -> Result<Val, String> {
    let a = call.unwrap_or(&[]);
    Ok(match (lib, member, call.is_some()) {
        ("Vector2", "new", true) => {
            let [x, y] = args(a)?;
            Val::Vector2(DVec2::new(x, y))
        }
        ("Vector2", "zero", false) => Val::Vector2(DVec2::ZERO),
        ("Vector2", "one", false) => Val::Vector2(DVec2::ONE),
        ("Vector2", "xAxis", false) => Val::Vector2(DVec2::X),
        ("Vector2", "yAxis", false) => Val::Vector2(DVec2::Y),
        ("Vector3", "new", true) => {
            let [x, y, z] = args(a)?;
            Val::Vector3(DVec3::new(x, y, z))
        }
        ("Vector3", "zero", false) => Val::Vector3(DVec3::ZERO),
        ("Vector3", "one", false) => Val::Vector3(DVec3::ONE),
        ("Vector3", "xAxis", false) => Val::Vector3(DVec3::X),
        ("Vector3", "yAxis", false) => Val::Vector3(DVec3::Y),
        ("Vector3", "zAxis", false) => Val::Vector3(DVec3::Z),
        ("Color3", "new", true) => {
            let [r, g, b] = args(a)?;
            Val::Color3([r, g, b])
        }
        ("Color3", "fromRGB", true) => {
            let [r, g, b] = args(a)?;
            Val::Color3([r / 255., g / 255., b / 255.])
        }
        ("Color3", "fromHSV", true) => {
            let [h, s, v] = args(a)?;
            Val::Color3(hsv(h, s, v))
        }
        ("CFrame", "identity", false) => Val::CFrame(CFrame::IDENTITY),
        ("CFrame", "new", true) => Val::CFrame(match a {
            [] => CFrame::IDENTITY,
            [at] => CFrame {
                position: vector3(at)?,
                ..CFrame::IDENTITY
            },
            [at, target] => CFrame::looking(vector3(at)?, vector3(target)?),
            _ => {
                let [x, y, z] = args(a)?;
                CFrame {
                    position: DVec3::new(x, y, z),
                    ..CFrame::IDENTITY
                }
            }
        }),
        ("CFrame", "Angles" | "fromEulerAnglesXYZ", true) => {
            let [x, y, z] = args(a)?;
            Val::CFrame(CFrame {
                position: DVec3::ZERO,
                rotation: DMat3::from_rotation_x(x)
                    * DMat3::from_rotation_y(y)
                    * DMat3::from_rotation_z(z),
            })
        }
        _ => return Err(format!("{lib}.{member} is not available in expressions")),
    })
}

fn hsv(h: f64, s: f64, v: f64) -> [f64; 3] {
    let h = h.rem_euclid(1.) * 6.;
    let (i, f) = (h.floor(), h.fract());
    let (p, q, t) = (v * (1. - s), v * (1. - s * f), v * (1. - s * (1. - f)));
    match i as u8 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// `value.name`.
pub(super) fn member(value: &Val, name: &str) -> Result<Val, String> {
    let missing = || format!("{name} is not a valid member of {}", value.type_name());
    Ok(match (value, name) {
        (Val::Vector2(v), "X") => Val::Number(v.x),
        (Val::Vector2(v), "Y") => Val::Number(v.y),
        (Val::Vector2(v), "Magnitude") => Val::Number(v.length()),
        (Val::Vector2(v), "Unit") => Val::Vector2(v.normalize_or_zero()),
        (Val::Vector3(v), "X") => Val::Number(v.x),
        (Val::Vector3(v), "Y") => Val::Number(v.y),
        (Val::Vector3(v), "Z") => Val::Number(v.z),
        (Val::Vector3(v), "Magnitude") => Val::Number(v.length()),
        (Val::Vector3(v), "Unit") => Val::Vector3(v.normalize_or_zero()),
        (Val::Color3(c), "R") => Val::Number(c[0]),
        (Val::Color3(c), "G") => Val::Number(c[1]),
        (Val::Color3(c), "B") => Val::Number(c[2]),
        (Val::CFrame(c), "Position" | "p") => Val::Vector3(c.position),
        (Val::CFrame(c), "X") => Val::Number(c.position.x),
        (Val::CFrame(c), "Y") => Val::Number(c.position.y),
        (Val::CFrame(c), "Z") => Val::Number(c.position.z),
        (Val::CFrame(c), "Rotation") => Val::CFrame(CFrame {
            position: DVec3::ZERO,
            rotation: c.rotation,
        }),
        (Val::CFrame(c), "RightVector" | "XVector") => Val::Vector3(c.rotation.x_axis),
        (Val::CFrame(c), "UpVector" | "YVector") => Val::Vector3(c.rotation.y_axis),
        (Val::CFrame(c), "LookVector") => Val::Vector3(-c.rotation.z_axis),
        (Val::CFrame(c), "ZVector") => Val::Vector3(c.rotation.z_axis),
        _ => return Err(missing()),
    })
}

/// `value:name(args)`.
pub(super) fn method(value: &Val, name: &str, a: &[Val]) -> Result<Val, String> {
    let other = a.first().cloned().unwrap_or(Val::Nil);
    let alpha = || a.get(1).map_or(Ok(0.), number);
    Ok(match (value, name, &other) {
        (Val::Vector2(v), "Dot", Val::Vector2(w)) => Val::Number(v.dot(*w)),
        (Val::Vector2(v), "Cross", Val::Vector2(w)) => Val::Number(v.perp_dot(*w)),
        (Val::Vector2(v), "Lerp", Val::Vector2(w)) => Val::Vector2(v.lerp(*w, alpha()?)),
        (Val::Vector3(v), "Dot", Val::Vector3(w)) => Val::Number(v.dot(*w)),
        (Val::Vector3(v), "Cross", Val::Vector3(w)) => Val::Vector3(v.cross(*w)),
        (Val::Vector3(v), "Lerp", Val::Vector3(w)) => Val::Vector3(v.lerp(*w, alpha()?)),
        (Val::Color3(c), "Lerp", Val::Color3(d)) => {
            let t = alpha()?;
            Val::Color3([0, 1, 2].map(|i| c[i] + (d[i] - c[i]) * t))
        }
        (Val::CFrame(c), "Inverse", _) => Val::CFrame(c.inverse()),
        (Val::CFrame(c), "PointToWorldSpace", Val::Vector3(p)) => {
            Val::Vector3(c.position + c.rotation * *p)
        }
        (Val::CFrame(c), "PointToObjectSpace", Val::Vector3(p)) => {
            let inv = c.inverse();
            Val::Vector3(inv.position + inv.rotation * *p)
        }
        _ => {
            return Err(format!(
                "{name} is not a valid method of {}",
                value.type_name()
            ))
        }
    })
}

/// Arithmetic with a datatype on either side, or `None` when neither is
/// one (so the plain number rules apply).
pub(super) fn arith(op: &str, l: &Val, r: &Val) -> Option<Result<Val, String>> {
    use Val::{Number as N, Vector2 as V2, Vector3 as V3};
    let bad = || {
        Err(format!(
            "cannot {op} {} and {}",
            l.type_name(),
            r.type_name()
        ))
    };
    let datatype = |v: &Val| matches!(v, V2(_) | V3(_) | Val::Color3(_) | Val::CFrame(_));
    if !datatype(l) && !datatype(r) {
        return None;
    }
    Some(Ok(match (op, l, r) {
        ("+", V2(a), V2(b)) => V2(*a + *b),
        ("-", V2(a), V2(b)) => V2(*a - *b),
        ("*", V2(a), V2(b)) => V2(*a * *b),
        ("/", V2(a), V2(b)) => V2(*a / *b),
        ("*", V2(a), N(k)) | ("*", N(k), V2(a)) => V2(*a * *k),
        ("/", V2(a), N(k)) => V2(*a / *k),
        ("//", V2(a), N(k)) => V2((*a / *k).floor()),
        ("+", V3(a), V3(b)) => V3(*a + *b),
        ("-", V3(a), V3(b)) => V3(*a - *b),
        ("*", V3(a), V3(b)) => V3(*a * *b),
        ("/", V3(a), V3(b)) => V3(*a / *b),
        ("*", V3(a), N(k)) | ("*", N(k), V3(a)) => V3(*a * *k),
        ("/", V3(a), N(k)) => V3(*a / *k),
        ("//", V3(a), N(k)) => V3((*a / *k).floor()),
        ("*", Val::CFrame(a), Val::CFrame(b)) => Val::CFrame(a.mul(b)),
        ("*", Val::CFrame(a), V3(p)) => V3(a.position + a.rotation * *p),
        ("+", Val::CFrame(a), V3(p)) => Val::CFrame(CFrame {
            position: a.position + *p,
            ..*a
        }),
        ("-", Val::CFrame(a), V3(p)) => Val::CFrame(CFrame {
            position: a.position - *p,
            ..*a
        }),
        _ => return Some(bad()),
    }))
}

/// Unary minus on a vector.
pub(super) fn negate(v: &Val) -> Option<Val> {
    match v {
        Val::Vector2(a) => Some(Val::Vector2(-*a)),
        Val::Vector3(a) => Some(Val::Vector3(-*a)),
        _ => None,
    }
}

/// Luau's `tostring` of a datatype, which is what one turns into when a
/// property is left holding it.
pub(super) fn text(v: &Val) -> Option<String> {
    let list = |xs: &[f64]| {
        xs.iter()
            .map(|x| format!("{x}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    Some(match v {
        Val::Vector2(a) => list(&[a.x, a.y]),
        Val::Vector3(a) => list(&[a.x, a.y, a.z]),
        Val::Color3(c) => list(c),
        Val::CFrame(c) => {
            let r = c.rotation.transpose().to_cols_array();
            let mut all = vec![c.position.x, c.position.y, c.position.z];
            all.extend(r);
            list(&all)
        }
        _ => return None,
    })
}
