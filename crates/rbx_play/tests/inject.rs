use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_play::inject::{
    inject, preflight, InjectError, Preflight, ProbeConfig, CODEC_NAME, LIVE_NAME,
    MAX_PUBLISH_BYTES, MIRROR_NAME, PROBE_CLIENT, PROBE_CODEC, PROBE_LIVE, PROBE_NAME,
    PROBE_SERVER, REMOTE_NAME,
};

fn config() -> ProbeConfig {
    ProbeConfig {
        base_url: "https://probe.example".to_owned(),
        token: "a".repeat(64),
        session: "session-1".to_owned(),
    }
}

fn data_model_dom() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let root = dom.new_instance("DataModel", "Game", None);
    (dom, root)
}

fn children_matching(dom: &WeakDom, parent: Ref, pred: impl Fn(&Instance) -> bool) -> Vec<Ref> {
    dom.get(parent)
        .map(|p| {
            p.children()
                .iter()
                .copied()
                .filter(|c| dom.get(*c).is_some_and(&pred))
                .collect()
        })
        .unwrap_or_default()
}

fn single_child_of_class(dom: &WeakDom, parent: Ref, class: &str) -> Ref {
    let found = children_matching(dom, parent, |i| i.class() == class);
    assert_eq!(found.len(), 1, "expected one {class} under parent");
    found[0]
}

fn bool_prop(dom: &WeakDom, r: Ref, name: &str) -> Option<bool> {
    match dom.get(r)?.properties().get(name) {
        Some(Variant::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn string_prop(dom: &WeakDom, r: Ref, name: &str) -> Option<String> {
    match dom.get(r)?.properties().get(name) {
        Some(Variant::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn string_attr(dom: &WeakDom, r: Ref, name: &str) -> Option<String> {
    match dom.get(r)?.attributes().remove(name) {
        Some(Variant::String(s)) => Some(s),
        _ => None,
    }
}

#[test]
fn creates_services_when_absent() {
    let (mut dom, root) = data_model_dom();
    inject(&mut dom, &config()).expect("inject");

    single_child_of_class(&dom, root, "HttpService");
    single_child_of_class(&dom, root, "ServerScriptService");
    single_child_of_class(&dom, root, "ReplicatedStorage");
    let starter = single_child_of_class(&dom, root, "StarterPlayer");
    single_child_of_class(&dom, starter, "StarterPlayerScripts");
}

#[test]
fn enables_http_and_loadstring() {
    let (mut dom, root) = data_model_dom();
    inject(&mut dom, &config()).expect("inject");

    let http = single_child_of_class(&dom, root, "HttpService");
    let sss = single_child_of_class(&dom, root, "ServerScriptService");
    assert_eq!(bool_prop(&dom, http, "HttpEnabled"), Some(true));
    assert_eq!(bool_prop(&dom, sss, "LoadStringEnabled"), Some(true));
}

#[test]
fn probe_script_carries_source_and_attributes() {
    let (mut dom, root) = data_model_dom();
    let injected = inject(&mut dom, &config()).expect("inject");

    let sss = single_child_of_class(&dom, root, "ServerScriptService");
    assert_eq!(dom.parent(injected.probe), Some(sss));
    assert_eq!(dom.get(injected.probe).unwrap().class(), "Script");
    assert_eq!(dom.get(injected.probe).unwrap().name(), PROBE_NAME);
    assert_eq!(
        string_prop(&dom, injected.probe, "Source").as_deref(),
        Some(PROBE_SERVER)
    );
    assert_eq!(
        string_attr(&dom, injected.probe, "BASE").as_deref(),
        Some("https://probe.example")
    );
    assert_eq!(
        string_attr(&dom, injected.probe, "TOKEN").as_deref(),
        Some("a".repeat(64).as_str())
    );
    assert_eq!(
        string_attr(&dom, injected.probe, "SESSION").as_deref(),
        Some("session-1")
    );
}

#[test]
fn remote_and_mirror_are_placed_in_their_services() {
    let (mut dom, root) = data_model_dom();
    let injected = inject(&mut dom, &config()).expect("inject");

    let rs = single_child_of_class(&dom, root, "ReplicatedStorage");
    assert_eq!(dom.parent(injected.remote), Some(rs));
    assert_eq!(dom.get(injected.remote).unwrap().class(), "RemoteEvent");
    assert_eq!(dom.get(injected.remote).unwrap().name(), REMOTE_NAME);

    let starter = single_child_of_class(&dom, root, "StarterPlayer");
    let scripts = single_child_of_class(&dom, starter, "StarterPlayerScripts");
    assert_eq!(dom.parent(injected.mirror), Some(scripts));
    assert_eq!(dom.get(injected.mirror).unwrap().class(), "LocalScript");
    assert_eq!(dom.get(injected.mirror).unwrap().name(), MIRROR_NAME);
    assert_eq!(
        string_prop(&dom, injected.mirror, "Source").as_deref(),
        Some(PROBE_CLIENT)
    );

    for (r, name, src) in [
        (injected.codec, CODEC_NAME, PROBE_CODEC),
        (injected.live, LIVE_NAME, PROBE_LIVE),
    ] {
        assert_eq!(dom.parent(r), Some(rs));
        assert_eq!(dom.get(r).unwrap().class(), "ModuleScript");
        assert_eq!(dom.get(r).unwrap().name(), name);
        assert_eq!(string_prop(&dom, r, "Source").as_deref(), Some(src));
    }
}

#[test]
fn existing_services_are_reused() {
    let (mut dom, root) = data_model_dom();
    let existing_sss = dom.new_instance("ServerScriptService", "Custom", Some(root));
    let existing_rs = dom.new_instance("ReplicatedStorage", "Storage", Some(root));

    inject(&mut dom, &config()).expect("inject");

    assert_eq!(
        single_child_of_class(&dom, root, "ServerScriptService"),
        existing_sss
    );
    assert_eq!(
        single_child_of_class(&dom, root, "ReplicatedStorage"),
        existing_rs
    );
}

#[test]
fn injecting_twice_leaves_one_of_each() {
    let (mut dom, root) = data_model_dom();
    inject(&mut dom, &config()).expect("first inject");
    let second = inject(&mut dom, &config()).expect("second inject");

    let sss = single_child_of_class(&dom, root, "ServerScriptService");
    let rs = single_child_of_class(&dom, root, "ReplicatedStorage");
    let starter = single_child_of_class(&dom, root, "StarterPlayer");
    let scripts = single_child_of_class(&dom, starter, "StarterPlayerScripts");

    let probes = children_matching(&dom, sss, |i| i.name() == PROBE_NAME);
    let remotes = children_matching(&dom, rs, |i| i.name() == REMOTE_NAME);
    let mirrors = children_matching(&dom, scripts, |i| i.name() == MIRROR_NAME);
    assert_eq!(probes, vec![second.probe]);
    assert_eq!(remotes, vec![second.remote]);
    assert_eq!(mirrors, vec![second.mirror]);
}

#[test]
fn services_as_roots_get_their_missing_siblings_at_the_root() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let injected = inject(&mut dom, &config()).expect("roots are the services");
    let sss = dom.parent(injected.probe).expect("probe has a parent");
    assert_eq!(dom.get(sss).unwrap().class(), "ServerScriptService");
    assert_eq!(dom.parent(sss), None, "service created as a root");
    assert!(dom.root_refs().contains(&workspace));
}

#[test]
fn a_real_place_file_takes_the_probe() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/tests/TestPlace.rbxl"
    ))
    .expect("TestPlace.rbxl");
    let mut dom = rbx_binary::deserialize(&bytes).expect("parse");
    let roots_before = dom.root_refs().len();
    let injected = inject(&mut dom, &config()).expect("inject into a place file");
    let sss = dom.parent(injected.probe).unwrap();
    assert_eq!(dom.get(sss).unwrap().class(), "ServerScriptService");
    let classes: Vec<&str> = dom
        .root_refs()
        .iter()
        .map(|r| dom.get(*r).unwrap().class())
        .collect();
    for class in [
        "HttpService",
        "ServerScriptService",
        "ReplicatedStorage",
        "StarterPlayer",
    ] {
        assert_eq!(
            classes.iter().filter(|c| **c == class).count(),
            1,
            "{class}"
        );
    }
    assert!(dom.root_refs().len() >= roots_before);
}

#[test]
fn empty_dom_is_an_error() {
    let mut dom = WeakDom::new();
    assert_eq!(inject(&mut dom, &config()), Err(InjectError::NoDataModel));
}

#[test]
fn preflight_is_empty_for_small_dom() {
    let (mut dom, root) = data_model_dom();
    dom.new_instance("Workspace", "Workspace", Some(root));
    assert!(preflight(&dom, 1024).is_empty());
}

#[test]
fn preflight_reports_too_large_first() {
    let (mut dom, root) = data_model_dom();
    dom.new_instance("UnionOperation", "Union", Some(root));
    let result = preflight(&dom, MAX_PUBLISH_BYTES + 1);
    assert_eq!(
        result.first(),
        Some(&Preflight::TooLarge {
            bytes: MAX_PUBLISH_BYTES + 1
        })
    );
}

#[test]
fn preflight_counts_incomplete_classes_sorted() {
    let (mut dom, root) = data_model_dom();
    dom.new_instance("UnionOperation", "A", Some(root));
    dom.new_instance("UnionOperation", "B", Some(root));
    dom.new_instance("SurfaceAppearance", "S", Some(root));

    assert_eq!(
        preflight(&dom, 1024),
        vec![
            Preflight::Incomplete {
                class: "SurfaceAppearance".to_owned(),
                count: 1,
            },
            Preflight::Incomplete {
                class: "UnionOperation".to_owned(),
                count: 2,
            },
        ]
    );
}
