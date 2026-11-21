//! Each new Luau feature as Studio 0.748 behaves, recorded in its Command
//! Bar on 2026-11-21 (Vinegar, edit mode): the expected lines are what it
//! printed for the same code.

use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::Runtime;

fn run(source: &str) -> Vec<String> {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    let mut runtime = Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
    match runtime.run(source) {
        Ok(output) => output.lines().to_vec(),
        Err(error) => panic!("{error} after {:?}", error.output.lines()),
    }
}

fn release(version: &str) -> u32 {
    version
        .strip_prefix("0.")
        .and_then(|minor| minor.parse().ok())
        .unwrap_or_else(|| panic!("not a Luau release: {version}"))
}

/// The guard: Roblox's Luau moves every week, and new syntax or library
/// functions must work here the day Roblox turns them on.
#[test]
fn the_linked_luau_is_not_behind_roblox() {
    let linked = linked_version();
    assert!(
        release(linked) >= release(LUAU_VERSION),
        "this build links Luau {linked} but live Roblox runs {LUAU_VERSION}: \
         bump vendor/luau0-src (see vendor/README.md)"
    );
}

/// The VM and compiler flags behind what scripts can see must exist (a
/// bump where upstream renamed or retired one fails here, and the list in
/// `luau.rs` needs the same change).
#[test]
fn the_roblox_flags_behind_language_features_exist() {
    let _ = new_vm();
    for flag in ROBLOX_FFLAGS
        .iter()
        .filter(|flag| !flag.contains("Codegen") && !flag.contains("Inliner"))
    {
        assert!(mlua::Lua::set_fflag(flag, true).is_ok(), "{flag}");
    }
}

#[test]
fn library_matches_roblox() {
    assert_eq!(
        run(
            "print(_VERSION, typeof(integer), typeof(buffer.readvectorf32x3), \
             typeof(coroutine.finally), typeof(coroutine.close))"
        ),
        ["Luau nil nil function function"]
    );
    assert_eq!(
        run("local f, e = loadstring('return 1i') print(f, e)"),
        [r#"nil [string "return 1i"]:1: Malformed number"#]
    );
    assert_eq!(run("const x = 1 print(x)"), ["1"]);
}

/// Luau 0.748's fix: an inlined function could clobber a captured value.
#[test]
fn inlined_calls_keep_captured_values() {
    assert_eq!(
        run("local function test() local f f = function() f = nil end \
             f = (function() return f() end)() return f end print(test())"),
        ["nil"]
    );
}

#[test]
fn coroutine_finally_under_coroutine_resume() {
    let source = r##"
        local co = coroutine.create(function(a) local b = coroutine.yield(a + 1) return b * 2 end)
        coroutine.finally(co, function(...) print("F1", select("#", ...), ...) return "ignored" end)
        print("R1", coroutine.resume(co, 1))
        print("R2", coroutine.resume(co, 5))
        print("ST", coroutine.status(co))
        local ce = coroutine.create(function() error("boom", 0) end)
        coroutine.finally(ce, function(...) print("F2", select("#", ...), ...) end)
        print("R3", coroutine.resume(ce))
        local cc = coroutine.create(function() coroutine.yield() end)
        coroutine.resume(cc)
        coroutine.finally(cc, function(...) print("F3", select("#", ...), ...) end)
        print("C", coroutine.close(cc))
        local two = coroutine.create(function() return "v" end)
        coroutine.finally(two, function(...) print("F4a", ...) end)
        coroutine.finally(two, function(...) print("F4b", ...) end)
        print("R4", coroutine.resume(two))
        print("FE", pcall(coroutine.finally, coroutine.create(print)))
        print("FE2", pcall(coroutine.finally, 1, print))
        local fe = coroutine.create(function() return 1 end)
        coroutine.finally(fe, function() error("finboom", 0) end)
        print("R5", coroutine.resume(fe))
        local yf = coroutine.create(function() return 1 end)
        coroutine.finally(yf, function() print("F6 yieldable", coroutine.isyieldable()) end)
        print("R6", coroutine.resume(yf))
        local w = coroutine.wrap(function() coroutine.finally(coroutine.running(), print) return "w" end)
        print("G6", pcall(w))
        local d = coroutine.create(function() end)
        coroutine.resume(d)
        print("G8", pcall(coroutine.finally, d, print))
        local c11 = coroutine.create(function() coroutine.yield() end)
        coroutine.resume(c11)
        coroutine.finally(c11, function() error("cfin", 0) end)
        print("G11", coroutine.close(c11))
    "##;
    assert_eq!(
        run(source),
        [
            "R1 true 2",
            "F1 2 finished 10",
            "R2 true 10",
            "ST dead",
            "F2 2 error boom",
            "R3 false boom",
            "F3 1 cancelled",
            "C true",
            "F4b finished v",
            "F4a finished v",
            "R4 true v",
            "FE false missing argument #2",
            "FE2 false invalid argument #1 to 'finally' (thread expected, got number)",
            "R5 false finboom",
            "F6 yieldable true",
            "R6 true 1",
            "G6 false cannot run finalizers from a wrapped coroutine",
            "G8 false cannot register a finalizer on dead coroutine",
            "G11 false cfin",
        ]
    );
}

/// Threads the task scheduler runs call their finalizers too: when they
/// end, fail (whose error Studio then reports twice) or are cancelled.
#[test]
fn coroutine_finally_on_scheduler_threads() {
    let source = r##"
        local sp = task.spawn(function() task.wait() end)
        coroutine.finally(sp, function(...) print("FSPAWN", select("#", ...), ...) end)
        local df = task.defer(function() return 7 end)
        coroutine.finally(df, function(...) print("FDEFER", select("#", ...), ...) end)
        local t1 = task.spawn(function() task.wait() error("sperr", 0) end)
        coroutine.finally(t1, function(...) print("G1", select("#", ...), ...) end)
        local t2 = task.delay(1, function() end)
        coroutine.finally(t2, function(...) print("G2", select("#", ...), ...) end)
        task.cancel(t2)
        local t3 = task.spawn(function() task.wait(5) end)
        coroutine.finally(t3, function(...) print("G3", select("#", ...), ...) end)
        task.cancel(t3)
        local t4 = task.spawn(function() task.wait() return 1, 2 end)
        coroutine.finally(t4, function(s, a, b) task.wait() print("G4", s, a, b) end)
        local tl = task.spawn(function() task.wait() return "x" end)
        coroutine.finally(tl, function(...) print("G9", ...) end)
        coroutine.finally(tl, function(...) print("G9b", ...) end)
    "##;
    // An error's traceback lines follow its message: compare the first.
    let lines: Vec<String> = run(source)
        .iter()
        .map(|line| line.lines().next().unwrap_or_default().to_owned())
        .collect();
    let mut sorted: Vec<&str> = lines.iter().map(String::as_str).collect();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            "FDEFER 2 finished 7",
            "FSPAWN 1 finished",
            "G1 2 error sperr",
            "G2 1 cancelled",
            "G3 1 cancelled",
            "G4 finished 1 2",
            "G9 finished x",
            "G9b finished x",
            "sperr",
            "sperr",
        ],
        "{lines:?}"
    );
    assert_eq!(lines[..2], ["G2 1 cancelled", "G3 1 cancelled"]);
    let g9b = lines.iter().position(|l| l == "G9b finished x");
    assert!(g9b < lines.iter().position(|l| l == "G9 finished x"));
}

/// Studio's "If Local/Const Bindings" beta. Off (as in Studio without the
/// beta) the parser rejects it with Studio's messages; on, the binding is
/// scoped to its block and tested for truthiness, as the beta's
/// announcement describes. One test, since the flags are process-wide.
#[test]
fn if_local_follows_the_studio_beta() {
    let parse = |src: &str| {
        run(&format!(
            "local f, e = loadstring({src:?}) print(f ~= nil, e)"
        ))
        .pop()
        .unwrap()
    };
    assert_eq!(
        parse("local t = {} if local x = t then return x end"),
        r#"false [string "local t = {} if local x = t then return x end"]:1: Expected identifier when parsing expression, got 'local'"#
    );
    assert_eq!(
        parse("if const x = 1 then return x end"),
        r#"false [string "if const x = 1 then return x end"]:1: Expected 'then' when parsing if statement, got 'x'"#
    );
    assert_eq!(
        parse("while local x = nil do end"),
        r#"false [string "while local x = nil do end"]:1: Expected identifier when parsing expression, got 'local'"#
    );

    set_if_local(true);
    let on = run(r#"
        local t = { name = "a" }
        if local x = t then print("if", x.name) else print("else") end
        if const y = nil then print("then") elseif true then print("elseif", y) end
        print("expr", if local v = t.name then v .. "!" else "none")
        local jobs, fns = { 1, 2, 3 }, {}
        while local job = table.remove(jobs) do
            table.insert(fns, function() return job end)
            if job == 2 then continue end
            print("job", job)
        end
        print("captured", fns[1](), fns[2](), fns[3]())
        local ok, e = loadstring("if local a, b = 1, 2 then end")
        print("multi", ok ~= nil)
        local ok2 = loadstring("if const c = 1 then c = 2 end")
        print("const", ok2 ~= nil)
    "#);
    set_if_local(false);
    assert_eq!(
        on,
        [
            "if a",
            "elseif nil",
            "expr a!",
            "job 3",
            "job 1",
            "captured 3 2 1",
            "multi false",
            "const false",
        ]
    );
}
