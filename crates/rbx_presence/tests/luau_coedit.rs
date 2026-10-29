//! Runs the plugin's `CoeditCore.luau` in plain Luau (mlua) against the shared
//! golden fixtures, and syntax-checks every plugin module.

mod common;

use common::{load_module, new_lua, root, src_dir};
use mlua::{Function, Lua, Table};
use serde_json::Value as Json;

fn vectors() -> Json {
    let path = root().join("plugins/team-presence/fixtures/coedit_vectors.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("fixture is JSON")
}

fn core_fn(lua: &Lua, name: &str) -> Function {
    load_module(lua, "CoeditCore")
        .unwrap()
        .get(name)
        .unwrap_or_else(|e| panic!("CoeditCore.{name}: {e}"))
}

fn s<'a>(v: &'a Json, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_else(|| panic!("missing {key}"))
}

#[test]
fn text_hash_matches() {
    let lua = new_lua();
    let hash = core_fn(&lua, "textHash");
    for case in vectors()["text_hash"].as_array().unwrap() {
        let got: String = hash.call(s(case, "text")).unwrap();
        assert_eq!(got, s(case, "hash"));
    }
}

#[test]
fn chunks_equal_rust_and_reassemble() {
    let lua = new_lua();
    let split = core_fn(&lua, "split");
    let fixture = vectors();
    let chunks = &fixture["chunks"];
    let batch = s(chunks, "batch_json");
    let got: Table = split.call((s(chunks, "id"), batch)).unwrap();
    let expected = chunks["chunks"].as_array().unwrap();
    assert_eq!(got.len().unwrap() as usize, expected.len());
    for (k, want) in expected.iter().enumerate() {
        let chunk: Table = got.get(k + 1).unwrap();
        assert_eq!(chunk.get::<String>("b").unwrap(), s(want, "b"));
        assert_eq!(chunk.get::<String>("d").unwrap(), s(want, "d"));
        assert_eq!(chunk.get::<u64>("i").unwrap(), want["i"].as_u64().unwrap());
        assert_eq!(chunk.get::<u64>("n").unwrap(), want["n"].as_u64().unwrap());
    }

    let reassembler: Table = load_module(&lua, "CoeditCore")
        .unwrap()
        .get::<Table>("Reassembler")
        .unwrap()
        .get::<Function>("new")
        .unwrap()
        .call(())
        .unwrap();
    let push: Function = reassembler.get("push").unwrap();
    let mut out: Option<String> = None;
    for k in 0..expected.len() {
        let chunk: Table = got.get(k + 1).unwrap();
        let r: Option<String> = push
            .call((reassembler.clone(), k as u64 + 1, "42:studio", chunk))
            .unwrap();
        if r.is_some() {
            out = r;
        }
    }
    assert_eq!(out.as_deref(), Some(batch));
}

#[test]
fn reassembler_rejects_malformed() {
    let lua = new_lua();
    lua.globals()
        .set("core", load_module(&lua, "CoeditCore").unwrap())
        .unwrap();
    lua.load(
        r#"
        local r = core.Reassembler.new()
        assert(r:push(1, "a", { b = "x", i = 2, n = 2, d = "" }) == nil)
        assert(r:push(2, "a", { b = "x", i = 0, n = 0, d = "" }) == nil)
        assert(r:push(3, "a", { b = "x", i = 0, n = 1, d = "!!!" }) == nil)
        assert(r:push(4, "a", { b = "y", i = 0, n = 2, d = "YQ==" }) == nil)
        assert(r:push(5, "a", { b = "y", i = 1, n = 3, d = "Yg==" }) == nil)
        assert(r:push(6, "b", { b = "y", i = 1, n = 2, d = "Yg==" }) == nil)
        assert(r:push(7, "a", { b = "y", i = 1, n = 2, d = "Yg==" }) == "ab")
        assert(r:push(8, "a", { b = "e", i = 0, n = 1, d = "" }) == "")
        local stale = core.Reassembler.new()
        assert(stale:push(1, "a", { b = "z", i = 0, n = 2, d = "YQ==" }) == nil)
        assert(stale:push(5000, "a", { b = "z", i = 1, n = 2, d = "Yg==" }) == nil)
        "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn merge3_matches_rust() {
    let lua = new_lua();
    let merge3 = core_fn(&lua, "merge3");
    for case in vectors()["merge3"].as_array().unwrap() {
        let r: Table = merge3
            .call((s(case, "base"), s(case, "ours"), s(case, "theirs")))
            .unwrap();
        let text: String = r.get("text").unwrap();
        let conflicted: bool = r.get("conflicted").unwrap();
        assert_eq!(conflicted, case["conflicted"].as_bool().unwrap());
        if conflicted {
            assert!(
                text.contains("<<<<<<< ours")
                    && text.contains("=======")
                    && text.contains(">>>>>>> theirs")
            );
            assert!(text.contains(s(case, "ours").lines().nth(1).unwrap()));
            assert!(text.contains(s(case, "theirs").lines().nth(1).unwrap()));
        } else {
            assert_eq!(text, s(case, "text"));
        }
    }
}

#[test]
fn op_json_decodes() {
    let lua = new_lua();
    let decode: Function = load_module(&lua, "Json").unwrap().get("decode").unwrap();
    for case in vectors()["ops"].as_array().unwrap() {
        let op = &case["op_json"];
        let got: Table = decode.call(serde_json::to_string(op).unwrap()).unwrap();
        assert_eq!(got.get::<String>("k").unwrap(), s(op, "k"));
        assert_eq!(got.get::<String>("p").unwrap(), s(op, "p"));
        assert_eq!(got.get::<String>("id").unwrap(), s(op, "id"));
        let clock: Table = got.get("c").unwrap();
        assert_eq!(clock.get::<u64>(1).unwrap(), op["c"][0].as_u64().unwrap());
        assert_eq!(
            clock.get::<String>(2).unwrap(),
            op["c"][1].as_str().unwrap()
        );
        if op["k"] == "src" {
            assert_eq!(got.get::<String>("text").unwrap(), s(op, "text"));
            assert_eq!(got.get::<String>("base").unwrap(), s(op, "base"));
        }
    }
}

#[test]
fn registers_and_clocks() {
    let lua = new_lua();
    lua.globals()
        .set("core", load_module(&lua, "CoeditCore").unwrap())
        .unwrap();
    lua.load(
        r#"
        assert(core.clockLess({ 1, "b" }, { 2, "a" }))
        assert(core.clockLess({ 2, "a" }, { 2, "b" }))
        assert(not core.clockLess({ 2, "a" }, { 2, "a" }))
        local r = core.Registers.new()
        assert(r:claim("k", { 5, "x" }))
        assert(not r:claim("k", { 4, "z" }))
        assert(not r:claim("k", { 5, "x" }))
        assert(r:claim("k", { 5, "y" }))
        assert(r:claim("other", { 1, "a" }))
        assert(#core.session() == 8 and core.session():match("^%x+$"))
        "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn log_src_rules() {
    let lua = new_lua();
    lua.globals()
        .set("core", load_module(&lua, "CoeditCore").unwrap())
        .unwrap();
    lua.load(
        r#"
        local log = core.Log.new()
        local h = log:hash("k", "a\n")
        assert(h == core.textHash("a\n"))
        local r = log:src("k", "a\n", { base = h, text = "b\n" }, false)
        assert(r.accepted and r.write == "b\n" and not r.conflicted)
        r = log:src("k", "b\n", { base = h, text = "c\n" }, false)
        assert(not r.accepted and r.write == nil and r.resend == nil)
        r = log:src("k", "b\n", { base = core.textHash("b\n"), text = "d\n" }, true)
        assert(r.accepted and r.write == nil)

        local m = core.Log.new()
        local h0 = m:hash("s", "a\nb\nc\nd\ne\n")
        r = m:src("s", "A\nb\nc\nd\ne\n", { base = h0, text = "a\nb\nc\nd\nE\n" }, false)
        assert(r.accepted and r.write == "A\nb\nc\nd\nE\n" and not r.conflicted)

        local o = core.Log.new()
        local base = "a\nb\nc\nd\ne\n"
        local oh = o:hash("s", base)
        r = o:src("s", base, { base = oh, text = "a\nb\nc\nD\ne\n" }, false)
        assert(r.accepted)
        -- our in-flight edit was already merged into the editor's text when theirs landed
        r = o:src("s", "a\nB\nc\nD\ne\n", { base = oh, text = "a\nB\nc\nd\ne\n" }, true)
        assert(not r.accepted and r.resend == "a\nB\nc\nD\ne\n" and not r.conflicted)

        base = "a\nb\nc\n"
        local c = core.Log.new()
        local ch = c:hash("s", base)
        r = c:src("s", "a\nours\nc\n", { base = ch, text = "a\ntheirs\nc\n" }, false)
        assert(r.accepted and r.conflicted and r.write:find("ours", 1, true) and r.write:find("theirs", 1, true))
        "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn every_plugin_module_compiles() {
    let lua = Lua::new();
    let mut count = 0;
    for entry in std::fs::read_dir(src_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "luau") {
            let src = std::fs::read_to_string(&path).unwrap();
            let name = format!("={}", path.file_name().unwrap().to_string_lossy());
            lua.load(src.as_str())
                .set_name(name)
                .into_function()
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            count += 1;
        }
    }
    assert!(count > 0);
}

/// Attributes (`@name`) and tags (`#tag`) read, encode and apply through
/// CoeditApply on a stand-in instance, removals as `null`.
#[test]
fn attributes_and_tags_apply_one_by_one() {
    let lua = new_lua();
    lua.load(
        r##"
        __modules.CoeditClasses = {}
        game = { GetService = function() return {} end }
        local attrs, tags = { Speed = 16 }, { Lava = true }
        part = {
            ClassName = "Part",
            GetFullName = function() return "Workspace.P" end,
            GetAttribute = function(_, n) return attrs[n] end,
            SetAttribute = function(_, n, v) attrs[n] = v end,
            HasTag = function(_, t) return tags[t] == true end,
            AddTag = function(_, t) tags[t] = true end,
            RemoveTag = function(_, t) tags[t] = nil end,
        }
        attrsOf, tagsOf = function() return attrs end, function() return tags end
        "##,
    )
    .exec()
    .unwrap();
    let apply = load_module(&lua, "CoeditApply").unwrap();
    let json = load_module(&lua, "Json").unwrap();
    let result: (String, String, String, String) = lua
        .load(
            r##"
            local Apply, Json = ...
            local state = { idOf = {}, byId = { p1 = part }, marks = {}, registers = __modules.CoeditCore.Registers.new(), log = __modules.CoeditCore.Log.new(), counter = 0 }
            local before = Apply.encodeProp(state, part, "@Speed") .. Apply.encodeProp(state, part, "#Lava") .. Apply.encodeProp(state, part, "@Gone")
            local batch = Json.decode('{"a":"1:native:x","ops":[' ..
                '{"k":"set","id":"p1","p":"","n":"@Speed","v":null,"c":[1,"1:native:x"]},' ..
                '{"k":"set","id":"p1","p":"","n":"@Team","v":{"t":"String","v":"red"},"c":[2,"1:native:x"]},' ..
                '{"k":"set","id":"p1","p":"","n":"#Lava","v":null,"c":[3,"1:native:x"]},' ..
                '{"k":"set","id":"p1","p":"","n":"#Kill","v":{"t":"Bool","v":true},"c":[4,"1:native:x"]}]}')
            local r = Apply.new(state):batch(batch, false, "Chee")
            local a, t = attrsOf(), tagsOf()
            return before, tostring(r.applied), tostring(a.Speed) .. "," .. tostring(a.Team), tostring(t.Lava) .. "," .. tostring(t.Kill)
            "##,
        )
        .call((apply, json))
        .unwrap();
    assert_eq!(
        result.0,
        r##"{"t":"Float64","v":16}{"t":"Bool","v":true}null"##
    );
    assert_eq!(result.1, "4");
    assert_eq!(result.2, "nil,red");
    assert_eq!(result.3, "nil,true");
}

/// When the room's host changes, the plugin hands a new host that holds no
/// log the one it read (same seqs), and reads a host running another log
/// from its start.
#[test]
fn a_new_host_is_handed_the_log_or_read_from_the_start() {
    let lua = new_lua();
    common::studio_stubs(&lua);
    lua.load("__modules.CoeditClasses = {}").exec().unwrap();
    let coedit = load_module(&lua, "Coedit").unwrap();
    let asked: Table = lua
        .load(
            r##"
            local Coedit = ...
            local asked = {}
            local replies = {
                { log = "abc", head = 2, floor = 1, entries = {
                    { seq = 1, userId = 5, client = "studio", ct = "AAAA" },
                    { seq = 2, userId = 6, client = "rbx-native", ct = "BBBB" } } },
                { log = "", head = 0, floor = 1, entries = {} },
                { log = "zzz", head = 1, floor = 1, entries = {} },
                { log = "zzz", head = 1, floor = 1, entries = {} },
            }
            local relay = {}
            function relay.pollOps(_, since)
                table.insert(asked, "poll@" .. since)
                return table.remove(replies, 1)
            end
            function relay.postOps(_, body, fill)
                local sent = __modules.Json.decode(body)
                local seqs = {}
                for _, e in sent.entries do table.insert(seqs, e.seq .. "/" .. e.userId .. "/" .. e.client) end
                table.insert(asked, (if fill then "fill " else "post ") .. sent.log .. ":" .. table.concat(seqs, ","))
                return true
            end
            local c = Coedit.new({ relay = relay, userId = 1, room = "r", encKey = string.rep("k", 32), name = "n", onNote = function() end })
            c.running = true
            c.reassembler = __modules.CoeditCore.Reassembler.new()
            for _ = 1, 4 do c:poll() end
            return asked
            "##,
        )
        .call(coedit)
        .unwrap();
    let asked: Vec<String> = asked.sequence_values().map(|v| v.unwrap()).collect();
    assert_eq!(
        asked,
        [
            "poll@0",
            "poll@2",
            "fill abc:1/5/studio,2/6/rbx-native",
            "poll@2",
            "poll@0"
        ]
    );
}
