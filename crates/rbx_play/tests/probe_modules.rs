use mlua::Lua;

const CODEC: &str = include_str!("../src/probe/codec.luau");
const LIVE: &str = include_str!("../src/probe/live.luau");
const DEBUG: &str = include_str!("../src/probe/debug.luau");

fn compiles(name: &str, source: &str) {
    let lua = Lua::new();
    if let Err(err) = lua.load(source).set_name(name).into_function() {
        panic!("{name} does not compile: {err}");
    }
}

const SERVER: &str = include_str!("../src/probe/server.luau");
const CLIENT: &str = include_str!("../src/probe/client.luau");
const PRINT: &str = include_str!("../src/probe/print.luau");

#[test]
fn server_compiles() {
    compiles("server.luau", SERVER);
}

#[test]
fn client_compiles() {
    compiles("client.luau", CLIENT);
}

#[test]
fn print_compiles() {
    compiles("print.luau", PRINT);
}

#[test]
fn codec_compiles() {
    compiles("codec.luau", CODEC);
}

#[test]
fn live_compiles() {
    compiles("live.luau", LIVE);
}

#[test]
fn debug_compiles() {
    compiles("debug.luau", DEBUG);
}

fn debug_module(lua: &Lua) -> mlua::Table {
    lua.load(
        r##"
        game = { GetService = function() return { IsServer = function() return true end } end }
        task = { wait = coroutine.yield }
        warn = print
        "##,
    )
    .exec()
    .expect("stubs");
    lua.load(DEBUG)
        .set_name("debug.luau")
        .eval()
        .expect("module")
}

#[test]
fn debug_hit_pauses_and_resumes() {
    let lua = Lua::new();
    let d = debug_module(&lua);
    lua.globals().set("D", d).unwrap();
    lua.load(
        r##"
        sent = {}
        D.attach("server", function(m) table.insert(sent, m) end)
        D.lines(7, {5}, {})
        co = coroutine.create(function()
            D.hit(7, 5, "a s", {1, "x"})
            return "done"
        end)
        coroutine.resume(co)
        assert(sent[1].type == "paused" and sent[1].line == 5)
        assert(sent[1].locals[1].value == "1" and sent[1].locals[2].value == '"x"')
        assert(coroutine.status(co) == "suspended")
        D.command({ type = "debug_resume", side = "server", pause = sent[1].pause, action = "continue" })
        local _, result = coroutine.resume(co)
        assert(result == "done" and coroutine.status(co) == "dead")
        "##,
    )
    .exec()
    .expect("pause and resume");
}

#[test]
fn debug_step_over_skips_deeper_hits() {
    let lua = Lua::new();
    let d = debug_module(&lua);
    lua.globals().set("D", d).unwrap();
    lua.load(
        r##"
        sent = {}
        D.attach("server", function(m) table.insert(sent, m) end)
        D.lines(1, {1}, {})
        co = coroutine.create(function()
            local function deeper() D.hit(1, 3, "", {}) end
            D.hit(1, 1, "", {})
            deeper()
            D.hit(1, 2, "", {})
            return "done"
        end)
        coroutine.resume(co)
        assert(#sent == 1 and sent[1].line == 1)
        D.command({ type = "debug_resume", side = "server", pause = sent[1].pause, action = "step_over" })
        assert(D.s == false)
        coroutine.resume(co)
        assert(#sent == 2 and sent[2].line == 2, "stopped at the next same-depth hit")
        D.command({ type = "debug_resume", side = "server", pause = sent[2].pause, action = "continue" })
        local _, result = coroutine.resume(co)
        assert(result == "done" and D.s == false)
        "##,
    )
    .exec()
    .expect("step over");
}

#[test]
fn sources_cover_the_protocol() {
    let both = format!("{CODEC}\n{LIVE}");
    for needle in [
        "GetComponents",
        "DescendantAdded",
        "DescendantRemoving",
        "__rbxnative_",
        "Tree",
        "Diff",
        "Props",
    ] {
        assert!(both.contains(needle), "probe modules lack {needle}");
    }
}
