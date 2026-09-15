//! `Instance` userdata: one DOM node seen from Luau.

pub(crate) mod tree;

use mlua::{IntoLua, Lua, MetaMethod, Result, Table, UserData, UserDataMethods, Value};
use rbx_dom::Ref;

use crate::ctx::Ctx;
use crate::defaults;
use crate::game::LuaGame;
use crate::not_creatable;
use crate::property;

#[derive(Clone)]
pub(crate) struct LuaInstance {
    referent: Ref,
    ctx: Ctx,
}

impl LuaInstance {
    pub(crate) fn new(referent: Ref, ctx: Ctx) -> Self {
        LuaInstance { referent, ctx }
    }

    pub(crate) fn referent(&self) -> Ref {
        self.referent
    }

    fn class(&self) -> Result<String> {
        self.ctx
            .dom()
            .get(self.referent)
            .map(|instance| instance.class().to_string())
            .ok_or_else(property::missing_instance)
    }

    fn instance_name(&self) -> Result<String> {
        self.ctx
            .dom()
            .get(self.referent)
            .map(|instance| instance.name().to_string())
            .ok_or_else(property::missing_instance)
    }
}

/// Resolves the right-hand side of a `Parent` assignment.
///
/// `nil` and `game` both land on the root level: the DOM has no storage for an
/// instance that belongs to no tree, so a cleared parent is a root instance.
fn parent_target(value: &Value) -> Result<Option<Ref>> {
    match value {
        Value::Nil => Ok(None),
        Value::UserData(data) => {
            if let Ok(instance) = data.borrow::<LuaInstance>() {
                return Ok(Some(instance.referent));
            }
            if data.borrow::<LuaGame>().is_ok() {
                return Ok(None);
            }
            Err(mlua::Error::runtime(
                "Unable to assign property Parent. Instance expected",
            ))
        }
        other => Err(mlua::Error::runtime(format!(
            "Unable to assign property Parent. Instance expected, got {}",
            other.type_name()
        ))),
    }
}

pub(crate) fn instance_list(lua: &Lua, ctx: &Ctx, refs: Vec<Ref>) -> Result<Table> {
    let table = lua.create_table()?;
    for (index, referent) in refs.into_iter().enumerate() {
        table.set(index + 1, LuaInstance::new(referent, ctx.clone()))?;
    }
    Ok(table)
}

impl UserData for LuaInstance {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("IsA", |_, this, class: String| {
            Ok(this.ctx.database().is_subclass_of(&this.class()?, &class))
        });
        methods.add_method(
            "FindFirstChild",
            |_, this, (name, recursive): (String, Option<bool>)| {
                let found = tree::find_child(
                    &this.ctx.dom(),
                    this.referent,
                    &name,
                    recursive.unwrap_or(false),
                );
                Ok(found.map(|referent| LuaInstance::new(referent, this.ctx.clone())))
            },
        );
        methods.add_method("GetChildren", |lua, this, ()| {
            let children = this
                .ctx
                .dom()
                .get(this.referent)
                .map(|instance| instance.children().to_vec())
                .ok_or_else(property::missing_instance)?;
            instance_list(lua, &this.ctx, children)
        });
        methods.add_method("GetDescendants", |lua, this, ()| {
            let found = tree::descendants(&this.ctx.dom(), this.referent);
            instance_list(lua, &this.ctx, found)
        });
        methods.add_method("Destroy", |_, this, ()| {
            this.ctx.dom_mut().remove(this.referent);
            Ok(())
        });
        methods.add_method("Clone", |_, this, ()| {
            let copy = tree::deep_clone(&mut this.ctx.dom_mut(), this.referent, None)
                .ok_or_else(property::missing_instance)?;
            Ok(LuaInstance::new(copy, this.ctx.clone()))
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            match key.as_str() {
                "Name" => return this.instance_name()?.into_lua(lua),
                "ClassName" => return this.class()?.into_lua(lua),
                "Parent" => {
                    // Reject a destroyed instance before reporting "no parent".
                    this.class()?;
                    let parent = tree::parent_of(&this.ctx.dom(), this.referent);
                    return match parent {
                        Some(referent) => {
                            LuaInstance::new(referent, this.ctx.clone()).into_lua(lua)
                        }
                        None => LuaGame::new(this.ctx.clone()).into_lua(lua),
                    };
                }
                _ => {}
            }

            if let Some(value) = property::get(lua, &this.ctx, this.referent, &key)? {
                return Ok(value);
            }
            let child = tree::find_child(&this.ctx.dom(), this.referent, &key, false);
            match child {
                Some(referent) => LuaInstance::new(referent, this.ctx.clone()).into_lua(lua),
                None => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of {}",
                    this.class()?
                ))),
            }
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |_, this, (key, value): (String, Value)| {
                match key.as_str() {
                    "Name" => {
                        let Value::String(text) = &value else {
                            return Err(mlua::Error::runtime(format!(
                                "Unable to assign property Name. string expected, got {}",
                                value.type_name()
                            )));
                        };
                        let text = text.to_str()?.to_string();
                        this.ctx
                            .dom_mut()
                            .set_name(this.referent, &text)
                            .map_err(|error| mlua::Error::runtime(error.to_string()))?;
                        return Ok(());
                    }
                    "ClassName" => {
                        return Err(mlua::Error::runtime(
                            "Unable to assign property ClassName. Property is read only",
                        ))
                    }
                    "Parent" => {
                        let target = parent_target(&value)?;
                        if let Some(parent) = target {
                            let dom = this.ctx.dom();
                            if dom.get(parent).is_none() {
                                return Err(property::missing_instance());
                            }
                            if tree::is_ancestor_of(&dom, this.referent, parent) {
                                return Err(mlua::Error::runtime(
                                    "Attempt to set parent would result in circular reference",
                                ));
                            }
                        }
                        this.ctx.dom_mut().set_parent(this.referent, target);
                        return Ok(());
                    }
                    _ => {}
                }

                if property::set(&this.ctx, this.referent, &key, &value)? {
                    return Ok(());
                }
                Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of {}",
                    this.class()?
                )))
            },
        );
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| this.instance_name());
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: mlua::UserDataRef<Self>| {
            Ok(this.referent == other.referent)
        });
    }
}

pub(crate) fn constructors(lua: &Lua, ctx: Ctx) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            move |_, (class, parent): (String, Option<Value>)| -> Result<LuaInstance> {
                if ctx.database().class(&class).is_none() {
                    return Err(mlua::Error::runtime(format!(
                        "Unable to create an Instance of type {class}"
                    )));
                }
                if not_creatable::is_blocked(&class) {
                    return Err(mlua::Error::runtime(format!(
                        "Unable to create an Instance of type {class}: it is not creatable"
                    )));
                }
                let target = match &parent {
                    Some(value) => parent_target(value)?,
                    None => None,
                };
                let referent = ctx.dom_mut().new_instance(&class, &class, target);
                defaults::apply(&ctx, &class, referent);
                Ok(LuaInstance::new(referent, ctx.clone()))
            },
        )?,
    )?;
    Ok(table)
}
