use mlua::Lua;

const SERVER: &str = include_str!("../src/probe/server.luau");
const CLIENT: &str = include_str!("../src/probe/client.luau");

fn compiles(name: &str, src: &str) {
    Lua::new()
        .load(src)
        .set_name(name)
        .into_function()
        .unwrap_or_else(|e| panic!("{name} does not compile: {e}"));
}

#[test]
fn server_probe_compiles() {
    compiles("server.luau", SERVER);
}

#[test]
fn client_probe_compiles() {
    compiles("client.luau", CLIENT);
}

#[test]
fn server_probe_mentions_protocol_pieces() {
    for needle in [
        "/v1/ingest",
        "/v1/poll",
        "X-Probe-Token",
        "__rbxnative_remote",
        "PlayerRemoving",
    ] {
        assert!(SERVER.contains(needle), "server.luau lacks {needle}");
    }
}

#[test]
fn server_probe_reports_job_id_and_joins() {
    assert!(SERVER.contains("JobId"), "server.luau lacks JobId");
    assert!(
        SERVER.contains("PlayerAdded"),
        "server.luau lacks PlayerAdded"
    );
    assert!(
        SERVER.contains("'joined'") || SERVER.contains("\"joined\""),
        "server.luau lacks a joined message type"
    );
}

#[test]
fn server_probe_never_indexes_the_unscriptable_loadstring_flag() {
    // Live: "LoadStringEnabled is not a valid member of ServerScriptService".
    let server = include_str!("../src/probe/server.luau");
    assert!(!server.contains("ServerScriptService.LoadStringEnabled"));
}
