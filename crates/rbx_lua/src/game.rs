//! `game`: the DOM's root level seen as a pseudo-`DataModel`.
//!
//! A place file has no serialized `DataModel` instance — its services sit at the
//! root of the DOM — so `game` is its own userdata whose children are
//! `WeakDom::root_refs`.

use mlua::{IntoLua, MetaMethod, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::{Ref, WeakDom};

use crate::ctx::Ctx;
use crate::instance::{instance_list, tree, LuaInstance};
use crate::lua_enum::LuaEnumItem;

pub(crate) struct LuaGame {
    ctx: Ctx,
}

impl LuaGame {
    pub(crate) fn new(ctx: Ctx) -> Self {
        LuaGame { ctx }
    }

    fn service(dom: &WeakDom, class: &str) -> Option<Ref> {
        dom.root_refs()
            .iter()
            .copied()
            .find(|referent| dom.get(*referent).is_some_and(|i| i.class() == class))
    }

    fn child(dom: &WeakDom, name: &str) -> Option<Ref> {
        dom.root_refs()
            .iter()
            .copied()
            .find(|referent| dom.get(*referent).is_some_and(|i| i.name() == name))
    }
}

/// Registers a `game` method that also works when a script calls it on
/// another instance (`local Children = game.GetChildren; Children(part)`):
/// the call is handed to that instance's own method of the same name.
fn either<M, A, R>(
    methods: &mut M,
    name: &'static str,
    f: impl Fn(&mlua::Lua, &LuaGame, A) -> mlua::Result<R> + 'static,
) where
    M: UserDataMethods<LuaGame>,
    A: mlua::FromLuaMulti,
    R: mlua::IntoLuaMulti,
{
    methods.add_function(
        name,
        move |lua, (this, args): (mlua::AnyUserData, mlua::MultiValue)| {
            if let Ok(game) = this.borrow::<LuaGame>() {
                let args = A::from_lua_multi(args, lua)?;
                return f(lua, &game, args)?.into_lua_multi(lua);
            }
            mlua::ObjectLike::call_method::<mlua::MultiValue>(&this, name, args)
        },
    );
}

impl UserData for LuaGame {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("GetService", |lua, this, class: String| {
            if let Some(hooks) = crate::hooks::hooks(lua) {
                if let Some(service) = hooks.service(lua, &this.ctx, &class)? {
                    return Ok(service);
                }
            }
            if this.ctx.database().class(&class).is_none() {
                return Err(mlua::Error::runtime(format!(
                    "{class} is not a valid Service name"
                )));
            }
            // Unlike Roblox, a missing service is not created on demand: services
            // carry state this crate cannot invent, and the file is the authority.
            let found = LuaGame::service(&this.ctx.dom(), &class).ok_or_else(|| {
                mlua::Error::runtime(format!("{class} is not present in this DataModel"))
            })?;
            LuaInstance::new(found, this.ctx.clone()).into_lua(lua)
        });
        either(
            methods,
            "FindFirstChild",
            |_, this, (name, recursive): (String, Option<bool>)| {
                let dom = this.ctx.dom();
                let mut found = LuaGame::child(&dom, &name);
                if found.is_none() && recursive.unwrap_or(false) {
                    found = dom
                        .root_refs()
                        .iter()
                        .find_map(|root| tree::find_child(&dom, *root, &name, true));
                }
                Ok(found.map(|referent| LuaInstance::new(referent, this.ctx.clone())))
            },
        );
        either(methods, "GetChildren", |lua, this, ()| {
            let roots = this.ctx.dom().root_refs().to_vec();
            instance_list(lua, &this.ctx, roots)
        });
        either(methods, "GetDescendants", |lua, this, ()| {
            let dom = this.ctx.dom();
            let mut found = Vec::new();
            for &root in dom.root_refs() {
                found.push(root);
                found.extend(tree::descendants(&dom, root));
            }
            drop(dom);
            instance_list(lua, &this.ctx, found)
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            match key.as_str() {
                "Name" => return "game".into_lua(lua),
                "ClassName" => return "DataModel".into_lua(lua),
                "Parent" => return Ok(Value::Nil),
                "PlaceId" | "GameId" | "CreatorId" | "PlaceVersion" => return 0.into_lua(lua),
                "JobId" => return "".into_lua(lua),
                "CreatorType" => {
                    let value = this
                        .ctx
                        .database()
                        .enum_items("CreatorType")
                        .and_then(|items| items.iter().find(|(name, _)| *name == "User"))
                        .map(|(_, value)| *value)
                        .unwrap_or(0);
                    return LuaEnumItem::new(
                        "CreatorType".to_string(),
                        "User".to_string(),
                        value,
                        this.ctx.clone(),
                    )
                    .interned(lua);
                }
                _ => {}
            }
            // A service the host provides itself (`game.Selection`) is the
            // same object `GetService` gives.
            if let Some(hooks) = crate::hooks::hooks(lua) {
                if let Some(service) = hooks.service(lua, &this.ctx, &key)? {
                    return Ok(service);
                }
            }
            let dom = this.ctx.dom();
            // `game.Workspace` works whether the service was renamed or not, so the
            // name is tried first and the class name second.
            let found = LuaGame::child(&dom, &key).or_else(|| LuaGame::service(&dom, &key));
            drop(dom);
            if found.is_none() {
                if let Some(hooks) = crate::hooks::hooks(lua) {
                    if let Some(member) = hooks.game_member(lua, &key)? {
                        return Ok(member);
                    }
                }
                if let Some(method) = crate::compat::instance_method(lua, &key)? {
                    return Ok(method);
                }
            }
            match found {
                Some(referent) => LuaInstance::new(referent, this.ctx.clone()).into_lua(lua),
                None => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of DataModel"
                ))),
            }
        });
        // The `DataModel` is left out of every path, so on its own it has only
        // its own name to give.
        either(methods, "GetFullName", |_, _, ()| Ok("game"));
        either(methods, "IsA", |_, _, class: String| {
            Ok(matches!(
                class.as_str(),
                "DataModel" | "ServiceProvider" | "Instance"
            ))
        });
        // Every service is already there, so nothing is waited for.
        either(
            methods,
            "WaitForChild",
            |_, this, (name, _timeout): (String, Option<f64>)| {
                let found = LuaGame::child(&this.ctx.dom(), &name);
                Ok(found.map(|referent| LuaInstance::new(referent, this.ctx.clone())))
            },
        );
        methods.add_method("FindService", |_, this, class: String| {
            let found = LuaGame::service(&this.ctx.dom(), &class);
            Ok(found.map(|referent| LuaInstance::new(referent, this.ctx.clone())))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("game"));
    }
}
