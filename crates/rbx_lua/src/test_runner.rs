//! Runs a place's TestEZ-style specs headlessly, in the command bar's
//! sandbox: no Studio, no client.
//!
//! A spec is a `ModuleScript` named `*.spec` or `*.test` (TestEZ's
//! convention). Each one runs in a VM of its own over a fresh copy of the
//! place, so no test sees what another file's tests did. The harness
//! (`harness.luau`) gives TestEZ's `describe`/`it`/`expect` with
//! `.to.equal`/`.to.be.ok`/`.to.throw`/`.never`, the four hooks and
//! `SKIP`/`FOCUS`, plus Jest-Lua's matcher names (`expect(x).toBe(y)`)
//! and `describe.skip`/`it.only`. A Jest-Lua spec that `require`s
//! `JestGlobals` from its packages still needs those packages in the place.
//!
//! Time is simulated: `task.wait(5)` returns at once with the clock moved
//! on by five seconds, so timing-based tests run fast and deterministically.
//! A file whose threads still wait after ten simulated minutes, or whose
//! test waits on something nothing will ever fire (a signal no code
//! fires, a client's remote), fails that test with a message saying so.
//!
//! A spec named `*.integration.spec` (or `.integration.test`) is an
//! integration spec: [`discover`] leaves it out, [`discover_integration`]
//! finds it, and Local Run runs it on a whole session (`rbx_sim::integration`)
//! through [`Harness`].

mod report;
#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{IntoLua, Lua, MultiValue, Table, Value, VmState};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::engine::engine;
use crate::instance::tree::full_name;
use crate::instance::LuaInstance;
use crate::output::Output;
use crate::runtime::{compile_in, RunState, Runtime};

pub use report::{human, json, junit};

const HARNESS: &str = include_str!("test_runner/harness.luau");
const HARNESS_NAME: &str = "TestRunner";

/// How much simulated time one file's threads may wait in all.
const TIME_LIMIT: Duration = Duration::from_secs(600);

/// How long one test (with its hooks) may run before it fails, as the
/// command bar's budget is per resume and a file's tests share one.
const TEST_BUDGET: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 });

/// Wally vendors each dependency, specs and all, under `Packages._Index`:
/// those are the packages' tests, not the place's.
const VENDORED: &str = "_Index";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecFile {
    pub module: Ref,
    /// `GetFullName()`, which is also the chunk name its stack frames carry.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Passed,
    Failed,
    Skipped,
}

/// A script and line: where a failure was raised, or where a test is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub script: Ref,
    pub name: String,
    /// 1-based.
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct TestResult {
    /// Describe names down to the test's own; empty for an error loading
    /// the file itself; ends in `afterAll` for a failed `afterAll` hook.
    pub path: Vec<String>,
    pub status: Status,
    pub message: Option<String>,
    /// Luau's traceback, harness frames dropped.
    pub stack: Option<String>,
    /// Where the failure was raised (the first frame in one of the place's
    /// scripts), or else where the test was declared.
    pub location: Option<Location>,
    pub duration: Duration,
    /// What the test and its hooks printed.
    pub output: Output,
}

#[derive(Debug, Clone)]
pub struct FileReport {
    pub file: SpecFile,
    pub results: Vec<TestResult>,
    /// What the file's tests printed.
    pub output: Output,
    pub duration: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub files: Vec<FileReport>,
}

impl Report {
    pub fn count(&self, status: Status) -> usize {
        self.results().filter(|r| r.status == status).count()
    }

    pub fn passed(&self) -> bool {
        self.count(Status::Failed) == 0
    }

    pub fn results(&self) -> impl Iterator<Item = &TestResult> {
        self.files.iter().flat_map(|f| &f.results)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Describe,
    Test,
}

/// A `describe` or `it` as declared, before anything runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanNode {
    pub kind: NodeKind,
    pub path: Vec<String>,
    pub line: Option<u32>,
    pub skip: bool,
    pub focus: bool,
}

#[derive(Debug, Clone)]
pub struct FilePlan {
    pub file: SpecFile,
    /// Pre-order: a describe comes before what it holds.
    pub nodes: Vec<PlanNode>,
    /// The file failed to load, or a describe body raised.
    pub error: Option<String>,
    pub focus: bool,
}

/// Which tests a run runs. Both narrow; neither set runs everything.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// A substring of the test's describe names and its own, space-joined.
    pub filter: Option<String>,
    /// A path prefix: a test, or every test in a describe.
    pub only: Option<Vec<String>>,
}

/// Every spec module in the place but the integration specs, in tree
/// order, skipping Wally's vendored packages. `roots` (dotted full names,
/// `ReplicatedStorage.Tests`) limits the search to those subtrees; a root
/// that names nothing is an error.
pub fn discover(dom: &WeakDom, roots: &[String]) -> Result<Vec<SpecFile>, String> {
    discover_where(dom, roots, false)
}

/// The integration specs, as [`discover`] finds the others.
pub fn discover_integration(dom: &WeakDom, roots: &[String]) -> Result<Vec<SpecFile>, String> {
    discover_where(dom, roots, true)
}

/// A module named as an integration spec: `Coins.integration.spec`.
pub fn is_integration(name: &str) -> bool {
    [".integration.spec", ".integration.test"]
        .iter()
        .any(|end| name.ends_with(end))
}

fn discover_where(
    dom: &WeakDom,
    roots: &[String],
    integration: bool,
) -> Result<Vec<SpecFile>, String> {
    let starts = if roots.is_empty() {
        dom.root_refs().to_vec()
    } else {
        roots
            .iter()
            .map(|root| find(dom, root).ok_or_else(|| format!("no instance named '{root}'")))
            .collect::<Result<_, _>>()?
    };
    let mut found = Vec::new();
    let mut stack: Vec<Ref> = starts.into_iter().rev().collect();
    while let Some(at) = stack.pop() {
        let Some(instance) = dom.get(at) else {
            continue;
        };
        if instance.name() == VENDORED {
            continue;
        }
        let name = instance.name();
        if instance.class() == "ModuleScript"
            && (name.ends_with(".spec") || name.ends_with(".test"))
            && is_integration(name) == integration
        {
            found.push(SpecFile {
                module: at,
                name: full_name(dom, at).unwrap_or_else(|| name.to_string()),
            });
        }
        stack.extend(instance.children().iter().rev());
    }
    Ok(found)
}

fn find(dom: &WeakDom, path: &str) -> Option<Ref> {
    let mut names = path.split('.');
    let first = names.next()?;
    let mut at = *dom
        .root_refs()
        .iter()
        .find(|r| dom.get(**r).is_some_and(|i| i.name() == first))?;
    for name in names {
        at = *dom
            .get(at)?
            .children()
            .iter()
            .find(|r| dom.get(**r).is_some_and(|i| i.name() == name))?;
    }
    Some(at)
}

/// What a file declares, from running its `describe` bodies only.
pub fn plan(dom: &WeakDom, file: &SpecFile, database: &ReflectionDatabase) -> FilePlan {
    let ran = drive(dom, file, database, |lua| {
        let options = lua.create_table()?;
        options.set("plan", true)?;
        Ok(options)
    });
    let mut plan = FilePlan {
        file: file.clone(),
        nodes: Vec::new(),
        error: ran.harness_error,
        focus: false,
    };
    for (event, _) in ran.events {
        match event {
            Event::Node(node) => plan.nodes.push(node),
            Event::Error { message, .. } => plan.error = Some(message),
            Event::Done { focus } => plan.focus = focus,
            Event::Start(_) | Event::Result(_) => {}
        }
    }
    plan
}

/// Runs every file, honouring `FOCUS` across all of them as TestEZ does:
/// once any test anywhere is focused, only focused tests run.
pub fn run(
    dom: &WeakDom,
    files: &[SpecFile],
    database: &ReflectionDatabase,
    options: &Options,
) -> Report {
    let focus = files.iter().any(|f| plan(dom, f, database).focus);
    Report {
        files: files
            .iter()
            .map(|f| run_file(dom, f, database, options, focus))
            .collect(),
    }
}

/// Runs one file's selected tests; `focus` skips every test not focused.
pub fn run_file(
    dom: &WeakDom,
    file: &SpecFile,
    database: &ReflectionDatabase,
    options: &Options,
    focus: bool,
) -> FileReport {
    let started = Instant::now();
    let ran = drive(dom, file, database, |lua| run_options(lua, options, focus));
    let unfinished = match (&ran.harness_error, ran.timed_out) {
        (Some(error), _) => error.clone(),
        (None, true) => format!(
            "still waiting after {} s of simulated time",
            TIME_LIMIT.as_secs()
        ),
        (None, false) => "never finished: it waits on something the sandbox never fires \
             (a signal no code fires, a client's remote); later tests in the file did not run"
            .to_string(),
    };
    assemble(dom, file, ran.events, ran.output, unfinished, started)
}

fn run_options(lua: &Lua, options: &Options, focus: bool) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("focus", focus)?;
    table.set("filter", options.filter.clone())?;
    table.set("only", options.only.clone())?;
    Ok(table)
}

/// A file's report from what its harness reported; `unfinished` fails the
/// test it stopped in when it never got to the end.
fn assemble(
    dom: &WeakDom,
    file: &SpecFile,
    events: Vec<(Event, usize)>,
    output: Output,
    unfinished: String,
    started: Instant,
) -> FileReport {
    let scripts = scripts(dom);
    let locate = |stack: Option<&str>, line: Option<u32>| {
        stack
            .and_then(|stack| raised_at(stack, &scripts))
            .or_else(|| {
                line.map(|line| Location {
                    script: file.module,
                    name: file.name.clone(),
                    line,
                })
            })
    };
    let mut results = Vec::new();
    let mut running: Option<(Vec<String>, usize)> = None;
    let mut done = false;
    for (event, at) in events {
        match event {
            Event::Start(path) => running = Some((path, at)),
            Event::Result(raw) => {
                let mut slice = Output::default();
                if running.as_ref().is_some_and(|(path, _)| *path == raw.path) {
                    if let Some((_, from)) = running.take() {
                        slice = output.slice(from..at);
                    }
                }
                results.push(TestResult {
                    output: slice,
                    location: locate(raw.stack.as_deref(), raw.line),
                    stack: raw.stack.as_deref().map(|s| user_frames(s, &scripts)),
                    path: raw.path,
                    status: raw.status,
                    message: raw.message,
                    duration: raw.duration,
                });
            }
            Event::Done { .. } => done = true,
            Event::Node(_) | Event::Error { .. } => {}
        }
    }
    if !done {
        let (path, from) = running.unwrap_or_default();
        results.push(TestResult {
            path,
            status: Status::Failed,
            message: Some(unfinished),
            stack: None,
            location: None,
            duration: Duration::ZERO,
            output: output.slice(from..usize::MAX),
        });
    }
    FileReport {
        file: file.clone(),
        results,
        output,
        duration: started.elapsed(),
    }
}

/// The harness on a VM its caller drives: Local Run runs integration specs
/// on a session's server this way.
pub struct Harness {
    events: Rc<RefCell<Vec<(Event, usize)>>>,
    started: Instant,
}

impl Harness {
    /// The harness for `file` and its arguments, to call on a thread of
    /// `lua`'s: `printed` says how many lines the run has printed so far,
    /// and `globals` join the spec's environment. A test has no budget of
    /// its own here; the VM's script timeout applies.
    pub fn new(
        lua: &Lua,
        file: &SpecFile,
        options: &Options,
        focus: bool,
        printed: impl Fn() -> usize + 'static,
        globals: Table,
    ) -> mlua::Result<(Harness, mlua::Function, MultiValue)> {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut args = harness_args(lua, file, events.clone(), Rc::new(printed), |lua| {
            run_options(lua, options, focus)
        })?;
        args.push_back(Value::Function(lua.create_function(|_, ()| Ok(()))?));
        args.push_back(Value::Table(globals));
        let chunk = lua
            .load(HARNESS)
            .set_name(format!("={HARNESS_NAME}"))
            .into_function()?;
        let harness = Harness {
            events,
            started: Instant::now(),
        };
        Ok((harness, chunk, args))
    }

    /// The file's report, from what the harness reported and what the run
    /// printed; `unfinished` fails the test it stopped in, if it stopped.
    pub fn report(
        self,
        dom: &WeakDom,
        file: &SpecFile,
        output: Output,
        unfinished: String,
    ) -> FileReport {
        let events = Rc::try_unwrap(self.events)
            .map(RefCell::into_inner)
            .unwrap_or_else(|shared| shared.take());
        assemble(dom, file, events, output, unfinished, self.started)
    }
}

struct RawResult {
    path: Vec<String>,
    status: Status,
    message: Option<String>,
    stack: Option<String>,
    line: Option<u32>,
    duration: Duration,
}

enum Event {
    Node(PlanNode),
    Start(Vec<String>),
    Result(RawResult),
    Error { message: String },
    Done { focus: bool },
}

struct Ran {
    /// Each with how many lines had been printed when it was reported.
    events: Vec<(Event, usize)>,
    output: Output,
    /// The harness itself could not run (a VM error, not a test's).
    harness_error: Option<String>,
    timed_out: bool,
}

/// Runs the harness over `file` in a fresh VM and copy of `dom`, on a
/// simulated clock, collecting what it reports.
fn drive(
    dom: &WeakDom,
    file: &SpecFile,
    database: &ReflectionDatabase,
    options: impl FnOnce(&Lua) -> mlua::Result<Table>,
) -> Ran {
    let mut ran = Ran {
        events: Vec::new(),
        output: Output::default(),
        harness_error: None,
        timed_out: false,
    };
    let mut runtime = match Runtime::new(dom.clone(), database.clone()) {
        Ok(runtime) => runtime,
        Err(error) => {
            ran.harness_error = Some(error.to_string());
            return ran;
        }
    };
    let clock = Rc::new(Cell::new(Duration::ZERO));
    let read = clock.clone();
    runtime.set_clock(move || read.get());

    let events = Rc::new(RefCell::new(Vec::new()));
    let printed = Rc::new(Cell::new(0));
    let (pending, seen) = (runtime.pending_output(), printed.clone());
    let counter = Rc::new(move || seen.get() + pending.borrow().lines().len());
    let args =
        harness_args(runtime.lua(), file, events.clone(), counter, options).and_then(|mut args| {
            let rearm = runtime.lua().create_function(|lua, ()| {
                arm(lua);
                Ok(())
            })?;
            args.push_back(Value::Function(rearm));
            Ok(args)
        });
    match args {
        Ok(args) => {
            let chunk = runtime
                .lua()
                .load(HARNESS)
                .set_name(format!("={HARNESS_NAME}"))
                .into_function();
            runtime.start_call(chunk, args);
        }
        Err(error) => {
            ran.harness_error = Some(error.to_string());
            return ran;
        }
    }
    loop {
        let step = runtime.resume();
        ran.output.extend(step.output);
        printed.set(ran.output.lines().len());
        match step.state {
            RunState::Finished => break,
            RunState::Failed(error) => {
                ran.harness_error = Some(error.to_string());
                break;
            }
            RunState::Pending { wake_in } => {
                if clock.get() + wake_in > TIME_LIMIT {
                    runtime.cancel();
                    ran.timed_out = true;
                    break;
                }
                clock.set(clock.get() + wake_in);
            }
        }
    }
    drop(runtime);
    ran.events = Rc::try_unwrap(events)
        .map(RefCell::into_inner)
        .unwrap_or_else(|shared| shared.take());
    ran
}

fn harness_args(
    lua: &Lua,
    file: &SpecFile,
    events: Rc<RefCell<Vec<(Event, usize)>>>,
    printed: Rc<dyn Fn() -> usize>,
    options: impl FnOnce(&Lua) -> mlua::Result<Table>,
) -> mlua::Result<MultiValue> {
    let spec = LuaInstance::new(file.module, engine(lua).ctx.clone()).into_lua(lua)?;
    let load = lua.create_function(|lua, (module, env): (Value, Table)| {
        let meta = lua.create_table()?;
        meta.set("__index", lua.globals())?;
        env.set_metatable(Some(meta))?;
        compile_in(lua, &module, env)
    })?;
    let report = lua.create_function(move |_, (kind, data): (String, Table)| {
        events.borrow_mut().push((event(&kind, &data)?, printed()));
        Ok(())
    })?;
    Ok(MultiValue::from_vec(vec![
        spec,
        file.name.clone().into_lua(lua)?,
        Value::Function(load),
        Value::Function(report),
        Value::Table(options(lua)?),
    ]))
}

/// Gives the running test [`TEST_BUDGET`], in place of what is left of
/// the resume's. A timeout re-arms it, so the harness that catches the
/// error has a budget of its own to report it and go on with.
fn arm(lua: &Lua) {
    let deadline = Cell::new(Instant::now() + TEST_BUDGET);
    lua.set_interrupt(move |_| {
        if Instant::now() < deadline.get() {
            return Ok(VmState::Continue);
        }
        deadline.set(Instant::now() + TEST_BUDGET);
        Err(mlua::Error::runtime(format!(
            "Script timeout: the test ran for over {} s",
            TEST_BUDGET.as_secs()
        )))
    });
}

fn event(kind: &str, data: &Table) -> mlua::Result<Event> {
    let path = || {
        data.get::<Option<Vec<String>>>("path")
            .map(Option::unwrap_or_default)
    };
    Ok(match kind {
        "node" => {
            let mode: Option<String> = data.get("mode")?;
            Event::Node(PlanNode {
                kind: match data.get::<String>("kind")?.as_str() {
                    "describe" => NodeKind::Describe,
                    _ => NodeKind::Test,
                },
                path: path()?,
                line: data.get("line")?,
                skip: mode.as_deref() == Some("skip"),
                focus: mode.as_deref() == Some("focus"),
            })
        }
        "start" => Event::Start(path()?),
        "result" => Event::Result(RawResult {
            path: path()?,
            status: match data.get::<String>("status")?.as_str() {
                "passed" => Status::Passed,
                "skipped" => Status::Skipped,
                _ => Status::Failed,
            },
            message: data.get("message")?,
            stack: data.get("stack")?,
            line: data.get("line")?,
            duration: Duration::from_secs_f64(data.get::<f64>("duration")?.max(0.0)),
        }),
        "error" => Event::Error {
            message: data.get("message")?,
        },
        _ => Event::Done {
            focus: data.get::<Option<bool>>("focus")?.unwrap_or(false),
        },
    })
}

/// Every script's full name, which is its chunk name in a traceback.
fn scripts(dom: &WeakDom) -> HashMap<String, Ref> {
    let mut found = HashMap::new();
    let mut stack = dom.root_refs().to_vec();
    while let Some(at) = stack.pop() {
        let Some(instance) = dom.get(at) else {
            continue;
        };
        if matches!(instance.class(), "ModuleScript" | "Script" | "LocalScript") {
            if let Some(name) = full_name(dom, at) {
                found.entry(name).or_insert(at);
            }
        }
        stack.extend_from_slice(instance.children());
    }
    found
}

/// A traceback line's chunk name and line: `ReplicatedStorage.A.spec:12`,
/// optionally followed by ` function name`.
fn frame(line: &str) -> Option<(&str, u32)> {
    let head = line.split(" function ").next()?.trim();
    let (name, number) = head.rsplit_once(':')?;
    Some((name, number.parse().ok()?))
}

fn raised_at(stack: &str, scripts: &HashMap<String, Ref>) -> Option<Location> {
    stack.lines().find_map(|line| {
        let (name, number) = frame(line)?;
        Some(Location {
            script: *scripts.get(name)?,
            name: name.to_string(),
            line: number,
        })
    })
}

/// The traceback's frames in the place's own scripts.
fn user_frames(stack: &str, scripts: &HashMap<String, Ref>) -> String {
    stack
        .lines()
        .filter(|line| frame(line).is_some_and(|(name, _)| scripts.contains_key(name)))
        .collect::<Vec<_>>()
        .join("\n")
}
