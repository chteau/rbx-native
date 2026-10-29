//! The module require graph: every script and `ModuleScript`, with an edge
//! for each `require(...)` call. Targets are resolved by the audit's own
//! walk, so the same instance paths it checks resolve here
//! (`game:GetService`, `WaitForChild`, `FindFirstChild`, dot chains,
//! `script.Parent`, locals bound to any of them), plus Luau's
//! require-by-string paths the way Roblox reads them: `./Name` a sibling,
//! `../` one level up, `@self/Name` a child of the script, `@game/Service/…`.
//!
//! On top of the edges it finds:
//!
//! - **cycles**: every edge inside a strongly connected group of modules
//!   lies on a cycle, and each group gets one cycle path to show;
//! - **unused modules**: a `ModuleScript` no resolved edge points at;
//! - **boundary crossings**: a client script requiring a module under
//!   `ServerScriptService` or `ServerStorage`, which the client never
//!   receives, so the require fails at run time.

use std::collections::{HashMap, HashSet, VecDeque};

use rbx_dom::{Ref, WeakDom};

use crate::resolve::Val;
use crate::{full_name, parse, walk_all, RequireSite, Side};

/// Where a `require` leads.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Module(Ref),
    /// Not statically resolvable, with why: a dynamic path, an asset id, a
    /// path that leaves the place, or an instance that isn't a module.
    Unknown(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub from: Ref,
    pub target: Target,
    /// 1-based line of the call.
    pub line: usize,
    /// The argument as written.
    pub text: String,
    /// On a require cycle.
    pub cycle: bool,
    /// A client script requiring a server-only module.
    pub crosses: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub script: Ref,
    pub name: String,
    /// `Instance:GetFullName()`.
    pub path: String,
    pub class: String,
    /// Runs on the client (a `LocalScript`, a client `RunContext`, or a
    /// module under a client container).
    pub client: bool,
    /// A `ModuleScript` nothing requires.
    pub unused: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RequireGraph {
    /// Every `ModuleScript`, and every other script that requires
    /// something, sorted by path.
    pub nodes: Vec<Node>,
    /// Sorted by the requiring script's path, then line.
    pub edges: Vec<Edge>,
    /// One cycle path per group of modules requiring each other, each
    /// module requiring the next and the last the first.
    pub cycles: Vec<Vec<Ref>>,
}

impl RequireGraph {
    pub fn node(&self, script: Ref) -> Option<&Node> {
        self.nodes.iter().find(|node| node.script == script)
    }
}

/// The require graph of every script in `dom`.
pub fn require_graph(dom: &WeakDom) -> RequireGraph {
    let (scripts, _) = parse(dom);
    let (_, usage) = walk_all(dom, &scripts);
    let sides: HashMap<Ref, Side> = scripts.iter().map(|(r, side, _)| (*r, *side)).collect();

    let mut seen = HashSet::new();
    let mut edges: Vec<Edge> = usage
        .requires
        .into_iter()
        // A named handler is walked twice, see `audit`.
        .filter(|site| seen.insert((site.from, site.line, site.text.clone())))
        .map(|site| {
            let target = target(dom, &site);
            let client = sides.get(&site.from) == Some(&Side::Client);
            let crosses = client && matches!(target, Target::Module(m) if server_only(dom, m));
            Edge {
                from: site.from,
                target,
                line: site.line,
                text: site.text,
                cycle: false,
                crosses,
            }
        })
        .collect();

    let mut members: Vec<Ref> = scripts
        .iter()
        .filter(|(r, _, _)| is_module(dom, *r) || edges.iter().any(|e| e.from == *r))
        .map(|(r, _, _)| *r)
        .collect();
    members.extend(edges.iter().filter_map(|e| match e.target {
        Target::Module(m) => Some(m),
        Target::Unknown(_) => None,
    }));
    members.sort_by_cached_key(|r| full_name(dom, *r));
    members.dedup();

    let required: HashSet<Ref> = edges
        .iter()
        .filter_map(|e| match e.target {
            Target::Module(m) => Some(m),
            Target::Unknown(_) => None,
        })
        .collect();
    let nodes = members
        .iter()
        .filter_map(|&r| {
            let instance = dom.get(r)?;
            Some(Node {
                script: r,
                name: instance.name().to_string(),
                path: full_name(dom, r),
                class: instance.class().to_string(),
                client: sides.get(&r) == Some(&Side::Client),
                unused: is_module(dom, r) && !required.contains(&r),
            })
        })
        .collect();

    let groups = strongly_connected(&members, &edges);
    let group_of: HashMap<Ref, usize> = groups
        .iter()
        .enumerate()
        .flat_map(|(i, group)| group.iter().map(move |r| (*r, i)))
        .collect();
    for edge in &mut edges {
        if let Target::Module(m) = edge.target {
            edge.cycle = group_of
                .get(&edge.from)
                .is_some_and(|g| group_of.get(&m) == Some(g));
        }
    }
    let cycles = groups
        .iter()
        .filter_map(|group| cycle_path(group, &edges))
        .collect();

    let order: HashMap<Ref, usize> = members.iter().enumerate().map(|(i, r)| (*r, i)).collect();
    edges.sort_by_key(|e| (order.get(&e.from).copied(), e.line));
    RequireGraph {
        nodes,
        edges,
        cycles,
    }
}

fn is_module(dom: &WeakDom, r: Ref) -> bool {
    dom.get(r).is_some_and(|i| i.class() == "ModuleScript")
}

fn target(dom: &WeakDom, site: &RequireSite) -> Target {
    let module = |r: Ref| {
        if is_module(dom, r) {
            Target::Module(r)
        } else {
            Target::Unknown("not a ModuleScript")
        }
    };
    if let Some(path) = &site.literal {
        return by_string(dom, site.from, path).map_or(Target::Unknown("unresolved path"), module);
    }
    match &site.target {
        Val::Inst(r) => module(*r),
        Val::Missing { .. } => Target::Unknown("no such instance"),
        _ if !site.text.is_empty() && site.text.chars().all(|c| c.is_ascii_digit()) => {
            Target::Unknown("asset id")
        }
        _ => Target::Unknown("dynamic path"),
    }
}

/// Luau's require-by-string, resolved against `script` as Roblox does.
fn by_string(dom: &WeakDom, script: Ref, path: &str) -> Option<Ref> {
    let child = |at: Ref, name: &str| {
        dom.get(at)?
            .children()
            .iter()
            .copied()
            .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
    };
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let mut at = match parts.next()? {
        "." => dom.parent(script)?,
        ".." => dom.parent(dom.parent(script)?)?,
        "@self" => script,
        "@game" => {
            let service = parts.next()?;
            dom.root_refs()
                .iter()
                .copied()
                .find(|r| dom.get(*r).is_some_and(|i| i.name() == service))?
        }
        _ => return None,
    };
    for part in parts {
        at = match part {
            "." => at,
            ".." => dom.parent(at)?,
            name => child(at, name)?,
        };
    }
    Some(at)
}

/// Under `ServerScriptService` or `ServerStorage`, which never replicate.
fn server_only(dom: &WeakDom, r: Ref) -> bool {
    let mut at = dom.parent(r);
    while let Some(a) = at {
        if matches!(
            dom.get(a).map(|i| i.class()),
            Some("ServerScriptService" | "ServerStorage")
        ) {
            return true;
        }
        at = dom.parent(a);
    }
    false
}

fn successors<'a>(edges: &'a [Edge], from: Ref) -> impl Iterator<Item = Ref> + 'a {
    edges
        .iter()
        .filter(move |e| e.from == from)
        .filter_map(|e| match e.target {
            Target::Module(m) => Some(m),
            Target::Unknown(_) => None,
        })
}

/// Tarjan's strongly connected components, keeping only the groups that
/// hold a cycle: more than one member, or one requiring itself.
fn strongly_connected(members: &[Ref], edges: &[Edge]) -> Vec<Vec<Ref>> {
    struct State<'a> {
        edges: &'a [Edge],
        index: HashMap<Ref, usize>,
        low: HashMap<Ref, usize>,
        stack: Vec<Ref>,
        on_stack: HashSet<Ref>,
        groups: Vec<Vec<Ref>>,
    }
    fn visit(state: &mut State<'_>, at: Ref) {
        let next = state.index.len();
        state.index.insert(at, next);
        state.low.insert(at, next);
        state.stack.push(at);
        state.on_stack.insert(at);
        for to in successors(state.edges, at).collect::<Vec<_>>() {
            if !state.index.contains_key(&to) {
                visit(state, to);
                let low = state.low[&at].min(state.low[&to]);
                state.low.insert(at, low);
            } else if state.on_stack.contains(&to) {
                let low = state.low[&at].min(state.index[&to]);
                state.low.insert(at, low);
            }
        }
        if state.low[&at] == state.index[&at] {
            let mut group = Vec::new();
            while let Some(top) = state.stack.pop() {
                state.on_stack.remove(&top);
                group.push(top);
                if top == at {
                    break;
                }
            }
            let looped = group.len() > 1 || successors(state.edges, at).any(|to| to == at);
            if looped {
                group.reverse();
                state.groups.push(group);
            }
        }
    }
    let mut state = State {
        edges,
        index: HashMap::new(),
        low: HashMap::new(),
        stack: Vec::new(),
        on_stack: HashSet::new(),
        groups: Vec::new(),
    };
    for &member in members {
        if !state.index.contains_key(&member) {
            visit(&mut state, member);
        }
    }
    state.groups
}

/// The shortest cycle through the group's first member, found breadth
/// first inside the group.
fn cycle_path(group: &[Ref], edges: &[Edge]) -> Option<Vec<Ref>> {
    let start = *group.first()?;
    let inside: HashSet<Ref> = group.iter().copied().collect();
    let mut came_from: HashMap<Ref, Ref> = HashMap::new();
    let mut queue = VecDeque::from([start]);
    while let Some(at) = queue.pop_front() {
        for to in successors(edges, at) {
            if to == start {
                let mut path = vec![at];
                while let Some(&back) = came_from.get(&path[path.len() - 1]) {
                    path.push(back);
                }
                path.reverse();
                return Some(path);
            }
            if inside.contains(&to) && !came_from.contains_key(&to) {
                came_from.insert(to, at);
                queue.push_back(to);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
