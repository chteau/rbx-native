//! The sampling ScriptProfiler: Luau's interrupt reads the running
//! coroutine's stack every period and adds the time since the last sample
//! to its callgraph node.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mlua::{ffi, Lua};

use super::{of, Profiler};

struct Node {
    total: Duration,
    /// `(function, node)` pairs, one per callee seen under this node.
    children: Vec<(usize, usize)>,
}

struct Function {
    name: String,
    source: String,
    line: i32,
    total: Duration,
}

pub(super) struct Sampler {
    period: Duration,
    started_ms: u128,
    pub(super) last: Instant,
    nodes: Vec<Node>,
    functions: Vec<Function>,
    ids: HashMap<(String, String, i32), usize>,
    categories: Vec<(String, usize)>,
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

impl Sampler {
    pub(super) fn new(frequency: u32) -> Self {
        Sampler {
            period: Duration::from_secs_f64(1.0 / f64::from(frequency.clamp(1, 100_000))),
            started_ms: epoch_ms(),
            last: Instant::now(),
            nodes: Vec::new(),
            functions: Vec::new(),
            ids: HashMap::new(),
            categories: Vec::new(),
        }
    }

    fn node(&mut self) -> usize {
        self.nodes.push(Node {
            total: Duration::ZERO,
            children: Vec::new(),
        });
        self.nodes.len() - 1
    }

    /// Files `spent` under `category` along `stack` (outermost first).
    fn record(&mut self, category: &str, stack: &[(String, String, i32)], spent: Duration) {
        let root = match self.categories.iter().find(|(name, _)| name == category) {
            Some(&(_, node)) => node,
            None => {
                let node = self.node();
                self.categories.push((category.to_owned(), node));
                node
            }
        };
        self.nodes[root].total += spent;
        let mut at = root;
        let mut seen = Vec::with_capacity(stack.len());
        for frame in stack {
            let function = match self.ids.get(frame) {
                Some(&id) => id,
                None => {
                    self.functions.push(Function {
                        name: frame.0.clone(),
                        source: frame.1.clone(),
                        line: frame.2,
                        total: Duration::ZERO,
                    });
                    let id = self.functions.len() - 1;
                    self.ids.insert(frame.clone(), id);
                    id
                }
            };
            if !seen.contains(&function) {
                seen.push(function);
                self.functions[function].total += spent;
            }
            let child = self.nodes[at]
                .children
                .iter()
                .find(|(f, _)| *f == function)
                .map(|&(_, n)| n);
            at = match child {
                Some(node) => node,
                None => {
                    let node = self.node();
                    self.nodes[at].children.push((function, node));
                    node
                }
            };
            self.nodes[at].total += spent;
        }
    }

    /// The Developer Console's export: ids are 1-based, durations in
    /// microseconds, times in milliseconds since the epoch.
    pub(super) fn json(&self) -> String {
        use serde_json::{json, Map, Value as Json};
        let us = |d: Duration| d.as_micros() as u64;
        let categories: Vec<Json> = self
            .categories
            .iter()
            .map(|(name, node)| json!({ "Name": name, "NodeId": node + 1 }))
            .collect();
        let nodes: Vec<Json> = self
            .nodes
            .iter()
            .map(|node| {
                json!({
                    "TotalDuration": us(node.total),
                    "FunctionIds": node.children.iter().map(|c| c.0 + 1).collect::<Vec<_>>(),
                    "NodeIds": node.children.iter().map(|c| c.1 + 1).collect::<Vec<_>>(),
                })
            })
            .collect();
        let functions: Vec<Json> = self
            .functions
            .iter()
            .map(|f| {
                let mut out = Map::new();
                if !f.name.is_empty() {
                    out.insert("Name".into(), json!(f.name));
                }
                if !f.source.is_empty() {
                    out.insert("Source".into(), json!(f.source));
                }
                if f.line > 0 {
                    out.insert("Line".into(), json!(f.line));
                }
                out.insert("TotalDuration".into(), json!(us(f.total)));
                out.insert("Flags".into(), json!(0));
                Json::Object(out)
            })
            .collect();
        json!({
            "Version": 2,
            "SessionStartTime": self.started_ms as u64,
            "SessionEndTime": epoch_ms() as u64,
            "Categories": categories,
            "Nodes": nodes,
            "Functions": functions,
        })
        .to_string()
    }
}

/// A sampling VM's profiler and the interrupt the sampler wraps.
type Sampling = (Rc<RefCell<Profiler>>, ffi::lua_InterruptFunction);

thread_local! {
    /// Each sampling VM by its main thread.
    static SAMPLING: RefCell<HashMap<usize, Sampling>> =
        RefCell::new(HashMap::new());
}

/// After `Lua::set_interrupt`: wraps the interrupt with the sampler while
/// a session runs, and marks the resumption's start.
pub(crate) fn arm(lua: &Lua) {
    let profiler = of(lua);
    profiler.borrow_mut().resumed();
    let sampling = profiler.borrow().profiling();
    // SAFETY: only the shared callbacks table is read and written.
    let _ = unsafe {
        lua.exec_raw::<()>((), |state| {
            let main = ffi::lua_mainthread(state) as usize;
            let callbacks = ffi::lua_callbacks(state);
            match ((*callbacks).interrupt, sampling) {
                (Some(previous), true)
                    if !std::ptr::eq(previous as *const (), sample_proc as *const ()) =>
                {
                    (*callbacks).interrupt = Some(sample_proc);
                    SAMPLING.with(|s| s.borrow_mut().insert(main, (profiler.clone(), previous)));
                }
                (_, false) => {
                    SAMPLING.with(|s| s.borrow_mut().remove(&main));
                }
                _ => {}
            }
        })
    };
}

fn text(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: Luau hands out NUL-terminated strings that live as long as
    // the function they describe.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

unsafe extern "C-unwind" fn sample_proc(state: *mut ffi::lua_State, gc: std::ffi::c_int) {
    let main = ffi::lua_mainthread(state) as usize;
    let Some((profiler, previous)) = SAMPLING.with(|s| s.borrow().get(&main).cloned()) else {
        return;
    };
    if gc < 0 {
        sample(state, &profiler);
    }
    previous(state, gc);
}

unsafe fn sample(state: *mut ffi::lua_State, profiler: &Rc<RefCell<Profiler>>) {
    let Ok(mut profiler) = profiler.try_borrow_mut() else {
        return;
    };
    let category = profiler
        .category
        .clone()
        .unwrap_or_else(|| "Script".to_owned());
    let Some(sampler) = profiler.sampler.as_mut() else {
        return;
    };
    let now = Instant::now();
    let spent = now - sampler.last;
    if spent < sampler.period {
        return;
    }
    sampler.last = now;
    let mut stack = Vec::new();
    let mut level = 0;
    loop {
        let mut ar = std::mem::zeroed::<ffi::lua_Debug>();
        if ffi::lua_getinfo(state, level, c"sln".as_ptr(), &mut ar) == 0 {
            break;
        }
        let source = text(ar.source);
        let source = source.strip_prefix('=').unwrap_or(&source);
        // Only script frames, as in Roblox's: not C functions, nor the
        // engine's own Luau.
        if text(ar.what) != "C" && !crate::engine::scheduler::internal_chunk(source) {
            stack.push((text(ar.name), source.to_owned(), ar.linedefined));
        }
        level += 1;
    }
    stack.reverse();
    // A sample stands for the time since the last one, at most a few
    // periods (the rest went to code outside Luau).
    sampler.record(&category, &stack, spent.min(sampler.period * 4));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_export_nests_categories_nodes_and_functions() {
        let mut sampler = Sampler::new(1000);
        let frame = |name: &str, line| (name.to_owned(), "Workspace.Script".to_owned(), line);
        let ms = Duration::from_millis(1);
        sampler.record("Heartbeat", &[frame("outer", 1), frame("inner", 5)], ms);
        sampler.record("Heartbeat", &[frame("outer", 1)], ms);
        sampler.record("Stepped", &[frame("inner", 5)], ms);
        let json: serde_json::Value = serde_json::from_str(&sampler.json()).unwrap();
        assert_eq!(json["Version"], 2);
        assert_eq!(json["Categories"][0]["Name"], "Heartbeat");
        let root = json["Categories"][0]["NodeId"].as_u64().unwrap() as usize;
        let node = &json["Nodes"][root - 1];
        assert_eq!(node["TotalDuration"], 2000);
        assert_eq!(node["FunctionIds"], serde_json::json!([1]));
        let outer = node["NodeIds"][0].as_u64().unwrap() as usize;
        assert_eq!(json["Nodes"][outer - 1]["TotalDuration"], 2000);
        assert_eq!(json["Functions"][0]["Name"], "outer");
        assert_eq!(json["Functions"][0]["TotalDuration"], 2000);
        assert_eq!(json["Functions"][1]["TotalDuration"], 2000);
        assert_eq!(json["Functions"][1]["Line"], 5);
    }
}
