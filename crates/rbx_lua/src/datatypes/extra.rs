//! `TweenInfo`, `Ray`, `Random` and `DateTime`.

mod random;
mod tween_info;

use mlua::{
    Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, UserDataRef,
};
use rbx_dom::Vector3Data;

use super::vector3::LuaVector3;

#[derive(Debug, Clone, Copy, PartialEq)]
struct LuaRay {
    origin: Vector3Data,
    direction: Vector3Data,
}

fn dot(a: Vector3Data, b: Vector3Data) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

impl LuaRay {
    fn closest_point(&self, point: Vector3Data) -> Vector3Data {
        let d = self.direction;
        let len_sq = dot(d, d);
        let t = if len_sq == 0.0 {
            0.0
        } else {
            let rel = Vector3Data {
                x: point.x - self.origin.x,
                y: point.y - self.origin.y,
                z: point.z - self.origin.z,
            };
            (dot(rel, d) / len_sq).clamp(0.0, 1.0)
        };
        Vector3Data {
            x: self.origin.x + d.x * t,
            y: self.origin.y + d.y * t,
            z: self.origin.z + d.z * t,
        }
    }
}

impl UserData for LuaRay {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Ray");
        fields.add_field_method_get("Origin", |_, this| Ok(LuaVector3(this.origin)));
        fields.add_field_method_get("Direction", |_, this| Ok(LuaVector3(this.direction)));
        fields.add_field_method_get("Unit", |_, this| {
            let d = this.direction;
            let len = dot(d, d).sqrt();
            let direction = if len == 0.0 {
                d
            } else {
                Vector3Data {
                    x: d.x / len,
                    y: d.y / len,
                    z: d.z / len,
                }
            };
            Ok(LuaRay {
                origin: this.origin,
                direction,
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("ClosestPoint", |_, this, point: LuaVector3| {
            Ok(LuaVector3(this.closest_point(point.0)))
        });
        methods.add_method("Distance", |_, this, point: LuaVector3| {
            let c = this.closest_point(point.0);
            let d = Vector3Data {
                x: point.0.x - c.x,
                y: point.0.y - c.y,
                z: point.0.z - c.z,
            };
            Ok(dot(d, d).sqrt())
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: UserDataRef<Self>| {
            Ok(*this == *other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let (o, d) = (this.origin, this.direction);
            Ok(format!(
                "{{{}, {}, {}}}, {{{}, {}, {}}}",
                o.x, o.y, o.z, d.x, d.y, d.z
            ))
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LuaDateTime {
    millis: i64,
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

impl LuaDateTime {
    fn iso(&self) -> String {
        let secs = self.millis.div_euclid(1000);
        let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
        let rem = secs.rem_euclid(86_400);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        )
    }
}

impl UserData for LuaDateTime {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "DateTime");
        fields.add_field_method_get("UnixTimestamp", |_, this| Ok(this.millis.div_euclid(1000)));
        fields.add_field_method_get("UnixTimestampMillis", |_, this| Ok(this.millis));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("ToIsoDate", |_, this, ()| Ok(this.iso()));
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: UserDataRef<Self>| {
            Ok(*this == *other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.iso()));
    }
}

fn ray(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, (origin, direction): (LuaVector3, LuaVector3)| {
            Ok(LuaRay {
                origin: origin.0,
                direction: direction.0,
            })
        })?,
    )?;
    Ok(table)
}

fn date_time(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "now",
        lua.create_function(|_, ()| {
            let millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            Ok(LuaDateTime { millis })
        })?,
    )?;
    table.set(
        "fromUnixTimestamp",
        lua.create_function(|_, seconds: f64| {
            Ok(LuaDateTime {
                millis: (seconds.floor() as i64).saturating_mul(1000),
            })
        })?,
    )?;
    table.set(
        "fromUnixTimestampMillis",
        lua.create_function(|_, millis: f64| {
            Ok(LuaDateTime {
                millis: millis.floor() as i64,
            })
        })?,
    )?;
    Ok(table)
}

pub(crate) fn register(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    globals.set("TweenInfo", tween_info::constructors(lua)?)?;
    globals.set("Random", random::constructors(lua)?)?;
    globals.set("Ray", ray(lua)?)?;
    globals.set("DateTime", date_time(lua)?)?;
    Ok(())
}
