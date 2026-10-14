use mlua::Lua;

const CODEC: &str = include_str!("../src/probe/codec.luau");
const LIVE: &str = include_str!("../src/probe/live.luau");

fn compiles(name: &str, source: &str) {
    let lua = Lua::new();
    if let Err(err) = lua.load(source).set_name(name).into_function() {
        panic!("{name} does not compile: {err}");
    }
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
