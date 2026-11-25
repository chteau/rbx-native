//! Integration specs: a TestEZ / Jest-Lua spec named `*.integration.spec`
//! (or `.integration.test`) runs on a whole Local Run [`Session`] instead
//! of the command bar's sandbox: the place's server scripts, players who
//! join with clients of their own, physics, Humanoids, remotes over the
//! simulated link.
//!
//! The spec runs on the server, after the server's scripts first ran,
//! with `LocalRun` beside TestEZ's names (`integration.luau` documents
//! each call): `addPlayer`, `removePlayer`, `act` (keys, jump, walk to,
//! GUI clicks, chat, reset), `runFor`, `waitUntil`, `client(p):eval` and
//! `setNetwork`. Each file gets a fresh session over a copy of the place,
//! on the simulated clock, with every VM's randomness seeded from
//! [`Config::seed`] (the run is recorded, as a replay is): the same place
//! and spec run the same way every time. Nothing is rendered and nothing
//! goes online: DataStores are the run's own, in memory, and avatars are
//! the default character.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use mlua::{Function, Lua, MultiValue, Table, Value};
use rbx_dom::WeakDom;
use rbx_lua::server::{decode_all, encode_args, ServerVm, Services, Wire};
use rbx_lua::test_runner::{self, FileReport, Harness, Options, Report, SpecFile, Status};
use rbx_lua::{LineKind, Output};
use rbx_reflection::ReflectionDatabase;

use crate::link::{Network, Way};
use crate::replay::Action;
use crate::{Input, Peer, Session};

const LIBRARY: &str = include_str!("integration.luau");

/// Simulated seconds one file may run in all.
pub const TIME_LIMIT: f64 = 300.0;

/// UserIds for players `addPlayer` names itself count down from here
/// (`Player1` and the rest take -1, -2...).
const NAMED_USER_IDS: i64 = -1000;

#[derive(Clone, Debug, Default)]
pub struct Config {
    /// Seeds every VM's `math.random`, `Random.new()` and clock reads.
    pub seed: u64,
    /// The link every client has from the start.
    pub network: Network,
}

/// The Network Simulator's presets (`rbx_play::netsim::PRESETS`, as the
/// editor turns them into Local Run's link), by short name, and `none`.
pub const PROFILES: &[(&str, Network)] = &[
    ("none", net(way(0.0, 0.0, 0.0), way(0.0, 0.0, 0.0))),
    ("fiber", net(way(8.0, 0.0, 0.0), way(8.0, 0.0, 0.0))),
    ("broadband", net(way(25.0, 3.0, 0.0), way(25.0, 3.0, 0.0))),
    ("wifi", net(way(30.0, 12.0, 0.2), way(30.0, 15.0, 0.3))),
    ("4g", net(way(45.0, 20.0, 0.4), way(55.0, 30.0, 0.5))),
    ("3g", net(way(150.0, 70.0, 0.5), way(180.0, 90.0, 0.5))),
];

const fn way(latency_ms: f64, jitter_ms: f64, loss_percent: f64) -> Way {
    Way {
        latency: latency_ms / 1000.0,
        jitter: jitter_ms / 1000.0,
        loss: loss_percent / 100.0,
    }
}

const fn net(down: Way, up: Way) -> Network {
    Network { down, up }
}

/// A profile by name, any case.
pub fn profile(name: &str) -> Option<Network> {
    PROFILES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, network)| *network)
}

/// Runs every file, honouring `FOCUS` across them as the plain runner does.
pub fn run(
    dom: &WeakDom,
    files: &[SpecFile],
    database: &ReflectionDatabase,
    options: &Options,
    config: &Config,
) -> Report {
    let focus = files
        .iter()
        .any(|f| test_runner::plan(dom, f, database).focus);
    Report {
        files: files
            .iter()
            .map(|f| run_file(dom, f, options, focus, config))
            .collect(),
    }
}

/// What the spec asked for during a frame, carried out after it.
enum Request {
    Join(Option<String>),
    Leave(String),
    Act(String, Action),
    Network(Network),
    Eval {
        id: u64,
        player: String,
        source: String,
        args: Vec<Wire>,
    },
}

type Replies = Rc<RefCell<HashMap<u64, Result<Vec<Wire>, String>>>>;

#[derive(Default)]
struct State {
    requests: RefCell<Vec<Request>>,
    replies: Replies,
    next_eval: Cell<u64>,
    /// The harness returned: `Some(error)` if it raised.
    finished: RefCell<Option<Option<String>>>,
}

/// Runs one file on a session of its own; `focus` skips every test not
/// focused.
pub fn run_file(
    dom: &WeakDom,
    file: &SpecFile,
    options: &Options,
    focus: bool,
    config: &Config,
) -> FileReport {
    let mut session = match Session::new(dom.clone()) {
        Ok(session) => session,
        Err(error) => return broken(file, format!("the place does not run: {error}")),
    };
    session.record(config.seed);
    session.set_services(Services::default());
    session.set_network(config.network);
    session.start();

    let state = Rc::new(State::default());
    let printed = Rc::new(Cell::new(0));
    let harness = {
        let vm = session.server.vm();
        let pending = vm.pending_output();
        let seen = printed.clone();
        let count = move || seen.get() + pending.borrow().lines().len();
        start(vm, file, options, focus, count, state.clone())
    };
    let harness = match harness {
        Ok(harness) => harness,
        Err(error) => return broken(file, error.to_string()),
    };

    let mut output = Output::default();
    let mut names: Vec<String> = Vec::new();
    let unfinished = loop {
        session.step();
        collect(&mut session, &names, &mut output);
        printed.set(output.lines().len());
        if let Some(error) = state.finished.borrow_mut().take() {
            break error.unwrap_or_default();
        }
        if session.time() > TIME_LIMIT {
            break format!("still waiting after {TIME_LIMIT} s of simulated time");
        }
        names = session.clients.iter().map(|c| c.name.clone()).collect();
        let requests = state.requests.take();
        for request in requests {
            carry_out(&mut session, request, &state.replies, &mut output);
        }
    };
    harness.report(dom, file, output, unfinished)
}

/// Starts the harness over `file` on the server's VM, `LocalRun` in the
/// spec's environment.
fn start(
    vm: &ServerVm,
    file: &SpecFile,
    options: &Options,
    focus: bool,
    printed: impl Fn() -> usize + 'static,
    state: Rc<State>,
) -> mlua::Result<Harness> {
    let lua = vm.lua();
    let (local_run, main): (Table, Function) = lua
        .load(LIBRARY)
        .set_name("=LocalRun")
        .call(native(lua, state)?)?;
    let globals = lua.create_table()?;
    globals.set("LocalRun", local_run)?;
    let (harness, chunk, args) = Harness::new(lua, file, options, focus, printed, globals)?;
    let mut call = MultiValue::from_vec(vec![Value::Function(chunk)]);
    call.extend(args);
    vm.spawn(main, call)?;
    Ok(harness)
}

/// The Rust half of `LocalRun` (see `integration.luau`).
fn native(lua: &Lua, state: Rc<State>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let push = |state: &Rc<State>, request| state.requests.borrow_mut().push(request);

    let s = state.clone();
    table.set(
        "join",
        lua.create_function(move |_, name: Option<String>| {
            push(&s, Request::Join(name));
            Ok(())
        })?,
    )?;
    let s = state.clone();
    table.set(
        "leave",
        lua.create_function(move |_, name: String| {
            push(&s, Request::Leave(name));
            Ok(())
        })?,
    )?;
    let s = state.clone();
    table.set(
        "act",
        lua.create_function(move |_, (name, input): (String, Table)| {
            push(&s, Request::Act(name, action(&input)?));
            Ok(())
        })?,
    )?;
    let s = state.clone();
    table.set(
        "network",
        lua.create_function(move |_, profile: Value| {
            push(&s, Request::Network(network(&profile)?));
            Ok(())
        })?,
    )?;
    let s = state.clone();
    table.set(
        "eval",
        lua.create_function(
            move |lua, (player, source, args): (String, String, MultiValue)| {
                let id = s.next_eval.get();
                s.next_eval.set(id + 1);
                let args = encode_args(lua, args.into_iter())?;
                push(
                    &s,
                    Request::Eval {
                        id,
                        player,
                        source,
                        args,
                    },
                );
                Ok(id)
            },
        )?,
    )?;
    let replies = state.replies.clone();
    table.set(
        "poll",
        lua.create_function(move |lua, id: u64| {
            let mut out = MultiValue::new();
            match replies.borrow_mut().remove(&id) {
                None => {}
                Some(Ok(values)) => {
                    out = decode_all(lua, &values)?;
                    out.push_front(Value::Boolean(true));
                }
                Some(Err(message)) => {
                    out.push_back(Value::Boolean(false));
                    out.push_back(Value::String(lua.create_string(message)?));
                }
            }
            Ok(out)
        })?,
    )?;
    table.set(
        "finish",
        lua.create_function(move |_, (ok, error): (bool, Option<String>)| {
            *state.finished.borrow_mut() = Some((!ok).then(|| error.unwrap_or_default()));
            Ok(())
        })?,
    )?;
    Ok(table)
}

/// `LocalRun.act`'s primitive inputs: `{key, down}`, `{button, down}`,
/// `{x, y}` (the cursor), `{chat}`, `{reset}`.
fn action(input: &Table) -> mlua::Result<Action> {
    let down = || input.get::<Option<bool>>("down").map(|d| d.unwrap_or(true));
    if let Some(code) = input.get::<Option<String>>("key")? {
        return Ok(Action::Input(Input::Key {
            code,
            down: down()?,
        }));
    }
    if let Some(button) = input.get::<Option<u8>>("button")? {
        return Ok(Action::Input(Input::Button {
            button,
            down: down()?,
        }));
    }
    if let (Some(x), Some(y)) = (
        input.get::<Option<f32>>("x")?,
        input.get::<Option<f32>>("y")?,
    ) {
        return Ok(Action::Input(Input::Move {
            position: [x, y],
            delta: [0.0, 0.0],
        }));
    }
    if let Some(text) = input.get::<Option<String>>("chat")? {
        return Ok(Action::Chat(text));
    }
    if input.get::<Option<bool>>("reset")?.unwrap_or(false) {
        return Ok(Action::Reset);
    }
    Err(mlua::Error::runtime("LocalRun.act: unknown action"))
}

/// `LocalRun.setNetwork`'s argument: a profile's name, or `{latency,
/// jitter, loss}` for both ways, or `{down = {...}, up = {...}}`.
fn network(value: &Value) -> mlua::Result<Network> {
    let way = |t: &Table| -> mlua::Result<Way> {
        let get = |key| t.get::<Option<f64>>(key).map(Option::unwrap_or_default);
        Ok(Way {
            latency: get("latency")?,
            jitter: get("jitter")?,
            loss: get("loss")?.clamp(0.0, 1.0),
        })
    };
    match value {
        Value::String(name) => {
            let name = name.to_str()?.to_string();
            profile(&name).ok_or_else(|| {
                let names: Vec<_> = PROFILES.iter().map(|(n, _)| *n).collect();
                mlua::Error::runtime(format!(
                    "LocalRun.setNetwork: no profile named '{name}' (one of {})",
                    names.join(", ")
                ))
            })
        }
        Value::Table(t) => match (
            t.get::<Option<Table>>("down")?,
            t.get::<Option<Table>>("up")?,
        ) {
            (None, None) => Ok(net(way(t)?, way(t)?)),
            (down, up) => Ok(Network {
                down: down.as_ref().map(way).transpose()?.unwrap_or_default(),
                up: up.as_ref().map(way).transpose()?.unwrap_or_default(),
            }),
        },
        _ => Err(mlua::Error::runtime(
            "LocalRun.setNetwork expects a profile name or a table",
        )),
    }
}

fn carry_out(session: &mut Session, request: Request, replies: &Replies, output: &mut Output) {
    let index = |session: &Session, name: &str| session.clients.iter().position(|c| c.name == name);
    match request {
        Request::Join(name) => {
            let joined = match name {
                None => session.join_test_players(1, None).map(drop),
                Some(name) => {
                    let user_id = NAMED_USER_IDS - session.clients.len() as i64 - 1;
                    session.join(&name, user_id, None).map(drop)
                }
            };
            if let Err(error) = joined {
                output.push_text(LineKind::Error, format!("LocalRun.addPlayer: {error}"));
            }
        }
        Request::Leave(name) => {
            if let Some(i) = index(session, &name) {
                session.leave(i);
            }
        }
        Request::Act(name, action) => {
            if let Some(i) = index(session, &name) {
                session.act(Peer::Client(i), action);
            }
        }
        Request::Network(network) => session.set_network(network),
        Request::Eval {
            id,
            player,
            source,
            args,
        } => {
            let started = match index(session, &player) {
                Some(i) => eval(
                    session.clients[i].sim.vm(),
                    &player,
                    id,
                    &source,
                    &args,
                    replies,
                )
                .map_err(|e| e.to_string()),
                None => Err(format!("{player} is not in the game")),
            };
            if let Err(error) = started {
                replies.borrow_mut().insert(id, Err(error));
            }
        }
    }
}

/// Queues `source` on the client's VM, its reply to land in `replies`.
fn eval(
    vm: &ServerVm,
    player: &str,
    id: u64,
    source: &str,
    args: &[Wire],
    replies: &Replies,
) -> mlua::Result<()> {
    let lua = vm.lua();
    let env = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set("__index", lua.globals())?;
    env.set_metatable(Some(meta))?;
    let chunk = lua
        .load(source)
        .set_name(format!("=LocalRun.client({player}):eval"))
        .set_environment(env)
        .into_function()?;
    let replies = replies.clone();
    let done = lua.create_function(move |lua, (ok, rest): (bool, MultiValue)| {
        let reply = if ok {
            Ok(encode_args(lua, rest.into_iter())?)
        } else {
            let error = rest.into_iter().next().unwrap_or(Value::Nil);
            Err(lua
                .globals()
                .get::<Function>("tostring")?
                .call::<String>(error)?)
        };
        replies.borrow_mut().insert(id, reply);
        Ok(())
    })?;
    let wrapper: Function = lua
        .load("local f, done = ...\nreturn function(...) done(pcall(f, ...)) end")
        .set_name("=LocalRun")
        .call((chunk, done))?;
    vm.spawn(wrapper, decode_all(lua, args)?)
}

/// What every peer printed this frame into `output`, client lines marked
/// with the player's name; `before` are the clients' names as the frame
/// began, for those who left during it.
fn collect(session: &mut Session, before: &[String], output: &mut Output) {
    let taken = session.take_output();
    let current = session.clients.len();
    let left = taken.len().saturating_sub(current + 1);
    for (n, (peer, lines)) in taken.into_iter().enumerate() {
        let Peer::Client(i) = peer else {
            output.extend(lines);
            continue;
        };
        let name = if n > left {
            session.clients.get(i).map(|c| c.name.clone())
        } else {
            before.get(i).cloned()
        }
        .unwrap_or_else(|| format!("client {}", i + 1));
        for (line, kind) in lines.lines().iter().zip(lines.kinds()) {
            output.push_text(*kind, format!("[{name}] {line}"));
        }
    }
}

/// A file that could not start: one failure saying why.
fn broken(file: &SpecFile, message: String) -> FileReport {
    FileReport {
        file: file.clone(),
        results: vec![test_runner::TestResult {
            path: Vec::new(),
            status: Status::Failed,
            message: Some(message),
            stack: None,
            location: None,
            duration: Duration::ZERO,
            output: Output::default(),
        }],
        output: Output::default(),
        duration: Duration::ZERO,
    }
}

#[cfg(test)]
mod tests;
