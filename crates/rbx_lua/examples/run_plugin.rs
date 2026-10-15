//! Runs plugins against a place without the editor, printing what they say:
//! `cargo run -p rbx_lua --example run_plugin -- place.rbxl plugin.rbxmx…`.
//! Starts every plugin, ticks for a simulated second, then clicks each
//! toolbar button once — enough to see whether a real plugin loads and what
//! of the API it reaches for that is missing.
//!
//! Two optional steps follow, set through the environment:
//! `RUN_PLUGIN_SELECT=<name>` selects the first place instance so named,
//! and `RUN_PLUGIN_CLICK=<text>` clicks the first widget element whose
//! `Text` starts with it, then ticks for two simulated seconds, printing
//! how many changes each tick made.

use std::path::Path;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::plugin::{Context, GuiEvent, GuiEventKind, Input, Outcome, PluginHost};
use rbx_reflection::ReflectionDatabase;

struct Run {
    host: PluginHost,
    dom: WeakDom,
    now: f64,
    selection: Vec<Ref>,
}

impl Run {
    fn step(&mut self, input: Input) -> Outcome {
        let context = Context {
            now: self.now,
            selection: self.selection.clone(),
            ..Context::default()
        };
        let outcome = self.host.dispatch(&mut self.dom, context, input);
        if let Some(selection) = &outcome.selection {
            self.selection = selection.clone();
        }
        for line in &outcome.output {
            println!("[{}] {:?}: {}", line.source, line.kind, line.text);
        }
        for request in &outcome.requests {
            println!("request: {request:?}");
        }
        outcome
    }

    fn tick(&mut self) -> Outcome {
        self.now += 0.05;
        self.step(Input::Tick)
    }
}

fn find(dom: &WeakDom, test: &dyn Fn(&rbx_dom::Instance) -> bool) -> Option<Ref> {
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    stack.reverse();
    while let Some(referent) = stack.pop() {
        let instance = dom.get(referent)?;
        if test(instance) {
            return Some(referent);
        }
        stack.extend(instance.children().iter().rev());
    }
    None
}

fn main() {
    let mut args = std::env::args().skip(1);
    let place = args.next().expect("usage: run_plugin place.rbxl plugin…");
    let bytes = std::fs::read(&place).expect("place must be readable");
    let dom = rbx_binary::deserialize(&bytes).expect("place must parse");
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), None);
    for path in args {
        match rbx_lua::plugin::read_source(Path::new(&path)) {
            Ok(source) => {
                host.add(source);
            }
            Err(err) => println!("{path}: {err}"),
        }
    }
    let mut run = Run {
        host,
        dom,
        now: 0.0,
        selection: Vec::new(),
    };

    run.step(Input::Start);
    for _ in 0..20 {
        run.tick();
    }
    let buttons: Vec<_> = run
        .host
        .ui()
        .toolbars
        .iter()
        .flat_map(|toolbar| toolbar.buttons.iter().map(|b| (b.id, b.text.clone())))
        .collect();
    for (id, text) in buttons {
        println!("-- click {text}");
        run.step(Input::ButtonClicked(id));
        run.tick();
    }
    let ui = run.host.ui();
    println!(
        "toolbars: {}, widgets: {:?}",
        ui.toolbars.len(),
        ui.widgets
            .iter()
            .map(|w| (&w.title, w.enabled))
            .collect::<Vec<_>>()
    );

    if let Ok(name) = std::env::var("RUN_PLUGIN_SELECT") {
        let found = find(&run.dom, &|i| i.name() == name);
        println!("-- select {name}: {found:?}");
        run.selection = found.into_iter().collect();
        run.step(Input::SelectionChanged);
        run.tick();
    }
    if let Ok(text) = std::env::var("RUN_PLUGIN_CLICK") {
        let target = ui.widgets.iter().find_map(|w| {
            let copy = run.host.widget_dom(w.id)?;
            find(&copy, &|i| {
                matches!(i.properties().get("Text"), Some(Variant::String(t)) if t.starts_with(&text))
            })
        });
        println!("-- click {text}: {target:?}");
        if let Some(referent) = target {
            let at = |kind| Input::Gui(GuiEvent { referent, kind });
            run.step(at(GuiEventKind::Down {
                button: 1,
                x: 1.0,
                y: 1.0,
            }));
            run.step(at(GuiEventKind::Up {
                button: 1,
                x: 1.0,
                y: 1.0,
            }));
            run.step(at(GuiEventKind::Click {
                button: 1,
                x: 1.0,
                y: 1.0,
            }));
            for tick in 0..40 {
                let outcome = run.tick();
                if !outcome.changes.is_empty() {
                    println!("tick {tick}: {} changes", outcome.changes.len());
                }
            }
        }
        if let Ok(name) = std::env::var("RUN_PLUGIN_SELECT") {
            let mut count = 0;
            let mut stack: Vec<Ref> = run.dom.root_refs().to_vec();
            while let Some(r) = stack.pop() {
                if let Some(i) = run.dom.get(r) {
                    if i.name() == name {
                        count += 1;
                        if std::env::var_os("RUN_PLUGIN_POSITIONS").is_some() {
                            if let Some(Variant::CFrame(f)) = i.properties().get("CFrame") {
                                let p = f.position;
                                println!("  {:.2} {:.2} {:.2}", p.x, p.y, p.z);
                            }
                        }
                    }
                    stack.extend_from_slice(i.children());
                }
            }
            println!("instances named {name}: {count}");
        }
    }
}
