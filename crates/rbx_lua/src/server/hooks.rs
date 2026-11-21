//! The members a running server adds: `RunService`'s frame signals and
//! run-mode flags, `BasePart`'s touch signals, network ownership and
//! impulses, `PhysicsService`'s collision groups (also on every
//! `WorldRoot`).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use mlua::{Function, IntoLua, Lua, MultiValue, ObjectLike, Result, Value};
use rbx_dom::Ref;

use super::{fire_fresh, Physics};
use crate::ctx::Ctx;
use crate::datatypes::vector3::LuaVector3;
use crate::engine::events::{self, instance_key};
use crate::engine::signal;
use crate::engine::signal::{deferred_signals, LuaSignal, Owner, SignalKey};
use crate::engine::{collection, json};
use crate::hooks::Hooks;
use crate::instance::LuaInstance;
use crate::plugin::api::Arg;
use crate::plugin::arg;

/// The frame signals a server has. `PreRender`/`RenderStepped` exist on
/// the server too but never fire there (Roblox fires them on clients only).
/// `RunService:BindToSimulationParallel`/`BindToAnimationParallel`: only a
/// script under an Actor may bind (a live server's error otherwise,
/// NewApis `bind_sim`); the function then runs every `frequency`'s worth of
/// frames before simulation or animation, in the parallel phase (see
/// `engine::parallel`).
const BIND_PARALLEL: &str = r#"
local name, signal, parallel = ...
local EVERY = { Hz60 = 1, Hz30 = 2, Hz15 = 4, Hz10 = 6, Hz5 = 12, Hz1 = 60 }
return function(self, callback, frequency)
	if not parallel.underActor() then
		error("RunService:" .. name .. ": The calling script must be rooted under an Actor.", 2)
	end
	local every = EVERY[if frequency then frequency.Name else "Hz30"] or 2
	local frame = 0
	callback = parallel.inParallel(callback)
	return self[signal]:Connect(function(dt)
		frame += 1
		if frame % every == 0 then
			callback(dt * every)
		end
	end)
end
"#;

const RUN_SERVICE_SIGNALS: &[&str] = &[
    "PreRender",
    "RenderStepped",
    "PreAnimation",
    "PreSimulation",
    "Stepped",
    "PostSimulation",
    "Heartbeat",
];

/// The collision group calls `WorldRoot` shares with `PhysicsService`.
const WORLD_ROOT_GROUPS: &[&str] = &[
    "RegisterCollisionGroup",
    "UnregisterCollisionGroup",
    "RenameCollisionGroup",
    "CollisionGroupSetCollidable",
    "CollisionGroupsAreCollidable",
    "GetRegisteredCollisionGroups",
    "IsCollisionGroupRegistered",
    "GetMaxCollisionGroups",
];

/// Roblox's cap on collision groups, `PhysicsService:GetMaxCollisionGroups`.
const MAX_GROUPS: usize = 32;
const DEFAULT_GROUP: &str = "Default";

/// `PhysicsService`'s registered groups and the pairs that don't collide.
/// `Default` is always there and can't be removed.
#[derive(Debug, Clone, PartialEq)]
pub struct CollisionGroups {
    names: Vec<String>,
    apart: HashSet<(String, String)>,
}

/// What Roblox says to a script that removes or renames `Default`.
const DEFAULT_LOCKED: &str = "Can not use/change the Default collision group.";

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
            return Err(DEFAULT_LOCKED.to_owned());
        }
        self.names.retain(|n| n != name);
        self.apart.retain(|(a, b)| a != name && b != name);
        Ok(())
    }

    /// The bits of the groups `name` collides with, by index, as Roblox's
    /// `GetRegisteredCollisionGroups` gives them: slots no group holds
    /// are set.
    pub fn mask(&self, name: &str) -> i32 {
        let mut bits = u32::MAX;
        for (index, other) in self.names.iter().enumerate().take(32) {
            if !self.collides(name, other) {
                bits &= !(1 << index);
            }
        }
        bits as i32
    }

    /// Renames a group, its pairs with it; parts keep the old name, which
    /// no longer names a group (so they collide as `Default`). Renaming a
    /// group that isn't there does nothing, as on Roblox.
    fn rename(&mut self, from: &str, to: &str) -> std::result::Result<(), String> {
        if from == DEFAULT_GROUP || to == DEFAULT_GROUP {
            return Err(DEFAULT_LOCKED.to_owned());
        }
        if !self.is_registered(from) {
            return Ok(());
        }
        if self.is_registered(to) {
            return Err(format!("Collision group already exists: {to}"));
        }
        for name in &mut self.names {
            if name == from {
                *name = to.to_owned();
            }
        }
        self.apart = self
            .apart
            .drain()
            .map(|(a, b)| {
                let swap = |n: String| if n == from { to.to_owned() } else { n };
                Self::pair(&swap(a), &swap(b))
            })
            .collect();
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
    /// The change log's length when the write or engine call in progress
    /// started, so a reparenting it made fires at once.
    pub(super) mark: Cell<usize>,
    /// A client's VM: render steps and input work, remotes go to the
    /// server.
    pub(super) client: Rc<Cell<bool>>,
    pub(super) remotes: super::remote::Shared,
    pub(super) chat: super::chat::Chat,
    /// `game:IsLoaded()`: a client's is false until the place arrived.
    pub(super) loaded: Rc<Cell<bool>>,
    pub(super) services: Rc<super::services::Runtime>,
    pub(super) styles: super::styles::Shared,
}

fn method<A, R>(lua: &Lua, f: impl Fn(&Lua, A) -> Result<R> + 'static) -> Result<Option<Value>>
where
    A: mlua::FromLuaMulti,
    R: mlua::IntoLuaMulti,
{
    Ok(Some(Value::Function(lua.create_function(f)?)))
}

/// Destroys the welds and other joints holding `parts` (`BreakJoints`).
fn break_joints(lua: &Lua, physics: &dyn Physics, parts: &[Ref]) -> Result<()> {
    let ctx = crate::engine::engine(lua).ctx.clone();
    for part in parts {
        let joints = match physics.call(*part, "Part", "GetJoints", Vec::new()) {
            Ok(reply) => match reply.values.into_iter().next() {
                Some(Arg::Instances(list)) => list,
                _ => continue,
            },
            Err(_) => continue,
        };
        for joint in joints {
            let rigid = ctx.dom().get(joint).is_some_and(|i| {
                ctx.database().is_subclass_of(i.class(), "JointInstance")
                    || i.class() == "WeldConstraint"
            });
            if rigid {
                let instance = LuaInstance::new(joint, ctx.clone()).into_lua(lua)?;
                if let Value::UserData(data) = instance {
                    data.call_method::<()>("Destroy", ())?;
                }
            }
        }
    }
    Ok(())
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
    /// What the simulation itself answers: its signals, its methods (whose
    /// reply's signals fire before the method returns) and its live
    /// properties.
    fn engine_member(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Result<Option<Value>> {
        if self.physics.engine_signal(class, key) {
            let signal = LuaSignal(instance_key(referent, key));
            return lua
                .create_userdata(signal)
                .map(|u| Some(Value::UserData(u)));
        }
        if self.physics.signal_method(class, key) {
            let key = key.to_owned();
            return method(lua, move |lua, args: MultiValue| {
                let name = match args.get(1) {
                    Some(Value::String(s)) => s.to_str()?.to_string(),
                    _ => return Err(mlua::Error::runtime("Argument 1 missing or nil")),
                };
                let signal = LuaSignal(instance_key(referent, format!("{key}:{name}")));
                lua.create_userdata(signal)
            });
        }
        if self.physics.engine_method(class, key) {
            let physics = self.physics.clone();
            let ctx = ctx.clone();
            let class = class.to_owned();
            let key = key.to_owned();
            let fired = self.fired.clone();
            return method(lua, move |lua, args: MultiValue| {
                let args: Vec<Arg> = args.iter().skip(1).map(arg::from_lua).collect();
                let mut method = key.clone();
                loop {
                    let before = ctx.dom().changes().len();
                    let reply = physics
                        .call(referent, &class, &method, args.clone())
                        .map_err(mlua::Error::runtime)?;
                    // What the call reparented fires first, deferred or
                    // not (a tool's `ChildAdded` before its `Equipped`).
                    let reparented = ctx.dom().changes().get(before..).is_some_and(|fresh| {
                        fresh
                            .iter()
                            .any(|c| matches!(c, rbx_dom::Change::Parent { .. }))
                    });
                    if reparented {
                        super::fire_fresh(lua, &fired)?;
                    } else {
                        crate::hooks::wrote(lua)?;
                    }
                    for (owner, name, args) in &reply.fire {
                        signal::fire(lua, &instance_key(*owner, name.as_str()), args)?;
                    }
                    match reply.and_then {
                        Some(next) => method = next,
                        None => return arg::to_lua_multi(lua, &ctx, &reply.values),
                    }
                }
            });
        }
        match self.physics.engine_property(referent, class, key) {
            Some(value) => arg::to_lua(lua, ctx, &value).map(Some),
            None => Ok(None),
        }
    }

    fn run_service(&self, lua: &Lua, key: &str) -> Result<Option<Value>> {
        if RUN_SERVICE_SIGNALS.contains(&key) {
            let signal = LuaSignal(SignalKey::new(Owner::Service("RunService"), key));
            return lua
                .create_userdata(signal)
                .map(|u| Some(Value::UserData(u)));
        }
        let client = self.client.get();
        match key {
            // Studio's Run and Play: running, in Studio, a server's VM or
            // a client's.
            "IsStudio" | "IsRunning" | "IsRunMode" => flag(lua, true),
            "IsServer" => flag(lua, !client),
            "IsClient" => flag(lua, client),
            "IsEdit" => flag(lua, false),
            "BindToRenderStep" | "UnbindFromRenderStep" if client => {
                super::input::member(lua, None, "RunService", key, Default::default())
            }
            "BindToRenderStep" | "UnbindFromRenderStep" => method(lua, |_, _: MultiValue| {
                Err::<(), _>(mlua::Error::runtime(
                    "BindToRenderStep can only be called from a client",
                ))
            }),
            "BindToSimulationParallel" | "BindToAnimationParallel" => {
                let signal = if key == "BindToSimulationParallel" {
                    "PreSimulation"
                } else {
                    "PreAnimation"
                };
                lua.load(BIND_PARALLEL)
                    .set_name("=RunService")
                    .call::<Function>((key, signal, crate::engine::parallel::module(lua)?))
                    .map(|f| Some(Value::Function(f)))
            }
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
            "SetNetworkOwner" | "SetNetworkOwnershipAuto" => {
                let physics = self.physics.clone();
                let auto = key == "SetNetworkOwnershipAuto";
                let client = self.client.get();
                method(lua, move |_, args: MultiValue| {
                    if client {
                        return Err(mlua::Error::runtime(
                            "Network Ownership API can only be called from the server",
                        ));
                    }
                    let owner = match args.get(1) {
                        Some(Value::UserData(data)) => match data.borrow::<LuaInstance>() {
                            Ok(player) => Some(player.referent()),
                            Err(_) => {
                                return Err(mlua::Error::runtime("Unable to cast value to Object"))
                            }
                        },
                        _ => None,
                    };
                    physics
                        .set_network_owner(referent, owner, auto)
                        .map_err(mlua::Error::runtime)
                })
            }
            "GetNetworkOwner" => {
                let physics = self.physics.clone();
                method(lua, move |lua, _: MultiValue| {
                    let owner = physics
                        .network_owner(referent)
                        .map_err(mlua::Error::runtime)?;
                    let ctx = crate::engine::engine(lua).ctx.clone();
                    match owner {
                        Some(player) => LuaInstance::new(player, ctx).into_lua(lua),
                        None => Ok(Value::Nil),
                    }
                })
            }
            "GetNetworkOwnershipAuto" => {
                let physics = self.physics.clone();
                method(lua, move |_, _: MultiValue| {
                    physics
                        .network_ownership_auto(referent)
                        .map_err(mlua::Error::runtime)
                })
            }
            "CanSetNetworkOwnership" => {
                let physics = self.physics.clone();
                method(lua, move |lua, _: MultiValue| {
                    Ok(match physics.network_ownership_auto(referent) {
                        Ok(_) => (true, Value::Nil),
                        Err(why) => (false, Value::String(lua.create_string(why)?)),
                    })
                })
            }
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
            "BreakJoints" => {
                let physics = self.physics.clone();
                method(lua, move |lua, _: MultiValue| {
                    break_joints(lua, &*physics, &[referent])
                })
            }
            // Surface joints are gone from Roblox: nothing to make.
            "MakeJoints" => method(lua, |_, _: MultiValue| Ok(())),
            "CanCollideWith" => {
                let groups = self.groups.clone();
                method(
                    lua,
                    move |_, (this, other): (mlua::AnyUserData, mlua::AnyUserData)| {
                        let group = |p: &mlua::AnyUserData| p.get::<String>("CollisionGroup");
                        Ok(this.get::<bool>("CanCollide")?
                            && other.get::<bool>("CanCollide")?
                            && groups.borrow().collides(&group(&this)?, &group(&other)?))
                    },
                )
            }
            _ => Ok(None),
        }
    }

    /// `Model:BreakJoints()` (deprecated): every part in it lets go.
    fn model(&self, lua: &Lua, ctx: &Ctx, referent: Ref, key: &str) -> Result<Option<Value>> {
        if key != "BreakJoints" {
            return Ok(None);
        }
        let physics = self.physics.clone();
        let ctx = ctx.clone();
        method(lua, move |lua, _: MultiValue| {
            let parts: Vec<Ref> = {
                let dom = ctx.dom();
                let db = ctx.database();
                let mut parts = Vec::new();
                let mut stack = vec![referent];
                while let Some(at) = stack.pop() {
                    let Some(i) = dom.get(at) else { continue };
                    stack.extend(i.children().iter().copied());
                    if db.is_subclass_of(i.class(), "BasePart") {
                        parts.push(at);
                    }
                }
                parts
            };
            break_joints(lua, &*physics, &parts)
        })
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
            "RenameCollisionGroup" => {
                method(lua, move |_, (_s, from, to): (Value, String, String)| {
                    groups.borrow_mut().rename(&from, &to).map_err(fail)
                })
            }
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
                    entry.set("mask", groups.mask(name))?;
                    list.push(entry)?;
                }
                Ok(list)
            }),
            // The deprecated id-based calls: a group's id is its place in
            // the registered list, Default being 0.
            "CreateCollisionGroup" => method(lua, move |_, (_s, name): (Value, String)| {
                let mut groups = groups.borrow_mut();
                groups.register(&name).map_err(fail)?;
                Ok(groups.names().iter().position(|n| *n == name).unwrap_or(0))
            }),
            "RemoveCollisionGroup" => method(lua, move |_, (_s, name): (Value, String)| {
                groups.borrow_mut().unregister(&name).map_err(fail)
            }),
            "GetCollisionGroupId" => method(lua, move |_, (_s, name): (Value, String)| {
                groups
                    .borrow()
                    .names()
                    .iter()
                    .position(|n| *n == name)
                    .ok_or_else(|| fail(format!("Collision group does not exist: {name}")))
            }),
            "GetCollisionGroupName" => method(lua, move |_, (_s, id): (Value, usize)| {
                groups
                    .borrow()
                    .names()
                    .get(id)
                    .cloned()
                    .ok_or_else(|| fail(format!("Collision group {id} does not exist")))
            }),
            "GetCollisionGroups" => method(lua, move |lua, _: MultiValue| {
                let list = lua.create_table()?;
                let groups = groups.borrow();
                for (id, name) in groups.names().iter().enumerate() {
                    let entry = lua.create_table()?;
                    entry.set("name", name.as_str())?;
                    entry.set("id", id)?;
                    entry.set("mask", groups.mask(name))?;
                    list.push(entry)?;
                }
                Ok(list)
            }),
            "SetPartCollisionGroup" => method(
                lua,
                |_, (_s, part, name): (Value, mlua::AnyUserData, String)| {
                    part.set("CollisionGroup", name)
                },
            ),
            "CollisionGroupContainsPart" => method(
                lua,
                |_, (_s, name, part): (Value, String, mlua::AnyUserData)| {
                    Ok(part.get::<String>("CollisionGroup")? == name)
                },
            ),
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
/// thread waits for a `ChildAdded` of that name and resumes at the next
/// resumption point, after the rest of the replicated batch has landed
/// (a folder's children with it); or after `timeout` seconds with
/// `nil`. With no timeout, Roblox's "Infinite yield possible" warning
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
			task.defer(thread, child)
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

impl Hooks for ServerHooks {
    fn require_asset(&self, id: f64) -> std::result::Result<Ref, String> {
        let reply =
            self.physics
                .call(Ref::new(0), "require", "LoadModule", vec![Arg::Number(id)])?;
        match reply.values.first() {
            Some(Arg::Instance(module)) => Ok(*module),
            _ => Err(format!("Unable to load module asset {id}")),
        }
    }

    fn index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Result<Option<Value>> {
        if let Some(value) = super::remote::member(
            lua,
            ctx,
            &self.remotes,
            self.client.get(),
            referent,
            class,
            key,
        )? {
            return Ok(Some(value));
        }
        if let Some(value) = events::index(lua, ctx, referent, class, key)? {
            return Ok(Some(value));
        }
        if let Some(value) = crate::studio::diagnostics(lua, ctx, referent, class, key)? {
            return Ok(Some(value));
        }
        if key == "WaitForChild" {
            return wait_for_child(lua).map(|f| Some(Value::Function(f)));
        }
        not_scriptable(ctx, referent, class, key)?;
        // Nothing registers `GetCore`'s values on a server, and the call
        // waits forever there (rbx_sim's CoreGuiApi `server_get_core`).
        if class == "StarterGui" && key == "GetCore" && !self.client.get() {
            let start = lua.create_function(|lua, _: MultiValue| {
                Ok(crate::engine::scheduler::new_await(lua))
            })?;
            return crate::engine::scheduler::yielding(lua, start)
                .map(|f| Some(Value::Function(f)));
        }
        if let Some(value) = self.engine_member(lua, ctx, referent, class, key)? {
            return Ok(Some(value));
        }
        if class == "Workspace" {
            if let Some(value) = super::query::member(lua, ctx, &self.physics, key)? {
                return Ok(Some(value));
            }
        }
        if let Some(value) = super::styles::member(lua, ctx, &self.styles, referent, class, key)? {
            return Ok(Some(value));
        }
        if let Some(value) = super::services::world(lua, &self.services, ctx, referent, class, key)?
        {
            return Ok(Some(value));
        }
        if let Some(value) = super::services::member(lua, &self.services, referent, class, key)? {
            return Ok(Some(value));
        }
        let db = ctx.database();
        match class {
            "RunService" => self.run_service(lua, key),
            "SharedTableRegistry" => {
                let name = match key {
                    "GetSharedTable" => "getShared",
                    "SetSharedTable" => "setShared",
                    _ => return Ok(None),
                };
                crate::engine::parallel::module(lua)?
                    .get::<Function>(name)
                    .map(|f| Some(Value::Function(f)))
            }
            "PhysicsService" => self.physics_service(lua, key),
            "Workspace" if key == "GetNumAwakeParts" => {
                let physics = self.physics.clone();
                method(lua, move |_, _: MultiValue| Ok(physics.awake_parts()))
            }
            _ if WORLD_ROOT_GROUPS.contains(&key) && db.is_subclass_of(class, "WorldRoot") => {
                self.physics_service(lua, key)
            }
            "HttpService" => json::member(lua, key),
            "AssetService" => match super::editable_image::member(lua, key)? {
                Some(value) => Ok(Some(value)),
                None => super::meshes::member(lua, ctx, &self.physics, key),
            },
            "Terrain" => match super::terrain::member(lua, ctx, &self.physics, key)? {
                Some(value) => Ok(Some(value)),
                None => self.base_part(lua, referent, key),
            },
            "CollectionService" => collection::member(lua, key),
            "TweenService" | "Tween" => super::tween::member(lua, referent, class, key),
            "TextChatService" | "TextChannel" | "TextChatCommand" | "ChatWindowConfiguration" => {
                super::chat::member(lua, &self.chat, referent, class, key)
            }
            "UserInputService" | "ContextActionService" | "InputObject" if self.client.get() => {
                let signals = (class == "UserInputService").then_some(referent);
                super::input::member(lua, signals, class, key, self.services.device())
            }
            _ if db.is_subclass_of(class, "BasePart") => self.base_part(lua, referent, key),
            "Model" => self.model(lua, ctx, referent, key),
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
        if super::chat::set(lua, &self.chat, referent, class, key, value)? {
            return Ok(true);
        }
        let replaced = match super::services::set(lua, &self.services, referent, class, key, value)?
        {
            super::services::Write::Taken => return Ok(true),
            super::services::Write::Store(replaced) => replaced,
        };
        let value = replaced.as_ref().unwrap_or(value);
        not_scriptable(ctx, referent, class, key)?;
        self.mark.set(ctx.dom().changes().len());
        let mut written = false;
        if let Some(stored) = self
            .physics
            .engine_write(referent, class, key, &arg::from_lua(value))
        {
            let stored = arg::to_lua(lua, ctx, &stored)?;
            if crate::property::set(ctx, referent, key, &stored)? {
                crate::hooks::wrote(lua)?;
                written = true;
            }
        }
        if !written {
            written = events::new_index(lua, ctx, referent, class, key, value)?;
        }
        // The plain write, here rather than the caller's, so what it sets
        // off below runs after it.
        if !written && crate::property::set(ctx, referent, key, value)? {
            crate::hooks::wrote(lua)?;
            written = true;
        }
        if written {
            super::styles::wrote(&self.styles, ctx, referent, class, key);
            for (owner, name, args) in self.physics.property_written(referent, class, key) {
                signal::fire(lua, &instance_key(owner, name.as_str()), &args)?;
            }
        }
        self.mark.set(usize::MAX);
        Ok(written)
    }

    fn service(&self, _lua: &Lua, _ctx: &Ctx, _name: &str) -> Result<Option<Value>> {
        Ok(None)
    }

    fn game_member(&self, lua: &Lua, key: &str) -> Result<Option<Value>> {
        match key {
            "IsLoaded" => flag(lua, self.loaded.get()),
            "Loaded" => lua
                .create_userdata(events::game_signal("Loaded"))
                .map(|u| Some(Value::UserData(u))),
            // Run when the server shuts down (`ServerVm::close`); a client
            // has no shutdown to wait on.
            "BindToClose" => {
                let client = self.client.get();
                method(lua, move |lua, (_this, f): (Value, Function)| {
                    if client {
                        return Err(mlua::Error::runtime(
                            "BindToClose can only be called on the server.",
                        ));
                    }
                    events::registry_table(lua, super::BIND_TO_CLOSE)?.push(f)
                })
            }
            "PlaceId" => Ok(Some(Value::Integer(self.services.place().0))),
            "GameId" => Ok(Some(Value::Integer(self.services.place().1))),
            "PrivateServerId" | "VIPServerId" => Ok(Some(Value::String(lua.create_string("")?))),
            "PrivateServerOwnerId" | "VIPServerOwnerId" => Ok(Some(Value::Integer(0))),
            _ => events::game_member(lua, key),
        }
    }

    /// Outside `Deferred` signal behaviour, a write's events run before the
    /// write returns, as on a real server (the SignalBehavior golden:
    /// `Changed` runs ahead of the line after `value.Value = 1`). A
    /// reparenting's always do, deferred or not (the ReparentOrder golden:
    /// `ChildAdded` and the rest run ahead of the line after `x.Parent = b`).
    fn after_write(&self, lua: &Lua) -> Result<()> {
        let reparented = || {
            let ctx = engine_ctx(lua);
            let dom = ctx.dom();
            let all = dom.changes();
            let parent = |c: &rbx_dom::Change| matches!(c, rbx_dom::Change::Parent { .. });
            // `x.Parent = y` itself (written outside these hooks), or a
            // write here that reparented something.
            (all.len() > self.fired.get() && all.last().is_some_and(parent))
                || all
                    .get(self.mark.get()..)
                    .is_some_and(|fresh| fresh.iter().any(parent))
        };
        if !deferred_signals(lua) || reparented() {
            fire_fresh(lua, &self.fired)?;
        }
        Ok(())
    }
}

fn engine_ctx(lua: &Lua) -> Ctx {
    crate::engine::engine(lua).ctx.clone()
}
