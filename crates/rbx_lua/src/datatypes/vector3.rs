//! `Vector3` datatype.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::Vector3Data;

use super::{from_userdata, number_arg};

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

impl UserData for LuaVector3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("X", |_, this| Ok(this.0.x));
        fields.add_field_method_get("Y", |_, this| Ok(this.0.y));
        fields.add_field_method_get("Z", |_, this| Ok(this.0.z));
        fields.add_field_method_get("Magnitude", |_, this| Ok(this.magnitude()));
        fields.add_field_method_get("Unit", |_, this| Ok(this.unit()));
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
        methods.add_meta_method(MetaMethod::Mul, |_, this, rhs: Value| {
            combine(this, &rhs, |a, b| a * b, "multiply")
        });
        methods.add_meta_method(MetaMethod::Div, |_, this, rhs: Value| {
            combine(this, &rhs, |a, b| a / b, "divide")
        });
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
    Ok(table)
}
