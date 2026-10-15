//! One [`PluginHost::dispatch`]: starting plugins, routing an input to the
//! VMs that care, and turning what they did into an [`Outcome`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Function, Lua, MultiValue, Table};
use rbx_dom::{Change, Ref, Variant, Vector2Data, WeakDom};

use super::{Loaded, PluginHost};
use crate::ctx::Ctx;
use crate::plugin::api::{
    Arg, Context, DebuggerView, Input, LayoutBox, Origin, Outcome, OutputKind, Permission,
    SourceBody,
};
use crate::plugin::env::Env;
use crate::plugin::scheduler::{self, Scheduler};
use crate::plugin::signal::{self, Owner, SignalKey, Signals};
use crate::plugin::{debugger_api, events, loader, plugin_object, services, vm};

const BUDGET: Duration = Duration::from_secs(5);
const ROUNDS: usize = 8;
/// Where `BindToClose` callbacks live in a VM's registry, keyed by the
/// widget root's referent printed with `{:?}`.
const BIND_TO_CLOSE_KEY: &str = "__plugin_bind_to_close";

pub(super) fn run(
    host: &mut PluginHost,
    dom: &mut WeakDom,
    context: Context,
    input: Input,
) -> Outcome {
    std::mem::swap(&mut *host.dom.borrow_mut(), dom);
    let debugger_before = {
        let mut shared = host.shared.borrow_mut();
        let before = std::mem::replace(&mut shared.context, context).debugger;
        shared.deadline = Some(Instant::now() + BUDGET);
        before
    };
    let merge = host.lot.merge(&mut host.dom.borrow_mut());
    let place_roots = merge.place_roots.clone();

    handle(host, input, &debugger_before);

    let mut log = Vec::new();
    for _ in 0..ROUNDS {
        let changes = host.dom.borrow_mut().take_changes();
        if changes.is_empty() {
            break;
        }
        for loaded in &host.plugins {
            if let (Some(lua), Some(env)) = (&loaded.lua, &loaded.env) {
                report(env, events::fire_changes(lua, &changes));
            }
        }
        log.extend(changes);
    }
    log.extend(host.dom.borrow_mut().take_changes());

    mark_views(host, &log, &place_roots);
    let changes = host.lot.split(&mut host.dom.borrow_mut(), merge, log);
    std::mem::swap(&mut *host.dom.borrow_mut(), dom);

    let mut shared = host.shared.borrow_mut();
    shared.deadline = None;
    let selection =
        std::mem::take(&mut shared.selection_written).then(|| shared.context.selection.clone());
    Outcome {
        changes,
        selection,
        output: std::mem::take(&mut shared.output),
        requests: std::mem::take(&mut shared.requests),
        ui_changed: std::mem::take(&mut shared.ui_changed),
        adornments_changed: std::mem::take(&mut shared.adornments_changed),
    }
}

fn report(env: &Env, result: mlua::Result<()>) {
    if let Err(error) = result {
        env.shared
            .borrow_mut()
            .push_output(&env.name, OutputKind::Error, error.to_string());
    }
}

fn vms(host: &PluginHost) -> Vec<(Lua, Rc<Env>)> {
    host.plugins
        .iter()
        .filter_map(|p| Some((p.lua.clone()?, p.env.clone()?)))
        .collect()
}

fn fire_all(host: &PluginHost, key: &SignalKey, args: &[Arg]) {
    for (lua, env) in vms(host) {
        report(&env, signal::fire(&lua, key, args));
    }
}

/// The VM owning the instance tree `referent` hangs in, found by its
/// topmost ancestor in the merged DOM.
fn owner_of(host: &PluginHost, referent: Ref) -> Option<(Lua, Rc<Env>)> {
    let top = {
        let dom = host.dom.borrow();
        let mut current = referent;
        while let Some(parent) = dom.parent(current) {
            current = parent;
        }
        current
    };
    vms(host)
        .into_iter()
        .find(|(_, env)| env.roots.borrow().contains(&top))
}

fn handle(host: &mut PluginHost, input: Input, debugger_before: &DebuggerView) {
    match input {
        Input::Start => {
            for index in 0..host.plugins.len() {
                if !host.plugins[index].info.started && host.plugins[index].source.is_some() {
                    start(host, index);
                }
            }
        }
        Input::Tick => tick(host),
        Input::ButtonClicked(id) => fire_all(host, &SignalKey::new(Owner::Ui(id), "Click"), &[]),
        Input::ActionTriggered(id) => {
            fire_all(host, &SignalKey::new(Owner::Ui(id), "Triggered"), &[])
        }
        Input::SelectionChanged => fire_all(
            host,
            &SignalKey::new(Owner::Service("Selection"), "SelectionChanged"),
            &[],
        ),
        Input::DomChanged(changes) => {
            for (lua, env) in vms(host) {
                report(&env, events::fire_changes(&lua, &changes));
            }
        }
        Input::PromptAnswered(id, answer) => {
            for (lua, env) in vms(host) {
                match plugin_object::answer(&lua, id, &answer) {
                    Ok(true) => break,
                    Ok(false) => {
                        report(&env, services::http_permission_answered(&lua, id, &answer))
                    }
                    Err(error) => report(&env, Err(error)),
                }
            }
        }
        Input::Gui(event) => {
            let targets = owner_of(host, event.referent).map_or_else(|| vms(host), |vm| vec![vm]);
            for (lua, env) in targets {
                report(&env, events::fire_gui(&lua, &event));
            }
        }
        Input::Handle(event) => {
            let targets = owner_of(host, event.referent).map_or_else(|| vms(host), |vm| vec![vm]);
            for (lua, env) in targets {
                report(&env, events::fire_handle(&lua, &event));
            }
        }
        Input::WidgetClosed(id) => close_widget(host, id),
        Input::DebuggerChanged => {
            let after = host.shared.borrow().context.debugger.clone();
            for (lua, env) in vms(host) {
                report(&env, debugger_api::changed(&lua, debugger_before, &after));
            }
        }
        Input::Layout(boxes) => apply_layout(host, &boxes),
        Input::Unload(id) => unload(host, id),
    }
}

fn set_vector2(dom: &mut WeakDom, referent: Ref, name: &str, value: [f32; 2]) {
    let current = dom
        .get(referent)
        .and_then(|i| i.properties().get(name).cloned());
    let new = Variant::Vector2(Vector2Data {
        x: value[0],
        y: value[1],
    });
    if current.as_ref() != Some(&new) {
        let _ = dom.set_property(referent, name, new);
    }
}

fn apply_layout(host: &PluginHost, boxes: &[LayoutBox]) {
    let mut dom = host.dom.borrow_mut();
    let by_ref: HashMap<Ref, &LayoutBox> = boxes.iter().map(|b| (b.referent, b)).collect();
    for b in boxes {
        if dom.get(b.referent).is_none() {
            continue;
        }
        set_vector2(&mut dom, b.referent, "AbsolutePosition", b.position);
        set_vector2(&mut dom, b.referent, "AbsoluteSize", b.size);
        let Some(instance) = dom.get(b.referent) else {
            continue;
        };
        let is_scrolling = instance.class() == "ScrollingFrame";
        let children = instance.children().to_vec();
        let layouts: Vec<Ref> = children
            .iter()
            .copied()
            .filter(|c| {
                dom.get(*c)
                    .is_some_and(|i| matches!(i.class(), "UIListLayout" | "UIGridLayout"))
            })
            .collect();
        if is_scrolling {
            set_vector2(&mut dom, b.referent, "AbsoluteWindowSize", b.size);
        }
        if layouts.is_empty() {
            continue;
        }
        let mut extent = [0.0f32; 2];
        for child in children.iter().filter_map(|c| by_ref.get(c)) {
            extent[0] = extent[0].max(child.position[0] + child.size[0] - b.position[0]);
            extent[1] = extent[1].max(child.position[1] + child.size[1] - b.position[1]);
        }
        for layout in layouts {
            set_vector2(&mut dom, layout, "AbsoluteContentSize", extent);
        }
    }
}

fn start(host: &mut PluginHost, index: usize) {
    let Some(source) = host.plugins[index].source.take() else {
        return;
    };
    let id = host.plugins[index].info.id;
    let (folder, core_gui, plugin_gui_service) = {
        let mut dom = host.dom.borrow_mut();
        // Studio parents a plugin's scripts under a `Plugin` instance.
        let folder = dom.new_instance("Plugin", &source.name, None);
        match &source.body {
            SourceBody::Luau(text) => {
                let script = dom.new_instance("Script", &source.name, Some(folder));
                let _ = dom.set_property(script, "Source", Variant::String(text.clone()));
            }
            SourceBody::Model(model) => {
                for &root in model.root_refs() {
                    loader::copy_tree(model, root, &mut dom, Some(folder));
                }
            }
        }
        let core_gui = dom.new_instance("Folder", "CoreGui", None);
        let plugin_gui_service = dom.new_instance("Folder", "PluginGuiService", None);
        (folder, core_gui, plugin_gui_service)
    };
    let manifest = match &source.origin {
        Origin::Native { manifest, .. } => Some(manifest.clone()),
        Origin::Roblox { .. } => None,
    };
    let mut ctx = Ctx::new(
        host.dom.clone(),
        host.database.clone(),
        Rc::new(RefCell::new(Vec::new())),
        Rc::new(RefCell::new(Vec::new())),
    );
    if let Some(manifest) = &manifest {
        if !manifest.permissions.contains(&Permission::DomWrite) {
            ctx = ctx.with_guard(Rc::new(crate::plugin::guard::place_writes_denied(
                source.name.clone(),
            )));
        }
    }
    let env = Rc::new(Env {
        plugin: id,
        name: source.name.clone(),
        ctx,
        shared: host.shared.clone(),
        signals: RefCell::new(Signals::default()),
        scheduler: RefCell::new(Scheduler::default()),
        manifest,
        roots: RefCell::new(vec![folder, core_gui, plugin_gui_service]),
        modules: RefCell::new(HashMap::new()),
        core_gui,
        plugin_gui_service,
    });
    let loaded: &mut Loaded = &mut host.plugins[index];
    loaded.info.started = true;
    loaded.env = Some(env.clone());
    let lua = match vm::build(env.clone()) {
        Ok(lua) => lua,
        Err(error) => {
            let text = error.to_string();
            env.shared
                .borrow_mut()
                .push_output(&env.name, OutputKind::Error, text.clone());
            loaded.info.error = Some(text);
            return;
        }
    };
    loaded.lua = Some(lua.clone());
    let scripts = loader::scripts_to_run(&host.dom.borrow(), &[folder]);
    for script in scripts {
        if let Err(error) = vm::run_script(&lua, script) {
            let text = error.to_string();
            env.shared
                .borrow_mut()
                .push_output(&env.name, OutputKind::Error, text.clone());
            host.plugins[index].info.error = Some(text);
        }
    }
}

fn tick(host: &mut PluginHost) {
    let finished = host.shared.borrow_mut().http.poll();
    let mut by_plugin: HashMap<u32, Vec<_>> = HashMap::new();
    for done in finished {
        by_plugin.entry(done.0).or_default().push(done);
    }
    let all = vms(host);
    for (plugin, done) in by_plugin {
        if let Some((lua, env)) = all.iter().find(|(_, env)| env.plugin == plugin) {
            report(env, services::http_finished(lua, done));
        }
    }

    let now = host.shared.borrow().context.now;
    let dt = host.last_tick.map_or(0.0, |last| (now - last).max(0.0));
    host.last_tick = Some(now);
    let run_service = |event: &str| SignalKey::new(Owner::Service("RunService"), event);
    for (lua, env) in all {
        report(&env, scheduler::tick(&lua, now));
        report(
            &env,
            signal::fire(&lua, &run_service("Heartbeat"), &[Arg::Number(dt)]),
        );
        report(
            &env,
            signal::fire(
                &lua,
                &run_service("Stepped"),
                &[Arg::Number(now), Arg::Number(dt)],
            ),
        );
        report(
            &env,
            signal::fire(&lua, &run_service("RenderStepped"), &[Arg::Number(dt)]),
        );
    }
}

fn close_widget(host: &mut PluginHost, id: u32) {
    let root = {
        let mut shared = host.shared.borrow_mut();
        let Some(widget) = shared.ui.widgets.iter_mut().find(|w| w.id == id) else {
            return;
        };
        widget.enabled = false;
        let root = widget.root;
        shared.ui_changed = true;
        root
    };
    let _ = host
        .dom
        .borrow_mut()
        .set_property(root, "Enabled", Variant::Bool(false));
    let Some((lua, env)) = owner_of(host, root) else {
        return;
    };
    let callback = lua
        .named_registry_value::<Option<Table>>(BIND_TO_CLOSE_KEY)
        .ok()
        .flatten()
        .and_then(|table| {
            table
                .get::<Option<Function>>(format!("{root:?}"))
                .ok()
                .flatten()
        });
    if let Some(callback) = callback {
        report(&env, scheduler::spawn(&lua, callback, MultiValue::new()));
    }
}

fn unload(host: &mut PluginHost, id: u32) {
    let Some(index) = host.plugins.iter().position(|p| p.info.id == id) else {
        return;
    };
    if let (Some(lua), Some(env)) = (
        host.plugins[index].lua.clone(),
        host.plugins[index].env.clone(),
    ) {
        report(
            &env,
            signal::fire(&lua, &SignalKey::new(Owner::Plugin, "Unloading"), &[]),
        );
        signal::disconnect_all(&lua);
    }
    if let Some(env) = host.plugins[index].env.take() {
        let mut dom = host.dom.borrow_mut();
        for &root in env.roots.borrow().iter() {
            dom.remove(root);
        }
    }
    host.plugins[index].lua = None;
    let mut shared = host.shared.borrow_mut();
    let gone: HashSet<Ref> = shared
        .ui
        .widgets
        .iter()
        .filter(|w| w.plugin == id)
        .map(|w| w.root)
        .collect();
    shared.ui.toolbars.retain(|t| t.plugin != id);
    shared.ui.actions.retain(|a| a.plugin != id);
    shared.ui.widgets.retain(|w| w.plugin != id);
    shared.widget_roots.retain(|r| !gone.contains(r));
    shared.owners.retain(|_, owner| *owner != id);
    shared.ui_changed = true;
}

/// Bumps the revision of every widget whose subtree a change touched and
/// flags adornments for changes on other plugin-owned instances, reading
/// the merged DOM before the lot is split.
fn mark_views(host: &PluginHost, log: &[Change], place_roots: &HashSet<Ref>) {
    let dom = host.dom.borrow();
    let mut shared = host.shared.borrow_mut();
    for change in log {
        let referent = match change {
            Change::Property { referent, .. }
            | Change::Parent { referent, .. }
            | Change::Class(referent)
            | Change::Added(referent)
            | Change::Removed(referent) => *referent,
        };
        if dom.get(referent).is_none() {
            continue;
        }
        let mut chain = vec![referent];
        while let Some(parent) = dom.parent(chain[chain.len() - 1]) {
            chain.push(parent);
        }
        let widget_root = chain
            .iter()
            .copied()
            .find(|r| shared.widget_roots.contains(r));
        match widget_root {
            Some(root) => {
                let mut bumped = false;
                for widget in shared.ui.widgets.iter_mut().filter(|w| w.root == root) {
                    widget.revision += 1;
                    bumped = true;
                }
                shared.ui_changed |= bumped;
            }
            None => {
                if chain.last().is_some_and(|top| !place_roots.contains(top)) {
                    shared.adornments_changed = true;
                }
            }
        }
    }
}
