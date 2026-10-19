//! `TweenService`: tweens that jump straight to their goals.
//!
//! There is no frame loop to interpolate on, so `Play` applies the goal
//! values at once and completes.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use mlua::{
    AnyUserData, Lua, MetaMethod, MultiValue, ObjectLike, Result, Table, UserData, UserDataFields,
    UserDataMethods, Value,
};
use rbx_dom::Ref;

use crate::engine::signal::{fire_values, LuaSignal, Owner, SignalKey};
use crate::plugin::env::env;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub(super) struct TweenService;

fn playback_state(lua: &Lua, item: &str) -> Result<AnyUserData> {
    let enums: AnyUserData = lua.globals().get("Enum")?;
    let kind: AnyUserData = enums.get("PlaybackState")?;
    kind.get(item)
}

struct Tween {
    id: u32,
    instance: Ref,
    goals: Vec<(String, Value)>,
    state: RefCell<&'static str>,
}

impl Tween {
    fn key(&self) -> SignalKey {
        SignalKey::new(Owner::Service("Tween"), format!("Completed:{}", self.id))
    }
}

impl UserData for Tween {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
        fields.add_field_method_get("PlaybackState", |lua, this| {
            playback_state(lua, *this.state.borrow())
        });
        fields.add_field_method_get("Completed", |_, this| Ok(LuaSignal(this.key())));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Play", |lua, this, ()| {
            let ctx = env(lua).ctx.clone();
            for (name, value) in &this.goals {
                if !crate::property::set(&ctx, this.instance, name, value)? {
                    return Err(mlua::Error::runtime(format!(
                        "{name} is not a valid member of the tweened instance"
                    )));
                }
            }
            *this.state.borrow_mut() = "Completed";
            let item = playback_state(lua, "Completed")?;
            fire_values(
                lua,
                &this.key(),
                MultiValue::from_iter([Value::UserData(item)]),
            )
        });
        methods.add_method("Cancel", |_, this, ()| {
            *this.state.borrow_mut() = "Cancelled";
            Ok(())
        });
        methods.add_method("Pause", |_, this, ()| {
            *this.state.borrow_mut() = "Paused";
            Ok(())
        });
        methods.add_method("Destroy", |_, _, ()| Ok(()));
    }
}

impl UserData for TweenService {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "Create",
            |_, _, (instance, _info, goals): (AnyUserData, Value, Table)| {
                let target = instance
                    .borrow::<crate::instance::LuaInstance>()
                    .map_err(|_| mlua::Error::runtime("TweenService:Create expects an Instance"))?
                    .referent();
                let mut list = Vec::new();
                for pair in goals.pairs::<String, Value>() {
                    list.push(pair?);
                }
                Ok(Tween {
                    id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                    instance: target,
                    goals: list,
                    state: RefCell::new("Begin"),
                })
            },
        );
        // Easing is not applied, so every alpha maps to itself.
        methods.add_method(
            "GetValue",
            |_, _, (alpha, _style, _direction): (f64, Value, Value)| Ok(alpha),
        );
    }
}
