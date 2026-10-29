use rbx_dom::Variant;

use super::*;

fn add(dom: &mut WeakDom, parent: Option<Ref>, class: &str, name: &str) -> Ref {
    dom.new_instance(class, name, parent)
}

fn script(dom: &mut WeakDom, parent: Ref, class: &str, name: &str, source: &str) -> Ref {
    let r = add(dom, Some(parent), class, name);
    dom.set_property(r, "Source", Variant::String(source.to_string()))
        .unwrap();
    r
}

struct Place {
    dom: WeakDom,
    a: Ref,
    b: Ref,
    c: Ref,
    d: Ref,
    util: Ref,
    inner: Ref,
    orphan: Ref,
    secret: Ref,
    main: Ref,
    client: Ref,
}

/// A, B and C require each other in a ring; D reaches Util and its own
/// child by string; Inner climbs `script.Parent.Parent`; Main resolves
/// through a local and has an asset id and a dynamic path; the client
/// script requires a server-only module.
fn place() -> Place {
    let mut dom = WeakDom::new();
    let replicated = add(&mut dom, None, "ReplicatedStorage", "ReplicatedStorage");
    let server = add(&mut dom, None, "ServerScriptService", "ServerScriptService");
    let player = add(&mut dom, None, "StarterPlayer", "StarterPlayer");
    let player_scripts = add(
        &mut dom,
        Some(player),
        "StarterPlayerScripts",
        "StarterPlayerScripts",
    );
    let shared = add(&mut dom, Some(replicated), "Folder", "Shared");
    let module = "ModuleScript";
    let a = script(
        &mut dom,
        shared,
        module,
        "A",
        "local B = require(script.Parent.B)\nreturn {}",
    );
    let b = script(
        &mut dom,
        shared,
        module,
        "B",
        "local Shared = game:GetService(\"ReplicatedStorage\"):WaitForChild(\"Shared\")\nreturn require(Shared:FindFirstChild(\"C\"))",
    );
    let c = script(
        &mut dom,
        shared,
        module,
        "C",
        "return require(script.Parent.A)",
    );
    let util = script(&mut dom, shared, module, "Util", "return {}");
    let d = script(
        &mut dom,
        shared,
        module,
        "D",
        "local U = require(\"./Util\")\nlocal I = require(\"@self/Inner\")\nreturn {}",
    );
    let inner = script(
        &mut dom,
        d,
        module,
        "Inner",
        "return require(script.Parent.Parent.Util)",
    );
    let orphan = script(&mut dom, shared, module, "Orphan", "return {}");
    let secret = script(&mut dom, server, module, "Secret", "return {}");
    let main = script(
        &mut dom,
        server,
        "Script",
        "Main",
        "local Shared = game.ReplicatedStorage.Shared\nrequire(Shared.D)\nrequire(1234567)\nrequire(Shared[name])\nrequire(script.Parent.Secret)",
    );
    let client = script(
        &mut dom,
        player_scripts,
        "LocalScript",
        "Client",
        "require(game:GetService(\"ServerScriptService\").Secret)\nrequire(game.ReplicatedStorage.Shared.A)",
    );
    Place {
        dom,
        a,
        b,
        c,
        d,
        util,
        inner,
        orphan,
        secret,
        main,
        client,
    }
}

fn edge(graph: &RequireGraph, from: Ref, line: usize) -> &Edge {
    graph
        .edges
        .iter()
        .find(|e| e.from == from && e.line == line)
        .unwrap_or_else(|| panic!("no edge from line {line}"))
}

#[test]
fn instance_paths_resolve_through_services_waits_and_locals() {
    let p = place();
    let graph = require_graph(&p.dom);
    assert_eq!(edge(&graph, p.a, 1).target, Target::Module(p.b));
    assert_eq!(edge(&graph, p.b, 2).target, Target::Module(p.c));
    assert_eq!(edge(&graph, p.main, 2).target, Target::Module(p.d));
    assert_eq!(edge(&graph, p.client, 2).target, Target::Module(p.a));
}

#[test]
fn relative_chains_and_string_paths_resolve() {
    let p = place();
    let graph = require_graph(&p.dom);
    assert_eq!(edge(&graph, p.c, 1).target, Target::Module(p.a));
    assert_eq!(edge(&graph, p.inner, 1).target, Target::Module(p.util));
    assert_eq!(edge(&graph, p.d, 1).target, Target::Module(p.util));
    assert_eq!(edge(&graph, p.d, 2).target, Target::Module(p.inner));
    assert_eq!(edge(&graph, p.main, 5).target, Target::Module(p.secret));
}

#[test]
fn unresolvable_requires_are_unknown_with_their_line_and_text() {
    let p = place();
    let graph = require_graph(&p.dom);
    let asset = edge(&graph, p.main, 3);
    assert_eq!(asset.target, Target::Unknown("asset id"));
    assert_eq!(asset.text, "1234567");
    let dynamic = edge(&graph, p.main, 4);
    assert_eq!(dynamic.target, Target::Unknown("dynamic path"));
    assert_eq!(dynamic.text, "Shared[name]");
}

#[test]
fn a_string_path_that_goes_nowhere_is_unknown() {
    let mut dom = WeakDom::new();
    let storage = add(&mut dom, None, "ReplicatedStorage", "ReplicatedStorage");
    let m = script(
        &mut dom,
        storage,
        "ModuleScript",
        "M",
        "return require(\"./Gone\")",
    );
    let graph = require_graph(&dom);
    assert_eq!(
        edge(&graph, m, 1).target,
        Target::Unknown("unresolved path")
    );
}

#[test]
fn a_ring_of_requires_is_one_cycle_and_only_its_edges_are_marked() {
    let p = place();
    let graph = require_graph(&p.dom);
    assert_eq!(graph.cycles.len(), 1);
    let cycle = &graph.cycles[0];
    assert_eq!(cycle.len(), 3);
    let start = cycle.iter().position(|r| *r == p.a).unwrap();
    let ring: Vec<Ref> = (0..3).map(|i| cycle[(start + i) % 3]).collect();
    assert_eq!(ring, [p.a, p.b, p.c]);
    let marked: HashSet<Ref> = graph
        .edges
        .iter()
        .filter(|e| e.cycle)
        .map(|e| e.from)
        .collect();
    assert_eq!(marked, HashSet::from([p.a, p.b, p.c]));
}

#[test]
fn a_module_requiring_itself_is_a_cycle_of_one() {
    let mut dom = WeakDom::new();
    let storage = add(&mut dom, None, "ReplicatedStorage", "ReplicatedStorage");
    let m = script(
        &mut dom,
        storage,
        "ModuleScript",
        "M",
        "return require(script)",
    );
    let graph = require_graph(&dom);
    assert_eq!(graph.cycles, [vec![m]]);
    assert!(graph.edges[0].cycle);
}

#[test]
fn only_a_module_nothing_requires_is_unused() {
    let p = place();
    let graph = require_graph(&p.dom);
    let unused: Vec<Ref> = graph
        .nodes
        .iter()
        .filter(|n| n.unused)
        .map(|n| n.script)
        .collect();
    assert_eq!(unused, [p.orphan]);
    // Scripts are never unused, and a script requiring nothing isn't drawn.
    assert!(graph.node(p.main).is_some_and(|n| !n.unused));
}

#[test]
fn a_client_requiring_a_server_module_crosses_the_boundary() {
    let p = place();
    let graph = require_graph(&p.dom);
    let crossing: Vec<(Ref, usize)> = graph
        .edges
        .iter()
        .filter(|e| e.crosses)
        .map(|e| (e.from, e.line))
        .collect();
    assert_eq!(crossing, [(p.client, 1)]);
    assert!(graph.node(p.client).unwrap().client);
}
