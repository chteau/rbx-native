use mlua::Lua;

const PRINT: &str = include_str!("../src/probe/print.luau");

fn lua_with_print() -> Lua {
    let lua = Lua::new();
    let module = lua
        .load(PRINT)
        .set_name("print.luau")
        .eval::<mlua::Table>()
        .expect("print module");
    lua.globals().set("P", module).expect("set P");
    lua.load(
        r#"
        calls = {}
        original = function(...) table.insert(calls, table.pack(...)) end
        "#,
    )
    .exec()
    .expect("original stub");
    lua
}

fn run(lua: &Lua, source: &str) {
    lua.load(source)
        .set_name("test")
        .exec()
        .expect("script assertions");
}

#[test]
fn wrap_forwards_arguments_to_original() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        wrapped(1, nil, "a")
        assert(#calls == 1)
        assert(calls[1].n == 3)
        assert(calls[1][1] == 1)
        assert(calls[1][2] == nil)
        assert(calls[1][3] == "a")
        "#,
    );
}

#[test]
fn take_matches_space_joined_tostring_with_nil() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        wrapped(1, nil, "a")
        local record = P.take("1 nil a")
        assert(record ~= nil)
        assert(record.parts == nil)
        "#,
    );
}

#[test]
fn take_drops_older_unmatched_records() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        wrapped("a")
        wrapped("b")
        assert(P.take("b") ~= nil)
        assert(P.take("a") == nil)
        "#,
    );
}

#[test]
fn take_without_match_returns_nil_and_keeps_fifo() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        wrapped("x")
        assert(P.take("zzz") == nil)
        assert(P.take("x") ~= nil)
        "#,
    );
}

#[test]
fn fifo_drops_oldest_beyond_capacity() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        wrapped("first")
        for i = 1, 256 do
            wrapped("n" .. i)
        end
        assert(P.take("first") == nil)
        assert(P.take("n256") ~= nil)
        "#,
    );
}

#[test]
fn scalar_print_has_no_parts() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        wrapped("hi", 2, true)
        local record = P.take("hi 2 true")
        assert(record ~= nil)
        assert(record.parts == nil)
        "#,
    );
}

#[test]
fn table_parts_have_sorted_quoted_keys() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        local t = { z = 1, a = 2, [2] = "two", [1] = "one" }
        local msg = tostring(t)
        wrapped(t)
        local record = P.take(msg)
        assert(record ~= nil)
        local dump = record.parts[1]
        assert(type(dump.address) == "string")
        assert(#dump.entries == 4)
        assert(dump.entries[1].key == "1")
        assert(dump.entries[2].key == "2")
        assert(dump.entries[3].key == "\"a\"")
        assert(dump.entries[4].key == "\"z\"")
        assert(dump.entries[1].value == "\"one\"")
        assert(dump.entries[2].value == "\"two\"")
        assert(dump.entries[3].value == "2")
        assert(dump.more == 0)
        "#,
    );
}

#[test]
fn nested_table_is_dumped_recursively() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        local t = { inner = { x = 1 } }
        local msg = tostring(t)
        wrapped(t)
        local dump = P.take(msg).parts[1]
        assert(dump.entries[1].key == "\"inner\"")
        local nested = dump.entries[1].value
        assert(type(nested) == "table")
        assert(nested.entries[1].key == "\"x\"")
        assert(nested.entries[1].value == "1")
        "#,
    );
}

#[test]
fn cycle_becomes_reference_to_open_table() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        local t = {}
        t.self = t
        local msg = tostring(t)
        wrapped(t)
        local dump = P.take(msg).parts[1]
        assert(dump.entries[1].key == "\"self\"")
        assert(dump.entries[1].value.reference == dump.address)
        "#,
    );
}

#[test]
fn table_at_depth_eight_becomes_reference() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        local top = {}
        local cur = top
        for _ = 1, 10 do
            cur.next = {}
            cur = cur.next
        end
        local msg = tostring(top)
        wrapped(top)
        local node = P.take(msg).parts[1]
        for _ = 1, 7 do
            node = node.entries[1].value
            assert(node.entries ~= nil)
        end
        local deep = node.entries[1].value
        assert(deep.reference ~= nil)
        assert(deep.entries == nil)
        "#,
    );
}

#[test]
fn tostring_metatable_table_is_a_string_part() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        local custom = setmetatable({}, { __tostring = function() return "custom" end })
        local walkable = {}
        local msg = "custom " .. tostring(walkable)
        wrapped(custom, walkable)
        local record = P.take(msg)
        assert(record ~= nil)
        assert(record.parts[1] == "custom")
        assert(type(record.parts[2]) == "table")
        "#,
    );
}

#[test]
fn large_table_is_capped_at_200_entries() {
    let lua = lua_with_print();
    run(
        &lua,
        r#"
        local wrapped = P.wrap(original)
        local t = {}
        for i = 1, 250 do
            t[i] = i
        end
        local msg = tostring(t)
        wrapped(t)
        local dump = P.take(msg).parts[1]
        assert(#dump.entries == 200)
        assert(dump.more == 50)
        "#,
    );
}

#[test]
fn origin_is_caller_script_and_line() {
    let lua = lua_with_print();
    lua.load(
        "local p = P.wrap(original)\n\np('hi')\nrec = P.take('hi')\nassert(rec.origin.script == 'Main' and rec.origin.line == 3)\n",
    )
    .set_name("Main")
    .exec()
    .expect("caller chunk");
}
