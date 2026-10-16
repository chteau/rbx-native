use rbx_wasm::{Effect, Grants, Host, WasmPlugin, MEMORY_LIMIT};
use serde_json::{json, Value};

#[derive(Default)]
struct MockHost {
    calls: Vec<String>,
}

impl Host for MockHost {
    fn call(&mut self, method: &str, _params: &Value) -> Result<Value, String> {
        self.calls.push(method.to_string());
        match method {
            "instance.roots" => Ok(json!([1, 2])),
            _ => Ok(Value::Null),
        }
    }
}

fn data(offset: u32, text: &str) -> String {
    format!(
        "(data (i32.const {offset}) \"{}\")",
        text.replace('\\', "\\\\").replace('"', "\\22")
    )
}

const ALLOC: &str = r#"
 (memory (export "memory") 1)
 (global $heap (mut i32) (i32.const 4096))
 (func (export "rbxn_alloc") (param i32) (result i32) (local i32)
   global.get $heap
   local.set 1
   global.get $heap
   local.get 0
   i32.add
   global.set $heap
   local.get 1)
"#;

const IMPORT: &str = r#"(import "rbxn" "call" (func $call (param i32 i32) (result i64)))"#;

fn module(body: &str) -> String {
    format!("(module {IMPORT} {ALLOC} {body})")
}

fn load(wat: &str, grants: Grants) -> Result<WasmPlugin, String> {
    WasmPlugin::load("test", wat.as_bytes(), grants)
}

#[test]
fn start_logs() {
    let req = r#"{"method":"log","params":{"text":"hi"}}"#;
    let wat = module(&format!(
        r#"{} (func (export "rbxn_start") (drop (call $call (i32.const 0) (i32.const {}))))"#,
        data(0, req),
        req.len()
    ));
    let mut plugin = load(&wat, Grants::default()).unwrap();
    let effects = plugin.start(&mut MockHost::default()).unwrap();
    assert_eq!(effects, vec![Effect::Log("hi".into())]);
}

#[test]
fn command_registers_then_calls_selection() {
    let reg = r#"{"method":"command.register","params":{"id":"go","text":"Go","tooltip":"t"}}"#;
    let sel = r#"{"method":"selection.get"}"#;
    let wat = module(&format!(
        r#"{} {}
        (func (export "rbxn_start") (drop (call $call (i32.const 0) (i32.const {}))))
        (func (export "rbxn_command") (param i32 i32)
          (drop (call $call (i32.const 512) (i32.const {}))))"#,
        data(0, reg),
        data(512, sel),
        reg.len(),
        sel.len()
    ));
    let grants = Grants::from_permissions(&["selection"]);
    let mut plugin = load(&wat, grants).unwrap();
    let mut host = MockHost::default();
    let effects = plugin.start(&mut host).unwrap();
    assert_eq!(
        effects,
        vec![Effect::Command {
            id: "go".into(),
            text: "Go".into(),
            tooltip: "t".into()
        }]
    );
    plugin.command("go", &mut host).unwrap();
    assert_eq!(host.calls, vec!["selection.get".to_string()]);
}

#[test]
fn missing_permission_is_reported() {
    let roots = r#"{"method":"instance.roots"}"#;
    let prefix = r#"{"method":"log","params":{"text":""#;
    let suffix = r#""}}"#;
    let pl = prefix.len();
    let dst = 1024 + pl;
    let rl = roots.len();
    let wat = module(&format!(
        r#"{} {} {}
        (func (export "rbxn_start") (local $r i64) (local $ptr i32) (local $n i32) (local $m i32)
          (local.set $r (call $call (i32.const 0) (i32.const {rl})))
          (local.set $ptr (i32.wrap_i64 (i64.shr_u (local.get $r) (i64.const 32))))
          (local.set $n (i32.wrap_i64 (local.get $r)))
          (local.set $m (i32.sub (local.get $n) (i32.const 12)))
          (memory.copy (i32.const 1024) (i32.const 300) (i32.const {pl}))
          (memory.copy (i32.const {dst}) (i32.add (local.get $ptr) (i32.const 10)) (local.get $m))
          (memory.copy (i32.add (i32.const {dst}) (local.get $m)) (i32.const 400) (i32.const 3))
          (drop (call $call (i32.const 1024)
            (i32.add (i32.add (i32.const {pl}) (local.get $m)) (i32.const 3)))))"#,
        data(0, roots),
        data(300, prefix),
        data(400, suffix),
    ));
    let mut plugin = load(&wat, Grants::default()).unwrap();
    let mut host = MockHost::default();
    let effects = plugin.start(&mut host).unwrap();
    assert_eq!(
        effects,
        vec![Effect::Log("permission 'dom.read' not granted".into())]
    );
    assert!(host.calls.is_empty());
}

#[test]
fn infinite_loop_runs_out_of_fuel() {
    let wat = module(r#"(func (export "rbxn_start") (loop $l (br $l)))"#);
    let mut plugin = load(&wat, Grants::default()).unwrap();
    let err = plugin.start(&mut MockHost::default()).unwrap_err();
    assert!(err.contains("fuel"), "{err}");
}

#[test]
fn missing_alloc_fails_to_load() {
    let wat = r#"(module (memory (export "memory") 1) (func (export "rbxn_start")))"#;
    assert!(load(wat, Grants::default()).is_err());
}

#[test]
fn growing_past_memory_limit_fails() {
    let pages = MEMORY_LIMIT / 65536 + 1;
    let wat = module(&format!(
        r#"(func (export "rbxn_start")
          (if (i32.eq (memory.grow (i32.const {pages})) (i32.const -1)) (then unreachable)))"#
    ));
    let failed = match load(&wat, Grants::default()) {
        Err(_) => true,
        Ok(mut plugin) => plugin.start(&mut MockHost::default()).is_err(),
    };
    assert!(failed);
}
