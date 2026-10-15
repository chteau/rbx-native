//! `InputObject`, the argument of the `InputBegan` / `InputChanged` /
//! `InputEnded` signals.

use mlua::{Lua, MetaMethod, Result, UserData, UserDataFields, Value};

use super::env::env;
use crate::ctx::Ctx;
use crate::datatypes::vector3::LuaVector3;
use crate::lua_enum::LuaEnumItem;

/// An input event as Roblox hands it to scripts: enum-valued `UserInputType`,
/// `UserInputState` and `KeyCode` (always `Unknown`: the host only forwards
/// pointer input), plus the pointer `Position` and its `Delta`.
pub(crate) struct LuaInputObject {
    kind: String,
    state: String,
    position: [f32; 3],
    ctx: Ctx,
}

/// `Enum.<enum_name>.<item>`, or an error naming the missing item.
pub(crate) fn enum_item(ctx: &Ctx, enum_name: &str, item: &str) -> Result<LuaEnumItem> {
    let value = ctx
        .database()
        .enum_items(enum_name)
        .and_then(|items| items.iter().find(|(name, _)| *name == item))
        .map(|(_, value)| *value)
        .ok_or_else(|| {
            mlua::Error::runtime(format!("{item} is not a valid member of Enum.{enum_name}"))
        })?;
    Ok(LuaEnumItem::new(
        enum_name.to_string(),
        item.to_string(),
        value,
        ctx.clone(),
    ))
}

impl UserData for LuaInputObject {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "InputObject");
        fields.add_field_method_get("UserInputType", |lua, this| {
            enum_item(&this.ctx, "UserInputType", &this.kind)?.interned(lua)
        });
        fields.add_field_method_get("UserInputState", |lua, this| {
            enum_item(&this.ctx, "UserInputState", &this.state)?.interned(lua)
        });
        fields.add_field_method_get("KeyCode", |lua, this| {
            enum_item(&this.ctx, "KeyCode", "Unknown")?.interned(lua)
        });
        fields.add_field_method_get("Position", |_, this| {
            let [x, y, z] = this.position;
            Ok(LuaVector3::new(x, y, z))
        });
        fields.add_field_method_get("Delta", |_, _| Ok(LuaVector3::new(0.0, 0.0, 0.0)));
    }
}

/// An `InputObject` for `Enum.UserInputType.<kind>` in `Enum.UserInputState.
/// <state>` at pointer position `(x, y)`.
pub(crate) fn input_object(lua: &Lua, kind: &str, state: &str, x: f32, y: f32) -> Result<Value> {
    input_object_at(lua, kind, state, [x, y, 0.0])
}

/// As [`input_object`] with an explicit `Position.Z` (the wheel direction).
pub(crate) fn input_object_at(
    lua: &Lua,
    kind: &str,
    state: &str,
    position: [f32; 3],
) -> Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaInputObject {
        kind: kind.to_string(),
        state: state.to_string(),
        position,
        ctx: env(lua).ctx.clone(),
    })?))
}

#[cfg(test)]
mod tests {
    use mlua::MultiValue;
    use rbx_dom::WeakDom;

    use crate::instance::LuaInstance;
    use crate::plugin::api::{GuiEvent, GuiEventKind};
    use crate::plugin::events::fire_gui;
    use crate::plugin::scheduler;
    use crate::plugin::testing::test_vm;

    #[test]
    fn input_began_reaches_frame_and_service() {
        let mut dom = WeakDom::new();
        let frame = dom.new_instance("Frame", "F", None);
        let (lua, env, _) = test_vm(dom, true);
        lua.globals()
            .set("f", LuaInstance::new(frame, env.ctx.clone()))
            .unwrap();
        let uis = crate::plugin::services::service(&lua, "UserInputService")
            .unwrap()
            .unwrap();
        lua.globals().set("UIS", uis).unwrap();
        let f = lua
            .load(
                "f.InputBegan:Connect(function(i) frame = i.UserInputType.Name state = i.UserInputState.Name end)
                 UIS.InputBegan:Connect(function(i, gp) svc = i.UserInputType.Name processed = gp end)",
            )
            .into_function()
            .unwrap();
        scheduler::spawn(&lua, f, MultiValue::new()).unwrap();
        fire_gui(
            &lua,
            &GuiEvent {
                referent: frame,
                kind: GuiEventKind::Down {
                    button: 2,
                    x: 3.0,
                    y: 4.0,
                },
            },
        )
        .unwrap();
        let g = lua.globals();
        assert_eq!(g.get::<String>("frame").unwrap(), "MouseButton2");
        assert_eq!(g.get::<String>("state").unwrap(), "Begin");
        assert_eq!(g.get::<String>("svc").unwrap(), "MouseButton2");
        assert!(!g.get::<bool>("processed").unwrap());
    }
}
