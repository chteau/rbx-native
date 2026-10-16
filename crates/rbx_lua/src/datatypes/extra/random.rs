//! `Random` datatype: a small xorshift64* generator, seeded through splitmix64.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods};

use crate::datatypes::vector3::LuaVector3;

#[derive(Clone)]
pub(crate) struct LuaRandom {
    state: u64,
}

impl LuaRandom {
    fn seeded(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        LuaRandom {
            state: if z == 0 { 0x2545_F491_4F6C_DD1D } else { z },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl UserData for LuaRandom {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Random");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut(
            "NextNumber",
            |_, this, range: (Option<f64>, Option<f64>)| {
                let (min, max) = match range {
                    (Some(min), Some(max)) => (min, max),
                    (None, None) => (0.0, 1.0),
                    _ => {
                        return Err(mlua::Error::runtime(
                            "Random:NextNumber expects both min and max, or neither",
                        ))
                    }
                };
                Ok(min + (max - min) * this.next_f64())
            },
        );
        methods.add_method_mut("NextInteger", |_, this, (min, max): (i64, i64)| {
            if min > max {
                return Err(mlua::Error::runtime(
                    "invalid argument #2 to 'NextInteger' (interval is empty)",
                ));
            }
            let span = (max as i128 - min as i128 + 1) as u128;
            let pick = (this.next_u64() as u128) % span;
            Ok((min as i128 + pick as i128) as i64)
        });
        methods.add_method_mut("NextUnitVector", |_, this, ()| loop {
            let x = this.next_f64() * 2.0 - 1.0;
            let y = this.next_f64() * 2.0 - 1.0;
            let z = this.next_f64() * 2.0 - 1.0;
            let len = (x * x + y * y + z * z).sqrt();
            if len > 1e-6 && len <= 1.0 {
                return Ok(LuaVector3::new(
                    (x / len) as f32,
                    (y / len) as f32,
                    (z / len) as f32,
                ));
            }
        });
        methods.add_method_mut("Shuffle", |_, this, table: Table| {
            let len = table.raw_len();
            for i in (2..=len).rev() {
                let j = 1 + (this.next_u64() % i as u64) as usize;
                let a: mlua::Value = table.raw_get(i)?;
                let b: mlua::Value = table.raw_get(j)?;
                table.raw_set(i, b)?;
                table.raw_set(j, a)?;
            }
            Ok(())
        });
        methods.add_method("Clone", |_, this, ()| Ok(this.clone()));
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Random"));
    }
}

pub(super) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, seed: Option<f64>| {
            let seed = match seed {
                Some(seed) => seed.to_bits(),
                None => std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1),
            };
            Ok(LuaRandom::seeded(seed))
        })?,
    )?;
    Ok(table)
}
