//! Runs plugins against a place without the editor, printing what they say:
//! `cargo run -p rbx_lua --example run_plugin -- place.rbxl plugin.rbxmx…`.
//! Starts every plugin, ticks for a simulated second, then clicks each
//! toolbar button once — enough to see whether a real plugin loads and what
//! of the API it reaches for that is missing.

use std::path::Path;

use rbx_lua::plugin::{Context, Input, PluginHost};
use rbx_reflection::ReflectionDatabase;

fn main() {
    let mut args = std::env::args().skip(1);
    let place = args.next().expect("usage: run_plugin place.rbxl plugin…");
    let bytes = std::fs::read(&place).expect("place must be readable");
    let mut dom = rbx_binary::deserialize(&bytes).expect("place must parse");
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), None);
    for path in args {
        match rbx_lua::plugin::read_source(Path::new(&path)) {
            Ok(source) => {
                host.add(source);
            }
            Err(err) => println!("{path}: {err}"),
        }
    }

    let mut now = 0.0;
    let mut step = |host: &mut PluginHost, input: Input, now: f64| {
        let context = Context {
            now,
            ..Context::default()
        };
        let outcome = host.dispatch(&mut dom, context, input);
        for line in &outcome.output {
            println!("[{}] {:?}: {}", line.source, line.kind, line.text);
        }
        for request in &outcome.requests {
            println!("request: {request:?}");
        }
    };
    step(&mut host, Input::Start, now);
    for _ in 0..10 {
        now += 0.1;
        step(&mut host, Input::Tick, now);
    }
    let buttons: Vec<_> = host
        .ui()
        .toolbars
        .iter()
        .flat_map(|toolbar| toolbar.buttons.iter().map(|b| (b.id, b.text.clone())))
        .collect();
    for (id, text) in buttons {
        println!("-- click {text}");
        step(&mut host, Input::ButtonClicked(id), now);
        now += 0.1;
        step(&mut host, Input::Tick, now);
    }
    let ui = host.ui();
    println!(
        "toolbars: {}, widgets: {:?}",
        ui.toolbars.len(),
        ui.widgets
            .iter()
            .map(|w| (&w.title, w.enabled))
            .collect::<Vec<_>>()
    );
}
