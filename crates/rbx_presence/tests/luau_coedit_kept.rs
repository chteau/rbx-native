//! The plugin's copy of the op log (`CoeditKept.luau`) in plain Luau: the
//! same handover decisions as `rbx_presence::coedit::keep`.

mod common;

use common::{load_module, new_lua};
use mlua::{Function, Lua, Table, Value};

const SETUP: &str = r#"
    local Kept = ...
    local function entry(seq) return { seq = seq, userId = 1, client = "studio", ct = "E" .. seq } end
    local function kept(log, from, to, cap)
        local k = Kept.new(cap)
        for seq = from, to do k:record(log, entry(seq)) end
        return k
    end
    local function seqs(list)
        local out = {}
        for _, e in list or {} do table.insert(out, e.seq) end
        return table.concat(out, ",")
    end
    local function step(k, log, head)
        local what, list = k:step({ log = log, head = head })
        return what .. ":" .. seqs(list)
    end
    return kept, step
"#;

fn helpers(lua: &Lua) -> (Function, Function) {
    let module = load_module(lua, "CoeditKept").unwrap();
    lua.load(SETUP)
        .call::<(Function, Function)>(module)
        .unwrap()
}

fn step(lua: &Lua, kept: &Table, log: &str, head: u64) -> String {
    let (_, step) = helpers(lua);
    step.call((kept.clone(), log, head)).unwrap()
}

fn kept(lua: &Lua, log: &str, from: u64, to: u64) -> Table {
    let (kept, _) = helpers(lua);
    kept.call((log, from, to, Value::Nil)).unwrap()
}

#[test]
fn an_empty_relay_is_handed_the_whole_log() {
    let lua = new_lua();
    let k = kept(&lua, "abc", 1, 3);
    assert_eq!(step(&lua, &k, "", 0), "fill:1,2,3");
    let empty = kept(&lua, "", 1, 0);
    assert_eq!(step(&lua, &empty, "", 0), "read:");
}

#[test]
fn a_relay_behind_on_the_same_log_gets_the_tail() {
    let lua = new_lua();
    let k = kept(&lua, "abc", 1, 5);
    assert_eq!(step(&lua, &k, "abc", 3), "fill:4,5");
    assert_eq!(step(&lua, &k, "abc", 5), "read:");
    assert_eq!(step(&lua, &k, "abc", 9), "read:");
    let trimmed = kept(&lua, "abc", 6, 8);
    assert_eq!(step(&lua, &trimmed, "abc", 3), "read:");
}

#[test]
fn another_log_restarts() {
    let lua = new_lua();
    let k = kept(&lua, "abc", 1, 2);
    assert_eq!(step(&lua, &k, "xyz", 1), "restart:");
    let fresh = kept(&lua, "", 1, 0);
    assert_eq!(step(&lua, &fresh, "xyz", 4), "read:");
}

#[test]
fn the_cap_trims_the_oldest_and_head_follows() {
    let lua = new_lua();
    let (kept, step) = helpers(&lua);
    // Each entry is 2 bytes of ct ("E1".."E9"); a 4-byte cap keeps two.
    let k: Table = kept.call(("abc", 1, 5, 4)).unwrap();
    let head: Function = k.get("head").unwrap();
    assert_eq!(head.call::<u64>(k.clone()).unwrap(), 5);
    let got: String = step.call((k, "", 0)).unwrap();
    assert_eq!(got, "fill:4,5");
}

#[test]
fn groups_stay_under_the_cap_and_keep_order() {
    let lua = new_lua();
    let module = load_module(&lua, "CoeditKept").unwrap();
    let got: String = lua
        .load(
            r#"
            local Kept = ...
            local out = {}
            for _, g in Kept.groups({ 4, 4, 4, 9, 1 }, 8, function(n) return n end) do
                table.insert(out, table.concat(g, "+"))
            end
            return table.concat(out, " ")
            "#,
        )
        .call(module)
        .unwrap();
    assert_eq!(got, "4+4 4 9 1");
}
