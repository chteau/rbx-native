use rbx_play::instrument::{instrument, resolve_line, Hook};

const SCRIPT: &str = "--!strict
local a = 1
local function f(p, q)
  local before = p
  do local inner = 2 end
  local after = q
  return before
end
f(10, 20)
local function foo(cb) cb(5) end
foo(function(x)
  local y = x
end)
";

const STUB: &str = r#"
HITS = {}
LINES = ""
PRINTED = {}
print = function(...) PRINTED[#PRINTED + 1] = table.concat({...}, " ") end
local marker = {}
game = { GetService = function(_, _) return { WaitForChild = function(_, _) return marker end } end }
local D = { s = false }
function D.lines(id, sv, cl)
  LINES = id .. ":" .. table.concat(sv, ",") .. ":" .. table.concat(cl, ",")
  return setmetatable({}, { __index = function() return true end })
end
function D.hit(id, line, names, values, cond, log, cont)
  local c = nil
  if cond then c = cond() end
  if log then log() end
  HITS[#HITS + 1] = { line = line, names = names, values = values, cond = c, cont = cont }
end
require = function(m) assert(m == marker) return D end
"#;

fn hooks() -> Vec<Hook> {
    vec![
        Hook {
            line: 6,
            condition: Some("before == 10".into()),
            log_message: Some("'b=' .. before".into()),
            continue_execution: true,
            server: false,
            client: true,
        },
        Hook {
            line: 8,
            server: true,
            ..Hook::default()
        },
    ]
}

fn run() -> mlua::Lua {
    let out = instrument(SCRIPT, 7, &hooks()).unwrap();
    let lua = mlua::Lua::new();
    lua.load(STUB).exec().unwrap();
    lua.load(&out.source).exec().unwrap();
    lua
}

fn eval(lua: &mlua::Lua, code: &str) -> String {
    lua.load(code).eval::<String>().unwrap()
}

fn at_line(lua: &mlua::Lua, line: u32) -> String {
    eval(
        lua,
        &format!(
            "for _, h in ipairs(HITS) do if h.line == {line} then
               local v = {{}}
               for i = 1, 5 do v[i] = tostring(h.values[i]) end
               return h.names .. '|' .. table.concat(v, ',') .. '|' .. tostring(h.cond) .. '|' .. tostring(h.cont)
             end end return 'none'"
        ),
    )
}

#[test]
fn keeps_lines() {
    let out = instrument(SCRIPT, 7, &hooks()).unwrap();
    let orig: Vec<&str> = SCRIPT.lines().collect();
    let new: Vec<&str> = out.source.lines().collect();
    assert_eq!(orig.len(), new.len());
    for (o, n) in orig.iter().zip(&new) {
        let indent = &o[..o.len() - o.trim_start().len()];
        assert!(n.starts_with(indent), "{n:?} lost the indent of {o:?}");
        assert!(n.ends_with(o.trim_start()), "{n:?} does not end with {o:?}");
    }
    assert_eq!(out.lines, vec![2, 3, 4, 5, 6, 7, 9, 10, 11, 12]);
}

#[test]
fn directive_stays_first() {
    let out = instrument(SCRIPT, 7, &[]).unwrap();
    let new: Vec<&str> = out.source.lines().collect();
    assert_eq!(new[0], "--!strict");
    assert!(new[1].starts_with("local __rbxdbg_D = require("));
    assert!(new[1].ends_with("local a = 1"));
}

#[test]
fn hits_in_order_with_scope() {
    let lua = run();
    let lines = eval(
        &lua,
        "local t = {} for _, h in ipairs(HITS) do t[#t + 1] = h.line end return table.concat(t, ',')",
    );
    assert_eq!(lines, "2,3,9,4,5,6,7,10,11,12");
    assert_eq!(eval(&lua, "return LINES"), "7:9:6");
    let line4 = at_line(&lua, 4);
    assert!(line4.starts_with("a f p q|1,"), "{line4}");
    let line12 = at_line(&lua, 12);
    assert!(line12.starts_with("a f foo x|1,"), "{line12}");
    assert!(line12.contains(",5,"), "{line12}");
}

#[test]
fn condition_and_log_see_locals() {
    let lua = run();
    let line6 = at_line(&lua, 6);
    assert!(line6.starts_with("a f p q before|1,function"), "{line6}");
    assert!(line6.ends_with(",10,20,10|true|true"), "{line6}");
    assert_eq!(eval(&lua, "return PRINTED[1]"), "b=10");
    assert!(at_line(&lua, 7).ends_with("|nil|nil"));
}

#[test]
fn parse_error() {
    assert!(instrument("local = = 3", 1, &[]).is_err());
}

#[test]
fn empty_source_unchanged() {
    let out = instrument("-- nothing\n", 1, &[]).unwrap();
    assert_eq!(out.source, "-- nothing\n");
    assert!(out.lines.is_empty());
}

#[test]
fn resolve_edges() {
    assert_eq!(resolve_line(&[], 3), None);
    assert_eq!(resolve_line(&[2, 5, 9], 1), Some(2));
    assert_eq!(resolve_line(&[2, 5, 9], 5), Some(5));
    assert_eq!(resolve_line(&[2, 5, 9], 6), Some(9));
    assert_eq!(resolve_line(&[2, 5, 9], 10), None);
}
