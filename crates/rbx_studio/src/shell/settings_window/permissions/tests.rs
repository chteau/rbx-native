use rbx_permissions::{Decision, PluginKind, PluginProfile, Policy};

use super::{shown, summary, Capability};

#[test]
fn every_capability_glyph_is_in_the_catalogue() {
    for capability in Capability::ALL {
        assert!(
            crate::launcher::ui::icon_exists(capability.glyph()),
            "{capability:?}"
        );
    }
    for glyph in [
        "bot",
        "puzzle",
        "chevron-down",
        "chevron-right",
        "x",
        "shield-check",
    ] {
        assert!(crate::launcher::ui::icon_exists(glyph), "{glyph}");
    }
}

#[test]
fn a_native_plugins_hosts_never_count_as_asking() {
    let policy = Policy::default();
    let mut profile = PluginProfile {
        name: "Tint".into(),
        kind: PluginKind::Luau,
        ..Default::default()
    };
    assert_eq!(
        shown(&policy, &profile, Capability::Network),
        Decision::Deny
    );
    profile.hosts.push("api.example.com".into());
    assert_eq!(
        shown(&policy, &profile, Capability::Network),
        Decision::Allow
    );
    let roblox = PluginProfile::roblox("Helper");
    assert_eq!(shown(&policy, &roblox, Capability::Network), Decision::Ask);
}

#[test]
fn summaries_name_asks_only_when_there_are_some() {
    assert_eq!(summary(3, 0, 2), "3 allowed, 2 denied");
    assert_eq!(summary(3, 1, 1), "3 allowed, 1 asks, 1 denied");
    assert_eq!(summary(1, 2, 0), "1 allowed, 2 ask, 0 denied");
}
