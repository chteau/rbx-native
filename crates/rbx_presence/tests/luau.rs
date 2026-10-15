//! Runs the plugin's pure Luau modules in plain Luau (mlua) against the
//! shared golden fixtures, so the plugin and the Rust side cannot drift apart.

mod common;

use common::{load_module, new_lua, root, src_dir};
use mlua::{Function, Lua, Table, Value};
use serde_json::Value as Json;

fn fixture_text(name: &str) -> String {
    let path = root().join("plugins/team-presence/fixtures").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn fixture_json(name: &str) -> Json {
    serde_json::from_str(&fixture_text(name)).expect("fixture is JSON")
}

fn func(lua: &Lua, module: &str, name: &str) -> Function {
    load_module(lua, module)
        .unwrap()
        .get(name)
        .unwrap_or_else(|e| panic!("{module}.{name}: {e}"))
}

fn str_field<'a>(v: &'a Json, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_else(|| panic!("missing {key}"))
}

fn num_field(v: &Json, key: &str) -> f64 {
    v[key].as_f64().unwrap_or_else(|| panic!("missing {key}"))
}

#[test]
fn crypto_sha256() {
    let lua = new_lua();
    let hex = func(&lua, "Sha256", "hex");
    let vectors = fixture_json("auth_vectors.json");
    for case in vectors["sha256"].as_array().unwrap() {
        let got: String = hex.call(str_field(case, "input")).unwrap();
        assert_eq!(got, str_field(case, "sha256"), "input {:?}", case["input"]);
    }
}

#[test]
fn crypto_hmac() {
    let lua = new_lua();
    let hmac = func(&lua, "Hmac", "sha256Hex");
    let vectors = fixture_json("auth_vectors.json");
    for case in vectors["hmac"].as_array().unwrap() {
        let got: String = hmac
            .call((str_field(case, "key"), str_field(case, "message")))
            .unwrap();
        assert_eq!(got, str_field(case, "hmac"), "key {:?}", case["key"]);
    }
}

#[test]
fn crypto_requests() {
    let lua = new_lua();
    let derive = func(&lua, "Auth", "deriveKey");
    let room = func(&lua, "Auth", "roomId");
    let canonical = func(&lua, "Auth", "canonical");
    let sign = func(&lua, "Auth", "sign");
    let headers = func(&lua, "Auth", "headers");
    let vectors = fixture_json("auth_vectors.json");
    for case in vectors["requests"].as_array().unwrap() {
        let key: String = derive
            .call((str_field(case, "secret"), num_field(case, "placeId")))
            .unwrap();
        assert_eq!(key, str_field(case, "key"));
        let room_id: String = room.call(key.clone()).unwrap();
        assert_eq!(room_id, str_field(case, "room"));

        let args = (
            str_field(case, "method"),
            str_field(case, "path"),
            num_field(case, "userId"),
            num_field(case, "ts"),
            str_field(case, "body"),
        );
        let canon: String = canonical.call(args).unwrap();
        assert_eq!(canon, str_field(case, "canonical"));
        let sig: String = sign.call((key.clone(), canon)).unwrap();
        assert_eq!(sig, str_field(case, "sig"));

        let map: Table = headers
            .call((key.clone(), args.0, args.1, args.2, args.3, args.4, true))
            .unwrap();
        assert_eq!(map.pairs::<Value, Value>().count(), 4);
        let get = |name: &str| map.get::<String>(name).unwrap();
        assert_eq!(get("X-Presence-Sig"), sig);
        assert_eq!(get("X-Presence-Key"), key);
        assert_eq!(get("X-Presence-Time"), format!("{:.0}", args.3));
        assert_eq!(get("X-Presence-User"), format!("{:.0}", args.2));
    }
}

#[test]
fn data_json_round_trip() {
    let lua = new_lua();
    let decode = func(&lua, "Json", "decode");
    let encode = func(&lua, "Json", "encode");
    for name in ["state_studio.json", "state_native.json", "snapshot.json"] {
        let text = fixture_text(name);
        let text = text.trim();
        let value: Value = decode.call(text).unwrap();
        let again: String = encode.call(value).unwrap();
        assert_eq!(again, text, "{name}");
    }
}

#[test]
fn data_schema() {
    let lua = new_lua();
    let decode = func(&lua, "Schema", "decodeState");
    let encode = func(&lua, "Schema", "encode");
    let text = fixture_text("state_studio.json");
    let state: Table = decode.call(text.trim()).unwrap();
    let out: String = encode.call(state).unwrap();
    let got: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(got, serde_json::from_str::<Json>(&text).unwrap());

    let decode_snapshot = func(&lua, "Schema", "decodeSnapshot");
    let snapshot: Table = decode_snapshot
        .call(fixture_text("snapshot.json").trim())
        .unwrap();
    let expected = fixture_json("snapshot.json");
    assert_eq!(
        snapshot.get::<f64>("cursor").unwrap(),
        num_field(&expected, "cursor")
    );
    let users: Table = snapshot.get("users").unwrap();
    let expected_users = expected["users"].as_array().unwrap();
    assert_eq!(users.raw_len(), expected_users.len());
    for (i, want) in expected_users.iter().enumerate() {
        let user: Table = users.get(i + 1).unwrap();
        assert_eq!(
            user.get::<f64>("userId").unwrap(),
            num_field(want, "userId")
        );
        assert_eq!(user.get::<String>("name").unwrap(), str_field(want, "name"));
        assert_eq!(
            user.get::<String>("client").unwrap(),
            str_field(want, "client")
        );
    }

    let color_for = func(&lua, "Schema", "colorFor");
    for id in 0u64..20 {
        let got: String = color_for.call(id as f64).unwrap();
        assert_eq!(got, rbx_presence::schema::color_for(id));
    }
}

#[test]
fn data_colors() {
    let lua = new_lua();
    let slot = func(&lua, "Schema", "colorSlot");
    let assign = func(&lua, "Schema", "assignColors");
    let fixture = fixture_json("colors.json");
    for case in fixture["hashes"].as_array().unwrap() {
        let got: f64 = slot.call(num_field(case, "userId")).unwrap();
        assert_eq!(got, num_field(case, "slot"), "{}", case["userId"]);
    }
    for case in fixture["rooms"].as_array().unwrap() {
        let ids = |key: &str| -> Vec<f64> {
            case[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_f64().unwrap())
                .collect()
        };
        let got: Table = assign.call((ids("room"), ids("extra"))).unwrap();
        let want = case["colors"].as_object().unwrap();
        assert_eq!(got.pairs::<Value, Value>().count(), want.len(), "{case}");
        for (id, color) in want {
            let id: f64 = id.parse().unwrap();
            assert_eq!(got.get::<String>(id).unwrap(), color.as_str().unwrap());
        }
    }
}

fn edits(body: &str) -> String {
    let lua = new_lua();
    lua.globals()
        .set("Edits", load_module(&lua, "Edits").unwrap())
        .unwrap();
    let prelude = "local function s(l) local o = {} for _, e in ipairs(l) do \
        table.insert(o, e.path .. '.' .. e.property .. '=' .. e.value) end \
        return table.concat(o, ',') end local t = Edits.new() ";
    lua.load(format!("{prelude}{body}")).eval().unwrap()
}

#[test]
fn data_edits_recording() {
    let committed = "t:recordingStarted(0) t:changed('A','P','v',10) \
        t:recordingFinished(true,20) return s(t:recent(30))";
    assert_eq!(edits(committed), "A.P=v");
    let cancelled = "t:recordingStarted(0) t:changed('A','P','v',10) \
        t:recordingFinished(false,20) return s(t:recent(30))";
    assert_eq!(edits(cancelled), "");
    assert_eq!(
        edits("t:changed('A','P','v',10) return s(t:recent(30))"),
        ""
    );
}

#[test]
fn data_edits_nested() {
    let body = "t:recordingStarted(0) t:recordingStarted(5) t:changed('A','P','v',10) \
        t:recordingFinished(true,20) local mid = s(t:recent(25)) \
        t:recordingFinished(true,30) return mid .. '|' .. s(t:recent(35))";
    assert_eq!(edits(body), "|A.P=v");
}

#[test]
fn data_edits_undo_window() {
    let inside = "t:undoRedo(1000) t:changed('A','P','v',1250) return s(t:recent(1300))";
    assert_eq!(edits(inside), "A.P=v");
    let after = "t:undoRedo(1000) t:changed('A','P','v',1251) return s(t:recent(1300))";
    assert_eq!(edits(after), "");
    // The undo's own changes may arrive just before its signal.
    let before = "t:changed('A','P','v',900) t:undoRedo(1000) return s(t:recent(1100))";
    assert_eq!(edits(before), "A.P=v");
    let too_early = "t:changed('A','P','v',700) t:undoRedo(1000) return s(t:recent(1100))";
    assert_eq!(edits(too_early), "");
}

#[test]
fn data_edits_stale_recording() {
    let body = "t:recordingStarted(0) t:changed('A','P','v',1000) \
        t:recordingFinished(true,11000) return s(t:recent(11000))";
    assert_eq!(edits(body), "");
}

#[test]
fn data_edits_merge() {
    let body = "t:recordingStarted(0) t:changed('A','P','1',0) t:changed('A','P','2',1500) \
        t:recordingFinished(true,1600) \
        t:recordingStarted(5000) t:changed('A','P','3',5000) \
        t:recordingFinished(true,5100) return s(t:recent(6000))";
    assert_eq!(edits(body), "A.P=2,A.P=3");
}

#[test]
fn data_edits_limits() {
    let many = "t:recordingStarted(0) for i = 1, 25 do t:changed('P' .. i,'X','v',i) end \
        t:recordingFinished(true,100) local l = t:recent(200) return #l .. ':' .. l[1].path";
    assert_eq!(edits(many), "20:P6");
    let old = "t:recordingStarted(0) t:changed('A','P','v',0) t:recordingFinished(true,1) \
        return s(t:recent(59999)) .. '|' .. s(t:recent(60000))";
    assert_eq!(edits(old), "A.P=v|");
}

const FAKE_TREE: &str = r#"
local function chain(names)
    local function node(name, parent)
        local n = {Name = name, Parent = parent, children = {}}
        function n.FindFirstChild(self, child)
            for _, c in ipairs(self.children) do
                if c.Name == child then return c end
            end
            return nil
        end
        return n
    end
    local root = node("game", nil)
    local leaf = root
    for _, name in ipairs(names) do
        local child = node(name, leaf)
        table.insert(leaf.children, child)
        leaf = child
    end
    return root, leaf
end
return chain
"#;

#[test]
fn data_paths() {
    let lua = new_lua();
    let join = func(&lua, "Paths", "join");
    let split = func(&lua, "Paths", "split");
    let path_of = func(&lua, "Paths", "pathOf");
    let resolve = func(&lua, "Paths", "resolve");
    let chain: Function = lua.load(FAKE_TREE).eval().unwrap();
    let fixtures = fixture_json("paths.json");

    for case in fixtures["valid"].as_array().unwrap() {
        let names: Vec<String> = case["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap().to_owned())
            .collect();
        let path = str_field(case, "path");
        let joined: String = join.call(names.clone()).unwrap();
        assert_eq!(joined, path);
        let parts: Option<Vec<String>> = split.call(path).unwrap();
        assert_eq!(parts, Some(names.clone()));

        let (root, leaf): (Table, Table) = chain.call(names).unwrap();
        let found: Option<String> = path_of.call((leaf.clone(), root.clone())).unwrap();
        assert_eq!(found.as_deref(), Some(path));
        let resolved: Table = resolve.call((root.clone(), path)).unwrap();
        assert!(resolved.equals(&leaf).unwrap(), "{path}");
        let own: Option<String> = path_of.call((root.clone(), root)).unwrap();
        assert_eq!(own, None);
    }

    for path in fixtures["invalid"].as_array().unwrap() {
        let parts: Option<Vec<String>> = split.call(path.as_str().unwrap()).unwrap();
        assert_eq!(parts, None, "{path}");
    }

    let (root, _): (Table, Table) = chain.call(vec!["Workspace".to_owned()]).unwrap();
    let (_, stranger): (Table, Table) = chain.call(vec!["Workspace".to_owned()]).unwrap();
    let outside: Option<String> = path_of.call((stranger, root)).unwrap();
    assert_eq!(outside, None);
}

#[test]
fn ntfy_signature_and_round_trip() {
    let lua = new_lua();
    let vectors = fixture_json("ntfy_vectors.json");
    let key = str_field(&vectors, "key");
    let topic = str_field(&vectors, "topic");
    let u = num_field(&vectors, "u");
    let t = num_field(&vectors, "t");
    let d = str_field(&vectors, "d");

    // The signature formula matches the golden file (and so the Rust side).
    let sign_path = func(&lua, "Ntfy", "signPath");
    let path: String = sign_path.call(topic).unwrap();
    let canonical = func(&lua, "Auth", "canonical");
    let canon: String = canonical.call(("POST", path, u, t, d)).unwrap();
    let sign = func(&lua, "Auth", "sign");
    let sig: String = sign.call((key, canon)).unwrap();
    assert_eq!(sig, str_field(&vectors, "s"));

    // A state encoded and decoded by the plugin round-trips, and a wrong key
    // or a stale clock is rejected.
    let encode = func(&lua, "Ntfy", "encode");
    let decode = func(&lua, "Ntfy", "decode");
    let state = lua.create_table().unwrap();
    state.set("v", 1).unwrap();
    state.set("userId", 4242.0).unwrap();
    state.set("name", "Ann").unwrap();
    state.set("color", "#e8590c").unwrap();
    state.set("client", "studio").unwrap();
    state.set("selection", lua.create_table().unwrap()).unwrap();
    state.set("edits", lua.create_table().unwrap()).unwrap();
    state.set("seen", 0).unwrap();
    let body: String = encode.call((key, topic, 4242.0, t, state)).unwrap();
    let got: Table = decode.call((key, topic, t, body.clone())).unwrap();
    assert_eq!(got.get::<f64>("userId").unwrap(), 4242.0);
    let bad: Value = decode
        .call(("z".repeat(64), topic, t, body.clone()))
        .unwrap();
    assert!(matches!(bad, Value::Nil));
    let stale: Value = decode.call((key, topic, t + 1_000_000.0, body)).unwrap();
    assert!(matches!(stale, Value::Nil));
}

/// The plugin's `Catchup.summarize` agrees with the golden fixture (and so
/// the Rust `catchup::summarize`), so the two cannot drift.
#[test]
fn data_catchup() {
    let lua = new_lua();
    let fixture = fixture_json("catchup.json");
    let entries = fixture["entries"].as_array().unwrap();
    let arr = lua.create_table().unwrap();
    for (i, entry) in entries.iter().enumerate() {
        let row = lua.create_table().unwrap();
        row.set("userId", num_field(entry, "userId")).unwrap();
        row.set("name", str_field(entry, "name")).unwrap();
        row.set("path", str_field(entry, "path")).unwrap();
        row.set("property", str_field(entry, "property")).unwrap();
        row.set("at", num_field(entry, "at")).unwrap();
        arr.set(i + 1, row).unwrap();
    }

    let summarize = func(&lua, "Catchup", "summarize");
    let summary: Table = summarize.call(arr).unwrap();
    let want = &fixture["summary"];
    assert_eq!(
        summary.get::<f64>("instances").unwrap(),
        num_field(want, "instances")
    );
    assert_eq!(
        summary.get::<f64>("properties").unwrap(),
        num_field(want, "properties")
    );
    assert_eq!(
        summary.get::<f64>("edits").unwrap(),
        num_field(want, "edits")
    );

    let authors: Table = summary.get("authors").unwrap();
    let want_authors = want["authors"].as_array().unwrap();
    assert_eq!(authors.raw_len(), want_authors.len());
    for (i, wanted) in want_authors.iter().enumerate() {
        let got: Table = authors.get(i + 1).unwrap();
        assert_eq!(
            got.get::<f64>("userId").unwrap(),
            num_field(wanted, "userId")
        );
        assert_eq!(
            got.get::<String>("name").unwrap(),
            str_field(wanted, "name")
        );
        assert_eq!(
            got.get::<f64>("instances").unwrap(),
            num_field(wanted, "instances")
        );
        assert_eq!(
            got.get::<f64>("properties").unwrap(),
            num_field(wanted, "properties")
        );
        assert_eq!(got.get::<f64>("edits").unwrap(), num_field(wanted, "edits"));
    }
}

/// Mirrors the Rust `ntfy::fit_encode` test over the same large state: the
/// plugin's `fitEncode` sheds down to a body that fits `MAX_BODY`, and the
/// shed body interoperates with the Rust receiver — and vice versa — so the
/// two shedding paths cannot drift.
#[test]
fn ntfy_fit_sheds_to_a_fitting_interoperable_body() {
    use rbx_presence::ntfy;
    use rbx_presence::schema::{Client, Edit, UserState, SCHEMA_VERSION};

    let lua = new_lua();
    let key = "k".repeat(64);
    let topic = "rbxnp-abc";
    let selection: Vec<String> = (0..100)
        .map(|i| format!("{i}{}", "a".repeat(400)))
        .collect();
    let edits: Vec<Edit> = (0..20)
        .map(|i| Edit {
            path: "Workspace.Part".into(),
            property: "Size".into(),
            value: "1, 2, 3".into(),
            at: i,
        })
        .collect();

    // Luau `fitEncode` over the same state.
    let state = lua.create_table().unwrap();
    state.set("v", 1).unwrap();
    state.set("userId", 42.0).unwrap();
    state.set("name", "Ann").unwrap();
    state.set("color", "#e8590c").unwrap();
    state.set("client", "rbx-native").unwrap();
    let sel = lua.create_table().unwrap();
    for (i, path) in selection.iter().enumerate() {
        sel.set(i + 1, path.clone()).unwrap();
    }
    state.set("selection", sel).unwrap();
    let ed = lua.create_table().unwrap();
    for (i, edit) in edits.iter().enumerate() {
        let row = lua.create_table().unwrap();
        row.set("path", edit.path.clone()).unwrap();
        row.set("property", edit.property.clone()).unwrap();
        row.set("value", edit.value.clone()).unwrap();
        row.set("at", edit.at as f64).unwrap();
        ed.set(i + 1, row).unwrap();
    }
    state.set("edits", ed).unwrap();
    state.set("seen", 0).unwrap();

    let max: f64 = load_module(&lua, "Ntfy").unwrap().get("MAX_BODY").unwrap();
    let fit = func(&lua, "Ntfy", "fitEncode");
    let luau_body: String = fit.call((key.clone(), topic, 42.0, 1000.0, state)).unwrap();
    assert!(
        luau_body.len() as f64 <= max,
        "Luau fitEncode must fit MAX_BODY"
    );
    // The Luau-shed body verifies and decodes under the Rust receiver.
    assert_eq!(
        ntfy::decode(&key, topic, 1000, &luau_body).unwrap().user_id,
        42
    );

    // The Rust side sheds the same inputs to a body that also fits and that
    // the plugin's decoder accepts.
    let rust_state = UserState {
        v: SCHEMA_VERSION,
        user_id: 42,
        name: "Ann".into(),
        color: "#e8590c".into(),
        client: Client::RbxNative,
        camera: None,
        selection,
        script: None,
        edits,
        seen: 0,
    };
    let rust_body = ntfy::fit_encode(&key, topic, 42, 1000, &rust_state).unwrap();
    assert!(rust_body.len() <= ntfy::NTFY_MAX_BODY);
    let decode = func(&lua, "Ntfy", "decode");
    let got: Table = decode.call((key, topic, 1000.0, rust_body)).unwrap();
    assert_eq!(got.get::<f64>("userId").unwrap(), 42.0);
}

#[test]
fn syntax_all() {
    let lua = Lua::new();
    let mut seen = 0;
    for entry in std::fs::read_dir(src_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("luau") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        lua.load(src)
            .set_name(format!("={}", path.display()))
            .into_function()
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        seen += 1;
    }
    assert!(seen > 0, "no .luau files found");
}

#[test]
fn data_changes_group_by_instance() {
    let lua = new_lua();
    let entries: Table = lua
        .load(
            r#"return {
                { userId = 1, name = "Ana", path = "Workspace.Ramp", property = "Size", at = 1000 },
                { userId = 2, name = "Cleo", path = "Workspace.Ramp", property = "Size", at = 3000 },
                { userId = 1, name = "Ana", path = "Workspace.Ramp", property = "Anchored", at = 1500 },
                { userId = 1, name = "Ana", path = "Lighting", property = "ClockTime", at = 2000 },
            }"#,
        )
        .eval()
        .unwrap();
    let from = func(&lua, "Changes", "fromJournal");
    let changes: Table = from.call(entries).unwrap();
    assert_eq!(changes.raw_len(), 2);

    // Newest edit first: the Ramp (3000) before Lighting (2000).
    let ramp: Table = changes.get(1).unwrap();
    assert_eq!(ramp.get::<String>("name").unwrap(), "Ramp");
    assert_eq!(ramp.get::<String>("parent").unwrap(), "Workspace");
    let props: Table = ramp.get("properties").unwrap();
    let first: Table = props.get(1).unwrap();
    assert_eq!(first.get::<String>("property").unwrap(), "Anchored");
    let size: Table = props.get(2).unwrap();
    assert_eq!(size.get::<f64>("edits").unwrap(), 2.0);
    let authors: Table = size.get("authors").unwrap();
    assert_eq!(authors.get::<String>(1).unwrap(), "Cleo");
    assert_eq!(authors.get::<String>(2).unwrap(), "Ana");

    let lighting: Table = changes.get(2).unwrap();
    assert_eq!(lighting.get::<String>("parent").unwrap(), "game");

    let ago = func(&lua, "Changes", "ago");
    let text: String = ago.call((0.0, 240_000.0)).unwrap();
    assert_eq!(text, "4 min ago");
}
