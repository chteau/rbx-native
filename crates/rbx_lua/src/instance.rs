//! `Instance` userdata: one DOM node seen from Luau.

mod bounds;
pub(crate) mod tags;
pub(crate) mod tree;

use mlua::{
    FromLuaMulti, IntoLua, IntoLuaMulti, Lua, MetaMethod, MultiValue, Result, Table, UserData,
    UserDataFields, UserDataMethods, Value,
};
use rbx_dom::Ref;

use crate::ctx::Ctx;
use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::from_userdata;
use crate::defaults;
use crate::game::LuaGame;
use crate::not_creatable;
use crate::property;

#[derive(Clone)]
pub(crate) struct LuaInstance {
    referent: Ref,
    ctx: Ctx,
}

/// What [`LuaInstance::new`] hands back. Converting it into Lua yields
/// the one userdata this VM holds for that instance, Roblox instances are
/// unique objects, so scripts key tables by them and `table.find` /
/// `rawequal` must see the same value however the instance was reached. The
/// cache is weak-valued: an instance no script holds is let go.
pub(crate) struct InstanceRef(LuaInstance);

const INSTANCES: &str = "rbxn.instances";

impl IntoLua for InstanceRef {
    fn into_lua(self, lua: &Lua) -> Result<Value> {
        let cache = match lua.named_registry_value::<Option<Table>>(INSTANCES)? {
            Some(cache) => cache,
            None => {
                let cache = lua.create_table()?;
                let weak = lua.create_table()?;
                weak.set("__mode", "v")?;
                cache.set_metatable(Some(weak))?;
                lua.set_named_registry_value(INSTANCES, cache.clone())?;
                cache
            }
        };
        let key = format!("{:?}", self.0.referent);
        let found: Value = cache.get(key.as_str())?;
        if let Value::UserData(_) = found {
            return Ok(found);
        }
        let value = Value::UserData(lua.create_userdata(self.0)?);
        cache.set(key, value.clone())?;
        Ok(value)
    }
}

impl LuaInstance {
    // Hands back the interning handle, not the userdata itself: every
    // route into Lua has to go through the one cache (see `InstanceRef`).
    #[allow(clippy::new_ret_no_self)]
    pub(crate) fn new(referent: Ref, ctx: Ctx) -> InstanceRef {
        InstanceRef(LuaInstance { referent, ctx })
    }

    fn find_child(
        &self,
        recursive: bool,
        test: impl Fn(&rbx_dom::Instance) -> bool,
    ) -> Option<InstanceRef> {
        tree::find_child_where(&self.ctx.dom(), self.referent, recursive, &test)
            .map(|referent| LuaInstance::new(referent, self.ctx.clone()))
    }

    fn find_ancestor(&self, test: impl Fn(&rbx_dom::Instance) -> bool) -> Option<InstanceRef> {
        tree::find_ancestor_where(&self.ctx.dom(), self.referent, &test)
            .map(|referent| LuaInstance::new(referent, self.ctx.clone()))
    }

    pub(crate) fn referent(&self) -> Ref {
        self.referent
    }

    fn class(&self) -> Result<String> {
        let found = self
            .ctx
            .dom()
            .get(self.referent)
            .map(|i| i.class().to_string());
        found
            .or_else(|| self.ctx.tombstone(self.referent).map(|(class, _)| class))
            .ok_or_else(property::missing_instance)
    }

    fn model_bounds(&self, member: &str) -> Result<(rbx_dom::CFrameData, rbx_dom::Vector3Data)> {
        let class = self.class()?;
        let found = if self.ctx.database().is_subclass_of(&class, "Model") {
            bounds::bounding_box(&self.ctx.dom(), self.ctx.database(), self.referent)
        } else {
            None
        };
        found.ok_or_else(|| self.not_a_member(member))
    }

    fn require_part(&self, member: &str) -> Result<()> {
        let class = self.class()?;
        if self.ctx.database().is_subclass_of(&class, "BasePart") {
            Ok(())
        } else {
            Err(self.not_a_member(member))
        }
    }

    /// Roblox's error for a member this instance lacks, naming it by its
    /// full name: `Nope is not a valid member of Part "Workspace.Part"`.
    fn not_a_member(&self, member: &str) -> mlua::Error {
        let class = match self.class() {
            Ok(class) => class,
            Err(error) => return error,
        };
        let full = tree::full_name(&self.ctx.dom(), self.referent)
            .or_else(|| self.ctx.tombstone(self.referent).map(|(_, name)| name))
            .unwrap_or_default();
        mlua::Error::runtime(format!(
            "{member} is not a valid member of {class} \"{full}\""
        ))
    }

    fn instance_name(&self) -> Result<String> {
        let found = self
            .ctx
            .dom()
            .get(self.referent)
            .map(|i| i.name().to_string());
        found
            .or_else(|| self.ctx.tombstone(self.referent).map(|(_, name)| name))
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
            // A plugin's own toolbar, button or `plugin` object: instances
            // in Roblox, handles here. What is parented to one stays the
            // plugin's own, out of the place.
            let handle = data
                .metatable()
                .ok()
                .and_then(|meta| meta.get::<Option<String>>("__type").ok().flatten());
            if handle.as_deref() == Some("Instance") {
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

/// An `Instance` argument as a referent; `nil` and non-instances give `None`.
fn instance_arg(value: &Value) -> Result<Option<Ref>> {
    match value {
        Value::UserData(data) if data.is::<LuaInstance>() => {
            Ok(Some(data.borrow::<LuaInstance>()?.referent))
        }
        _ => Ok(None),
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
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

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
        // The class- and ancestor-flavoured finds every plugin leans on.
        // `OfClass` matches the exact class, `WhichIsA` any subclass.
        methods.add_method("FindFirstChildOfClass", |_, this, class: String| {
            Ok(this.find_child(false, |i| i.class() == class))
        });
        methods.add_method(
            "FindFirstChildWhichIsA",
            |_, this, (class, recursive): (String, Option<bool>)| {
                let db = this.ctx.database();
                Ok(this.find_child(recursive.unwrap_or(false), |i| {
                    db.is_subclass_of(i.class(), &class)
                }))
            },
        );
        methods.add_method("FindFirstAncestor", |_, this, name: String| {
            Ok(this.find_ancestor(|i| i.name() == name))
        });
        methods.add_method("FindFirstAncestorOfClass", |_, this, class: String| {
            Ok(this.find_ancestor(|i| i.class() == class))
        });
        methods.add_method("FindFirstAncestorWhichIsA", |_, this, class: String| {
            let db = this.ctx.database();
            Ok(this.find_ancestor(|i| db.is_subclass_of(i.class(), &class)))
        });
        methods.add_method("IsDescendantOf", |_, this, other: Value| {
            let Some(ancestor) = instance_arg(&other)? else {
                // `game` is not a DOM node, yet everything in the place is
                // under it.
                let is_game = matches!(&other, Value::UserData(u) if u.is::<LuaGame>());
                return Ok(is_game && tree::parent_of(&this.ctx.dom(), this.referent).is_some());
            };
            Ok(ancestor != this.referent
                && tree::is_ancestor_of(&this.ctx.dom(), ancestor, this.referent))
        });
        methods.add_method("IsAncestorOf", |_, this, other: Value| {
            let Some(target) = instance_arg(&other)? else {
                return Ok(false);
            };
            Ok(target != this.referent
                && tree::is_ancestor_of(&this.ctx.dom(), this.referent, target))
        });
        methods.add_method("GetDebugId", |_, this, _scope: Option<Value>| {
            Ok(format!("{:?}", this.referent))
        });
        methods.add_method("GetBoundingBox", |_, this, ()| {
            let (frame, size) = this.model_bounds("GetBoundingBox")?;
            Ok((
                LuaCFrame(frame),
                crate::datatypes::vector3::LuaVector3::new(size.x, size.y, size.z),
            ))
        });
        methods.add_method("GetExtentsSize", |_, this, ()| {
            let (_, size) = this.model_bounds("GetExtentsSize")?;
            Ok(crate::datatypes::vector3::LuaVector3::new(
                size.x, size.y, size.z,
            ))
        });
        methods.add_method("GetTouchingParts", |lua, this, ()| {
            this.require_part("GetTouchingParts")?;
            lua.create_table()
        });
        methods.add_method(
            "GetConnectedParts",
            |lua, this, _recursive: Option<bool>| {
                this.require_part("GetConnectedParts")?;
                lua.create_table()
            },
        );
        methods.add_method("ClearAllChildren", |_, this, ()| {
            this.ctx.check_write(this.referent)?;
            let children = this
                .ctx
                .dom()
                .get(this.referent)
                .map(|instance| instance.children().to_vec())
                .ok_or_else(property::missing_instance)?;
            let mut dom = this.ctx.dom_mut();
            for child in children {
                this.ctx.bury(&dom, child);
                dom.remove(child);
            }
            Ok(())
        });
        methods.add_method("GetChildren", |lua, this, ()| {
            let children = this
                .ctx
                .dom()
                .get(this.referent)
                .map(|instance| instance.children().to_vec())
                .ok_or_else(property::missing_instance)?;
            instance_list(lua, &this.ctx, children)
        });
        methods.add_method("GetFullName", |_, this, ()| {
            tree::full_name(&this.ctx.dom(), this.referent).ok_or_else(property::missing_instance)
        });
        methods.add_method("GetDescendants", |lua, this, ()| {
            let found = tree::descendants(&this.ctx.dom(), this.referent);
            instance_list(lua, &this.ctx, found)
        });
        // `PVInstance:GetPivot`/`PivotTo`, through the same pivot the
        // editor's `Origin` row reads (see `crate::pivot`). Anything that is
        // not a part or model has no pivot, and Roblox has no such member on
        // it: the same error the index metamethod gives for one.
        methods.add_method("GetPivot", |_, this, ()| {
            let found = crate::pivot::pivot(&this.ctx.dom(), this.ctx.database(), this.referent);
            match found {
                Some(frame) => Ok(LuaCFrame(frame)),
                None => Err(this.not_a_member("GetPivot")),
            }
        });
        methods.add_method("PivotTo", |_, this, target: Value| {
            let target: LuaCFrame = from_userdata(&target, "CFrame")?;
            this.ctx.check_write(this.referent)?;
            let moved = crate::pivot::pivot_to(
                &mut this.ctx.dom_mut(),
                this.ctx.database(),
                this.referent,
                &target.0,
            );
            match moved {
                Some(result) => result.map_err(mlua::Error::runtime),
                None => Err(this.not_a_member("PivotTo")),
            }
        });
        methods.add_method("Destroy", |_, this, ()| {
            this.ctx.check_write(this.referent)?;
            let mut dom = this.ctx.dom_mut();
            // A model losing its `PrimaryPart` keeps the pivot it had.
            crate::pivot::keep_pivots(&mut dom, this.ctx.database(), &[this.referent])
                .map_err(mlua::Error::runtime)?;
            this.ctx.bury(&dom, this.referent);
            dom.remove(this.referent);
            Ok(())
        });
        methods.add_method("AddTag", |lua, this, args: MultiValue| {
            if let Some(answer) =
                tags::service_call(lua, &this.ctx, this.referent, "AddTag", &args)?
            {
                return Ok(answer);
            }
            let tag = String::from_lua_multi(args, lua)?;
            tags::set(&this.ctx, this.referent, &tag, true).map(|_| MultiValue::new())
        });
        methods.add_method("RemoveTag", |lua, this, args: MultiValue| {
            if let Some(answer) =
                tags::service_call(lua, &this.ctx, this.referent, "RemoveTag", &args)?
            {
                return Ok(answer);
            }
            let tag = String::from_lua_multi(args, lua)?;
            tags::set(&this.ctx, this.referent, &tag, false).map(|_| MultiValue::new())
        });
        methods.add_method("HasTag", |lua, this, args: MultiValue| {
            if let Some(answer) =
                tags::service_call(lua, &this.ctx, this.referent, "HasTag", &args)?
            {
                return Ok(answer);
            }
            let tag = String::from_lua_multi(args, lua)?;
            tags::get(&this.ctx, this.referent)
                .contains(&tag)
                .into_lua_multi(lua)
        });
        methods.add_method("GetTags", |lua, this, args: MultiValue| {
            if let Some(answer) =
                tags::service_call(lua, &this.ctx, this.referent, "GetTags", &args)?
            {
                return Ok(answer);
            }
            tags::get(&this.ctx, this.referent).into_lua_multi(lua)
        });
        // Deprecated: parents the instance to nil, children and all.
        methods.add_method("Remove", |_, this, ()| {
            this.ctx.check_write(this.referent)?;
            this.ctx.dom_mut().set_parent(this.referent, None);
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
                    // Reject an unknown instance before reporting "no parent";
                    // a destroyed one has none.
                    this.class()?;
                    if this.ctx.dom().get(this.referent).is_none() {
                        return Ok(Value::Nil);
                    }
                    let parent = tree::parent_of(&this.ctx.dom(), this.referent);
                    return match parent {
                        Some(referent) => {
                            LuaInstance::new(referent, this.ctx.clone()).into_lua(lua)
                        }
                        // A root is a service (its parent is `game`) or an
                        // instance nothing holds (nil).
                        None if this.ctx.database().is_service(&this.class()?) => {
                            LuaGame::new(this.ctx.clone()).into_lua(lua)
                        }
                        None => Ok(Value::Nil),
                    };
                }
                _ => {}
            }

            if let Some(hooks) = crate::hooks::hooks(lua) {
                let class = this.class()?;
                if let Some(value) = hooks.index(lua, &this.ctx, this.referent, &class, &key)? {
                    return Ok(value);
                }
            }
            if let Some(value) = property::get(lua, &this.ctx, this.referent, &key)? {
                return Ok(value);
            }
            let child = tree::find_child(&this.ctx.dom(), this.referent, &key, false);
            if child.is_none() {
                if let Some(alias) = crate::compat::lowercase_method(lua, &key)? {
                    return Ok(alias);
                }
            }
            match child {
                Some(referent) => LuaInstance::new(referent, this.ctx.clone()).into_lua(lua),
                None => Err(this.not_a_member(&key)),
            }
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, value): (String, Value)| {
                match key.as_str() {
                    "Name" => {
                        let Value::String(text) = &value else {
                            return Err(mlua::Error::runtime(format!(
                                "Unable to assign property Name. string expected, got {}",
                                value.type_name()
                            )));
                        };
                        let text = text.to_str()?.to_string();
                        this.ctx.check_write(this.referent)?;
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
                        this.ctx.check_write(this.referent)?;
                        if let Some(parent) = target {
                            this.ctx.check_write(parent)?;
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

                if let Some(hooks) = crate::hooks::hooks(lua) {
                    let class = this.class()?;
                    if hooks.new_index(lua, &this.ctx, this.referent, &class, &key, &value)? {
                        return Ok(());
                    }
                }
                if property::set(&this.ctx, this.referent, &key, &value)? {
                    return Ok(());
                }
                Err(this.not_a_member(&key))
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
            move |_, (class, parent): (String, Option<Value>)| -> Result<InstanceRef> {
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
                if let Some(parent) = target {
                    ctx.check_write(parent)?;
                }
                let referent = ctx.dom_mut().new_instance(&class, &class, target);
                defaults::apply(&ctx, &class, referent);
                Ok(LuaInstance::new(referent, ctx.clone()))
            },
        )?,
    )?;
    Ok(table)
}
