//! `Vector3` datatype.

use mlua::{
    Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic,
};
use rbx_dom::Vector3Data;

use super::{from_userdata, number_arg};
use crate::lua_enum::LuaEnumItem;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaVector3(pub(crate) Vector3Data);

impl LuaVector3 {
    pub(crate) fn new(x: f32, y: f32, z: f32) -> Self {
        LuaVector3(Vector3Data { x, y, z })
    }

    fn magnitude(&self) -> f32 {
        (self.0.x * self.0.x + self.0.y * self.0.y + self.0.z * self.0.z).sqrt()
    }

    // Roblox yields a zero vector rather than NaNs for the unit of a zero vector.
    fn unit(&self) -> LuaVector3 {
        let magnitude = self.magnitude();
        if magnitude == 0.0 {
            return LuaVector3::new(0.0, 0.0, 0.0);
        }
        LuaVector3::new(
            self.0.x / magnitude,
            self.0.y / magnitude,
            self.0.z / magnitude,
        )
    }
}

// Componentwise against another Vector3, uniform against a scalar, which is how
// Luau's own Vector3 behaves.
fn combine(
    lhs: &LuaVector3,
    rhs: &Value,
    op: fn(f32, f32) -> f32,
    what: &str,
) -> Result<LuaVector3> {
    if let Value::UserData(data) = rhs {
        if let Ok(other) = data.borrow::<LuaVector3>() {
            return Ok(LuaVector3::new(
                op(lhs.0.x, other.0.x),
                op(lhs.0.y, other.0.y),
                op(lhs.0.z, other.0.z),
            ));
        }
    }
    let scalar = number_arg(rhs).ok_or_else(|| {
        mlua::Error::runtime(format!("Vector3 expected a Vector3 or a number to {what}"))
    })?;
    Ok(LuaVector3::new(
        op(lhs.0.x, scalar),
        op(lhs.0.y, scalar),
        op(lhs.0.z, scalar),
    ))
}

type Operand = std::result::Result<LuaVector3, f32>;

fn operand(value: &Value) -> Option<Operand> {
    if let Value::UserData(data) = value {
        if let Ok(v) = data.borrow::<LuaVector3>() {
            return Some(Ok(*v));
        }
    }
    number_arg(value).map(Err)
}

// Either side may be the scalar, so `2 * v` and `2 / v` work.
fn binary(a: &Value, b: &Value, op: fn(f32, f32) -> f32, what: &str) -> Result<LuaVector3> {
    let bad = || mlua::Error::runtime(format!("Vector3 expected a Vector3 or a number to {what}"));
    let (a, b) = (operand(a).ok_or_else(bad)?, operand(b).ok_or_else(bad)?);
    let parts = |v: &Operand| match v {
        Ok(v) => [v.0.x, v.0.y, v.0.z],
        Err(s) => [*s; 3],
    };
    if let (Err(_), Err(_)) = (&a, &b) {
        return Err(bad());
    }
    let (l, r) = (parts(&a), parts(&b));
    Ok(LuaVector3::new(
        op(l[0], r[0]),
        op(l[1], r[1]),
        op(l[2], r[2]),
    ))
}

impl LuaVector3 {
    fn map(&self, f: fn(f32) -> f32) -> LuaVector3 {
        LuaVector3::new(f(self.0.x), f(self.0.y), f(self.0.z))
    }
}

impl UserData for LuaVector3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        // Roblox still answers to the deprecated lowercase spellings.
        for (upper, lower) in [("X", "x"), ("Y", "y"), ("Z", "z")] {
            let axis = |v: &LuaVector3, name: &str| match name {
                "X" => v.0.x,
                "Y" => v.0.y,
                _ => v.0.z,
            };
            fields.add_field_method_get(upper, move |_, this| Ok(axis(this, upper)));
            fields.add_field_method_get(lower, move |_, this| Ok(axis(this, upper)));
        }
        for name in ["Magnitude", "magnitude"] {
            fields.add_field_method_get(name, |_, this| Ok(this.magnitude()));
        }
        for name in ["Unit", "unit"] {
            fields.add_field_method_get(name, |_, this| Ok(this.unit()));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Dot", |_, this, other: LuaVector3| {
            Ok(this.0.x * other.0.x + this.0.y * other.0.y + this.0.z * other.0.z)
        });
        methods.add_method("Cross", |_, this, other: LuaVector3| {
            Ok(LuaVector3::new(
                this.0.y * other.0.z - this.0.z * other.0.y,
                this.0.z * other.0.x - this.0.x * other.0.z,
                this.0.x * other.0.y - this.0.y * other.0.x,
            ))
        });
        methods.add_meta_method(MetaMethod::Add, |_, this, rhs: Value| {
            combine(this, &rhs, |a, b| a + b, "add")
        });
        methods.add_meta_method(MetaMethod::Sub, |_, this, rhs: Value| {
            combine(this, &rhs, |a, b| a - b, "subtract")
        });
        methods.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| {
            binary(&a, &b, |x, y| x * y, "multiply")
        });
        methods.add_meta_function(MetaMethod::Div, |_, (a, b): (Value, Value)| {
            binary(&a, &b, |x, y| x / y, "divide")
        });
        methods.add_meta_function(MetaMethod::IDiv, |_, (a, b): (Value, Value)| {
            binary(&a, &b, |x, y| (x / y).floor(), "divide")
        });
        methods.add_method("Lerp", |_, this, (goal, alpha): (LuaVector3, f32)| {
            Ok(LuaVector3::new(
                this.0.x + (goal.0.x - this.0.x) * alpha,
                this.0.y + (goal.0.y - this.0.y) * alpha,
                this.0.z + (goal.0.z - this.0.z) * alpha,
            ))
        });
        methods.add_method("Abs", |_, this, ()| Ok(this.map(f32::abs)));
        methods.add_method("Floor", |_, this, ()| Ok(this.map(f32::floor)));
        methods.add_method("Ceil", |_, this, ()| Ok(this.map(f32::ceil)));
        methods.add_method("Sign", |_, this, ()| {
            Ok(this.map(|v| {
                if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            }))
        });
        methods.add_method("Min", |_, this, others: Variadic<LuaVector3>| {
            Ok(others.iter().fold(*this, |acc, o| {
                LuaVector3::new(acc.0.x.min(o.0.x), acc.0.y.min(o.0.y), acc.0.z.min(o.0.z))
            }))
        });
        methods.add_method("Max", |_, this, others: Variadic<LuaVector3>| {
            Ok(others.iter().fold(*this, |acc, o| {
                LuaVector3::new(acc.0.x.max(o.0.x), acc.0.y.max(o.0.y), acc.0.z.max(o.0.z))
            }))
        });
        methods.add_method(
            "FuzzyEq",
            |_, this, (other, eps): (LuaVector3, Option<f32>)| {
                let eps = eps.unwrap_or(1e-5);
                let close = |a: f32, b: f32| (a - b).abs() <= eps * (1.0 + a.abs().max(b.abs()));
                Ok(close(this.0.x, other.0.x)
                    && close(this.0.y, other.0.y)
                    && close(this.0.z, other.0.z))
            },
        );
        methods.add_method(
            "Angle",
            |_, this, (other, axis): (LuaVector3, Option<LuaVector3>)| {
                let cross = LuaVector3::new(
                    this.0.y * other.0.z - this.0.z * other.0.y,
                    this.0.z * other.0.x - this.0.x * other.0.z,
                    this.0.x * other.0.y - this.0.y * other.0.x,
                );
                let dot = this.0.x * other.0.x + this.0.y * other.0.y + this.0.z * other.0.z;
                let angle = cross.magnitude().atan2(dot);
                match axis {
                    Some(axis)
                        if cross.0.x * axis.0.x + cross.0.y * axis.0.y + cross.0.z * axis.0.z
                            < 0.0 =>
                    {
                        Ok(-angle)
                    }
                    _ => Ok(angle),
                }
            },
        );
        methods.add_meta_method(MetaMethod::Unm, |_, this, ()| {
            Ok(LuaVector3::new(-this.0.x, -this.0.y, -this.0.z))
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaVector3| {
            Ok(*this == other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}, {}", this.0.x, this.0.y, this.0.z))
        });
    }
}

impl mlua::FromLua for LuaVector3 {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "Vector3")
    }
}

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, (x, y, z): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaVector3::new(
                x.unwrap_or_default(),
                y.unwrap_or_default(),
                z.unwrap_or_default(),
            ))
        })?,
    )?;
    table.set("zero", LuaVector3::new(0.0, 0.0, 0.0))?;
    table.set("one", LuaVector3::new(1.0, 1.0, 1.0))?;
    table.set("xAxis", LuaVector3::new(1.0, 0.0, 0.0))?;
    table.set("yAxis", LuaVector3::new(0.0, 1.0, 0.0))?;
    table.set("zAxis", LuaVector3::new(0.0, 0.0, 1.0))?;
    table.set(
        "FromNormalId",
        lua.create_function(|_, item: Value| {
            // Right, Top, Back, Left, Bottom, Front — `Enum.NormalId`'s order.
            let [x, y, z] = match enum_value(&item, "NormalId")? {
                0 => [1.0, 0.0, 0.0],
                1 => [0.0, 1.0, 0.0],
                2 => [0.0, 0.0, 1.0],
                3 => [-1.0, 0.0, 0.0],
                4 => [0.0, -1.0, 0.0],
                _ => [0.0, 0.0, -1.0],
            };
            Ok(LuaVector3::new(x, y, z))
        })?,
    )?;
    table.set(
        "FromAxis",
        lua.create_function(|_, item: Value| {
            let [x, y, z] = match enum_value(&item, "Axis")? {
                0 => [1.0, 0.0, 0.0],
                1 => [0.0, 1.0, 0.0],
                _ => [0.0, 0.0, 1.0],
            };
            Ok(LuaVector3::new(x, y, z))
        })?,
    )?;
    Ok(table)
}

/// The value of an `Enum.<expected>` item, or the error Roblox gives for
/// anything else.
fn enum_value(item: &Value, expected: &str) -> Result<u32> {
    if let Value::UserData(data) = item {
        if let Ok(item) = data.borrow::<LuaEnumItem>() {
            if item.enum_name() == expected {
                return Ok(item.value());
            }
        }
    }
    Err(mlua::Error::runtime(format!(
        "invalid argument #1 (Enum.{expected} expected, got {})",
        item.type_name()
    )))
}
