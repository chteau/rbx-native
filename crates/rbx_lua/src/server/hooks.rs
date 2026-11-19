//! The members a running server adds: `RunService`'s frame signals and
//! run-mode flags, `BasePart`'s touch signals, network ownership and
//! impulses, `PhysicsService`'s collision groups.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use mlua::{Function, Lua, MultiValue, Result, Value};
use rbx_dom::Ref;

use super::{fire_fresh, Physics};
use crate::ctx::Ctx;
use crate::datatypes::vector3::LuaVector3;
use crate::engine::events::{self, instance_key};
use crate::engine::signal::{deferred_signals, LuaSignal, Owner, SignalKey};
use crate::engine::{collection, json};
use crate::hooks::Hooks;

/// The frame signals a server has. `PreRender`/`RenderStepped` exist on
/// the server too but never fire there (Roblox fires them on clients only).
const RUN_SERVICE_SIGNALS: &[&str] = &[
    "PreRender",
    "RenderStepped",
    "PreAnimation",
    "PreSimulation",
    "Stepped",
    "PostSimulation",
    "Heartbeat",
];

/// Roblox's cap on collision groups, `PhysicsService:GetMaxCollisionGroups`.
const MAX_GROUPS: usize = 32;
const DEFAULT_GROUP: &str = "Default";

/// `PhysicsService`'s registered groups and the pairs that don't collide.
/// `Default` is always there and can't be removed.
#[derive(Debug, Clone)]
pub struct CollisionGroups {
    names: Vec<String>,
    apart: HashSet<(String, String)>,
}

impl Default for CollisionGroups {
    fn default() -> Self {
        CollisionGroups {
            names: vec![DEFAULT_GROUP.to_owned()],
            apart: HashSet::new(),
        }
    }
}

impl CollisionGroups {
    fn pair(a: &str, b: &str) -> (String, String) {
        if a <= b {
            (a.to_owned(), b.to_owned())
        } else {
            (b.to_owned(), a.to_owned())
        }
    }

    pub fn is_registered(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }

    /// Whether parts in `a` and `b` collide. An unregistered name counts as
    /// `Default`, which is what a part in a removed group falls back to.
    pub fn collides(&self, a: &str, b: &str) -> bool {
        let known = |n: &str| {
            if self.is_registered(n) {
                n.to_owned()
            } else {
                DEFAULT_GROUP.to_owned()
            }
        };
        !self.apart.contains(&Self::pair(&known(a), &known(b)))
    }

    /// The index of a group, for the physics engine's bit masks.
    pub fn index(&self, name: &str) -> usize {
        self.names.iter().position(|n| n == name).unwrap_or(0)
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    fn register(&mut self, name: &str) -> std::result::Result<(), String> {
        if self.is_registered(name) {
            return Ok(());
        }
        if self.names.len() >= MAX_GROUPS {
            return Err(format!(
                "Could not create collision group, one with that name already exists or the limit of {MAX_GROUPS} groups was reached."
            ));
        }
        self.names.push(name.to_owned());
        Ok(())
    }

    fn unregister(&mut self, name: &str) -> std::result::Result<(), String> {
        if name == DEFAULT_GROUP {
            return Err("Cannot remove the Default collision group".to_owned());
        }
        self.names.retain(|n| n != name);
        self.apart.retain(|(a, b)| a != name && b != name);
        Ok(())
    }

    fn set_collidable(&mut self, a: &str, b: &str, on: bool) -> std::result::Result<(), String> {
        for name in [a, b] {
            if !self.is_registered(name) {
                return Err(format!("Collision group does not exist: {name}"));
            }
        }
        let pair = Self::pair(a, b);
        if on {
            self.apart.remove(&pair);
        } else {
            self.apart.insert(pair);
        }
        Ok(())
    }
}

pub(super) struct ServerHooks {
    pub(super) physics: Rc<dyn Physics>,
    pub(super) groups: Rc<RefCell<CollisionGroups>>,
    pub(super) fired: Rc<Cell<usize>>,
}

fn method<A, R>(lua: &Lua, f: impl Fn(&Lua, A) -> Result<R> + 'static) -> Result<Option<Value>>
where
    A: mlua::FromLuaMulti,
    R: mlua::IntoLuaMulti,
{
    Ok(Some(Value::Function(lua.create_function(f)?)))
}

fn flag(lua: &Lua, on: bool) -> Result<Option<Value>> {
    method(lua, move |_, _: MultiValue| Ok(on))
}

fn vector(value: &Value) -> Result<[f64; 3]> {
    match value {
        Value::UserData(data) if data.is::<LuaVector3>() => {
            let v = data.borrow::<LuaVector3>()?.0;
            Ok([v.x as f64, v.y as f64, v.z as f64])
        }
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to Vector3",
            other.type_name()
        ))),
    }
}

impl ServerHooks {
    fn run_service(&self, lua: &Lua, key: &str) -> Result<Option<Value>> {
        if RUN_SERVICE_SIGNALS.contains(&key) {
            let signal = LuaSignal(SignalKey::new(Owner::Service("RunService"), key));
            return lua
                .create_userdata(signal)
                .map(|u| Some(Value::UserData(u)));
        }
        match key {
            // Studio's Run: a server that is running, in Studio.
            "IsServer" | "IsStudio" | "IsRunning" | "IsRunMode" => flag(lua, true),
            "IsClient" | "IsEdit" => flag(lua, false),
            "BindToRenderStep" | "UnbindFromRenderStep" => method(lua, |_, _: MultiValue| {
                Err::<(), _>(mlua::Error::runtime(
                    "BindToRenderStep can only be called from a client",
                ))
            }),
            _ => Ok(None),
        }
    }

    fn base_part(&self, lua: &Lua, referent: Ref, key: &str) -> Result<Option<Value>> {
        match key {
            "Touched" | "TouchEnded" => {
                let signal = LuaSignal(instance_key(referent, key));
                lua.create_userdata(signal)
                    .map(|u| Some(Value::UserData(u)))
            }
            // One machine: the server owns everything, nothing to hand over.
            "SetNetworkOwner" | "SetNetworkOwnershipAuto" => method(lua, |_, _: MultiValue| Ok(())),
            "GetNetworkOwner" => method(lua, |_, _: MultiValue| Ok(Value::Nil)),
            "GetNetworkOwnershipAuto" => flag(lua, true),
            "CanSetNetworkOwnership" => method(lua, |_, _: MultiValue| Ok((true, Value::Nil))),
            "GetMass" | "Mass" | "AssemblyMass" => {
                let physics = self.physics.clone();
                let assembly = key == "AssemblyMass";
                let mass = move || {
                    if assembly {
                        physics.assembly_mass(referent)
                    } else {
                        physics.mass(referent)
                    }
                    .unwrap_or(0.0)
                };
                if key == "GetMass" {
                    method(lua, move |_, _: MultiValue| Ok(mass()))
                } else {
                    Ok(Some(Value::Number(mass())))
                }
            }
            "ApplyImpulse" | "ApplyAngularImpulse" => {
                let physics = self.physics.clone();
                let angular = key == "ApplyAngularImpulse";
                method(lua, move |_, (_this, impulse): (Value, Value)| {
                    physics.apply_impulse(referent, vector(&impulse)?, angular);
                    Ok(())
                })
            }
            _ => Ok(None),
        }
    }

    fn physics_service(&self, lua: &Lua, key: &str) -> Result<Option<Value>> {
        let groups = self.groups.clone();
        let fail = |e: String| mlua::Error::runtime(e);
        match key {
            "RegisterCollisionGroup" => method(lua, move |_, (_s, name): (Value, String)| {
                groups.borrow_mut().register(&name).map_err(fail)
            }),
            "UnregisterCollisionGroup" => method(lua, move |_, (_s, name): (Value, String)| {
                groups.borrow_mut().unregister(&name).map_err(fail)
            }),
            "CollisionGroupSetCollidable" => method(
                lua,
                move |_, (_s, a, b, on): (Value, String, String, bool)| {
                    groups.borrow_mut().set_collidable(&a, &b, on).map_err(fail)
                },
            ),
            "CollisionGroupsAreCollidable" => {
                method(lua, move |_, (_s, a, b): (Value, String, String)| {
                    let groups = groups.borrow();
                    for name in [&a, &b] {
                        if !groups.is_registered(name) {
                            return Err(fail(format!("Collision group does not exist: {name}")));
                        }
                    }
                    Ok(groups.collides(&a, &b))
                })
            }
            "IsCollisionGroupRegistered" => method(lua, move |_, (_s, name): (Value, String)| {
                Ok(groups.borrow().is_registered(&name))
            }),
            "GetMaxCollisionGroups" => method(lua, |_, _: MultiValue| Ok(MAX_GROUPS)),
            "GetRegisteredCollisionGroups" => method(lua, move |lua, _: MultiValue| {
                let groups = groups.borrow();
                let list = lua.create_table()?;
                for name in groups.names() {
                    let entry = lua.create_table()?;
                    entry.set("name", name.as_str())?;
                    entry.set("mask", 0)?;
                    list.push(entry)?;
                }
                Ok(list)
            }),
            _ => Ok(None),
        }
    }
}

/// Game scripts can neither read nor write a property the API dump tags
/// `NotScriptable` (`Workspace.SignalBehavior`): Roblox answers as if the
/// member did not exist, which the SignalBehavior golden shows.
fn not_scriptable(ctx: &Ctx, referent: Ref, class: &str, key: &str) -> Result<()> {
    let hidden = ctx
        .database()
        .resolve_property(class, key)
        .is_some_and(|p| p.tags.iter().any(|t| t == "NotScriptable"));
    if !hidden {
        return Ok(());
    }
    let name = ctx
        .dom()
        .get(referent)
        .map_or_else(String::new, |i| i.name().to_owned());
    Err(mlua::Error::runtime(format!(
        "{key} is not a valid member of {class} \"{name}\""
    )))
}

/// `WaitForChild(name, timeout?)`: the child when it is there, else the
/// thread waits for a `ChildAdded` of that name, or `timeout` seconds and
/// then `nil`. With no timeout, Roblox's "Infinite yield possible" warning
/// after 5 seconds, and the wait goes on.
const WAIT_FOR_CHILD: &str = r#"
return function(self, name, timeout)
	local found = self:FindFirstChild(name)
	if found then
		return found
	end
	local thread = coroutine.running()
	local done = false
	local conn
	conn = self.ChildAdded:Connect(function(child)
		if not done and child.Name == name then
			done = true
			conn:Disconnect()
			task.spawn(thread, child)
		end
	end)
	if timeout then
		task.delay(timeout, function()
			if not done then
				done = true
				conn:Disconnect()
				task.spawn(thread, nil)
			end
		end)
	else
		task.delay(5, function()
			if not done then
				warn(string.format("Infinite yield possible on '%s:WaitForChild(\"%s\")'", self:GetFullName(), name))
			end
		end)
	end
	return coroutine.yield()
end
"#;

fn wait_for_child(lua: &Lua) -> Result<Function> {
    const KEY: &str = "rbxn.server.wait_for_child";
    if let Some(f) = lua.named_registry_value::<Option<Function>>(KEY)? {
        return Ok(f);
    }
    let f: Function = lua.load(WAIT_FOR_CHILD).set_name("=WaitForChild").eval()?;
    lua.set_named_registry_value(KEY, f.clone())?;
    Ok(f)
}

/// Roblox's error for an HTTP request while `HttpEnabled` is off; a local
/// Run has no network to reach either way.
fn http_member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    if let Some(member) = json::member(lua, key)? {
        return Ok(Some(member));
    }
    if !matches!(key, "GetAsync" | "PostAsync" | "RequestAsync") {
        return Ok(None);
    }
    let f: Function = lua.create_function(|_, _: MultiValue| -> Result<()> {
        Err(mlua::Error::runtime(
            "Http requests are not enabled. Enable via game settings",
        ))
    })?;
    Ok(Some(Value::Function(f)))
}

impl Hooks for ServerHooks {
    fn index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Result<Option<Value>> {
        if let Some(value) = events::index(lua, ctx, referent, class, key)? {
            return Ok(Some(value));
        }
        if key == "WaitForChild" {
            return wait_for_child(lua).map(|f| Some(Value::Function(f)));
        }
        not_scriptable(ctx, referent, class, key)?;
        let db = ctx.database();
        match class {
            "RunService" => self.run_service(lua, key),
            "PhysicsService" => self.physics_service(lua, key),
            "HttpService" => http_member(lua, key),
            "CollectionService" => collection::member(lua, key),
            _ if db.is_subclass_of(class, "BasePart") => self.base_part(lua, referent, key),
            _ => Ok(None),
        }
    }

    fn new_index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
        value: &Value,
    ) -> Result<bool> {
        not_scriptable(ctx, referent, class, key)?;
        events::new_index(lua, ctx, referent, class, key, value)
    }

    fn service(&self, _lua: &Lua, _ctx: &Ctx, _name: &str) -> Result<Option<Value>> {
        Ok(None)
    }

    fn game_member(&self, lua: &Lua, key: &str) -> Result<Option<Value>> {
        events::game_member(lua, key)
    }

    /// Outside `Deferred` signal behaviour, a write's events run before the
    /// write returns, as on a real server (the SignalBehavior golden:
    /// `Changed` runs ahead of the line after `value.Value = 1`).
    fn after_write(&self, lua: &Lua) -> Result<()> {
        if !deferred_signals(lua) {
            fire_fresh(lua, &self.fired)?;
        }
        Ok(())
    }
}
