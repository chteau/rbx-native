use crate::{
    manifest_text, parse_manifest, Capability, Decision, Level, PluginKind, PluginProfile, Policy,
    Request, Store,
};

fn native(requested: Vec<Capability>, hosts: &[&str]) -> PluginProfile {
    PluginProfile {
        name: "Tool".into(),
        kind: PluginKind::Luau,
        requested,
        hosts: hosts.iter().map(|h| h.to_string()).collect(),
    }
}

#[test]
fn manifest_strings_round_trip() {
    for text in [
        "dom.read",
        "dom.write",
        "scripts",
        "selection",
        "debugger",
        "http:*",
        "http:example.com",
    ] {
        let request = parse_manifest(text).unwrap();
        assert_eq!(manifest_text(&request).as_deref(), Some(text));
    }
    assert_eq!(parse_manifest("root"), None);
    assert_eq!(parse_manifest("http:"), None);
    assert_eq!(
        parse_manifest("debugger"),
        Some(Request::Capability(Capability::RunCode))
    );
}

#[test]
fn mcp_starts_balanced_and_turns_custom_when_changed() {
    let mut policy = Policy::default();
    assert_eq!(policy.mcp_level(), Some(Level::Balanced));
    assert!(policy.mcp_allows(Capability::EditScripts));
    assert!(!policy.mcp_allows(Capability::CloudUpload));

    policy.set_mcp(Capability::CloudUpload, true);
    assert!(policy.mcp_allows(Capability::CloudUpload));
    assert_eq!(policy.mcp_level(), None);

    policy.set_mcp(Capability::CloudUpload, false);
    assert!(policy.mcp.overrides.is_empty());
    assert_eq!(policy.mcp_level(), Some(Level::Balanced));

    policy.set_mcp_level(Level::Safe);
    assert!(policy.mcp_allows(Capability::ReadPlace));
    assert!(!policy.mcp_allows(Capability::EditPlace));
    // Changing Safe until it matches Full access reads as Full access.
    for capability in Capability::ALL {
        policy.set_mcp(capability, true);
    }
    assert_eq!(policy.mcp_level(), Some(Level::Full));
}

#[test]
fn roblox_plugins_ask_for_scripts_and_hosts_only() {
    let policy = Policy::default();
    let profile = PluginProfile::roblox("F3X");
    assert_eq!(policy.decide(&profile, Capability::EditPlace), Decision::Allow);
    assert_eq!(policy.decide(&profile, Capability::EditScripts), Decision::Ask);
    assert_eq!(policy.host(&profile, "f3xteam.com"), Decision::Ask);
}

#[test]
fn native_plugins_get_what_their_manifest_asks_for() {
    let policy = Policy::default();
    let profile = native(vec![Capability::Selection], &["api.example.com"]);
    assert!(policy.allows(&profile, Capability::Selection));
    assert!(!policy.allows(&profile, Capability::EditPlace));
    assert_eq!(policy.host(&profile, "api.example.com"), Decision::Allow);
    // Not listed: refused without a prompt.
    assert_eq!(policy.host(&profile, "other.com"), Decision::Deny);
    let any = native(vec![Capability::Network], &[]);
    assert_eq!(policy.host(&any, "other.com"), Decision::Allow);
}

#[test]
fn stored_answers_override_defaults_and_network_covers_hosts() {
    let mut policy = Policy::default();
    let profile = PluginProfile::roblox("Tool");
    policy.set_host("Tool", "a.com", Some(true));
    policy.set_host("Tool", "b.com", Some(false));
    assert_eq!(policy.host(&profile, "a.com"), Decision::Allow);
    assert_eq!(policy.host(&profile, "b.com"), Decision::Deny);
    assert_eq!(policy.hosts(&profile), vec!["a.com", "b.com"]);

    policy.set("Tool", Capability::Network, Some(Decision::Deny));
    assert_eq!(policy.host(&profile, "a.com"), Decision::Deny);
    policy.set("Tool", Capability::Network, Some(Decision::Allow));
    assert_eq!(policy.host(&profile, "b.com"), Decision::Allow);

    // Back to defaults leaves no entry behind.
    policy.set("Tool", Capability::Network, None);
    policy.set_host("Tool", "a.com", None);
    policy.set_host("Tool", "b.com", None);
    assert!(policy.plugins.is_empty());
}

#[test]
fn store_writes_only_on_change_and_reads_back() {
    let dir = std::env::temp_dir().join(format!("rbx-permissions-{}", std::process::id()));
    let path = dir.join("permissions.json");
    let _ = std::fs::remove_dir_all(&dir);

    let mut store = Store::open(path.clone());
    store.change(|_| {}).unwrap();
    assert!(!path.exists());
    store
        .change(|p| p.set("Tool", Capability::EditScripts, Some(Decision::Allow)))
        .unwrap();
    let reopened = Store::open(path.clone());
    assert_eq!(
        reopened.policy().stored("Tool", Capability::EditScripts),
        Some(Decision::Allow)
    );
    assert_eq!(reopened.policy(), store.policy());

    std::fs::write(&path, "not json").unwrap();
    assert_eq!(Store::open(path).policy(), &Policy::default());
    let _ = std::fs::remove_dir_all(&dir);
}
