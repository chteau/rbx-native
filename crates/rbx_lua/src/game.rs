//! `game`: the DOM's root level seen as a pseudo-`DataModel`.
//!
//! A place file has no serialized `DataModel` instance — its services sit at the
//! root of the DOM — so `game` is its own userdata whose children are
//! `WeakDom::root_refs`.

use mlua::{IntoLua, MetaMethod, UserData, UserDataMethods, Value};
use rbx_dom::{Ref, WeakDom};

use crate::ctx::Ctx;
use crate::instance::{instance_list, tree, LuaInstance};

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

impl UserData for LuaGame {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("GetService", |_, this, class: String| {
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
            Ok(LuaInstance::new(found, this.ctx.clone()))
        });
        methods.add_method(
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
        methods.add_method("GetChildren", |lua, this, ()| {
            let roots = this.ctx.dom().root_refs().to_vec();
            instance_list(lua, &this.ctx, roots)
        });
        methods.add_method("GetDescendants", |lua, this, ()| {
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
                _ => {}
            }
            let dom = this.ctx.dom();
            // `game.Workspace` works whether the service was renamed or not, so the
            // name is tried first and the class name second.
            let found = LuaGame::child(&dom, &key).or_else(|| LuaGame::service(&dom, &key));
            drop(dom);
            match found {
                Some(referent) => LuaInstance::new(referent, this.ctx.clone()).into_lua(lua),
                None => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of DataModel"
                ))),
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("game"));
    }
}
