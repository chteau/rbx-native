use rbx_dom::Instance;

use super::*;

fn attrs(entries: &[(String, Variant)]) -> BTreeMap<String, Variant> {
    entries.iter().cloned().collect()
}

#[test]
fn attributes_within_the_limits_all_replicate() {
    let fifty = "x".repeat(50);
    let map = attrs(&[
        (fifty.clone(), Variant::String(fifty)),
        ("Health".into(), Variant::Float64(1.0)),
    ]);

    assert!(unreplicated(&map).is_empty());
}

#[test]
fn a_long_name_or_string_value_is_flagged_but_a_long_number_is_not() {
    let long = "x".repeat(51);
    let map = attrs(&[
        (long.clone(), Variant::Bool(true)),
        ("Label".into(), Variant::String(long)),
        ("Fine".into(), Variant::String("short".into())),
    ]);

    let flagged = unreplicated(&map);
    assert_eq!(flagged.len(), 2);
    assert!(flagged["Label"].starts_with("Value over 50"));
    assert!(flagged.keys().any(|name| name.len() == 51));
}

#[test]
fn only_the_first_64_attributes_replicate() {
    let map = attrs(
        &(0..66)
            .map(|i| (format!("a{i:02}"), Variant::Bool(true)))
            .collect::<Vec<_>>(),
    );

    let flagged = unreplicated(&map);
    assert_eq!(flagged.len(), 2);
    assert!(flagged.contains_key("a64") && flagged.contains_key("a65"));
}

#[test]
fn only_a_server_authority_workspace_counts() {
    let place = |mode: Option<u32>| {
        let mut workspace = Instance::new(rbx_dom::Ref::new(1), "Workspace", "Workspace");
        if let Some(mode) = mode {
            workspace
                .properties_mut()
                .insert("AuthorityMode".into(), Variant::Enum(mode));
        }
        let mut dom = WeakDom::new();
        dom.insert(workspace);
        dom
    };

    assert!(!is_server_authority(&place(None)));
    assert!(!is_server_authority(&place(Some(1))));
    assert!(is_server_authority(&place(Some(0))));
}
