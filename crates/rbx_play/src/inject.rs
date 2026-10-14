//! Adds the probe to a copy of the place before it is published. Works on
//! the in-memory DOM only; the user's file is never touched.

use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};

/// Server probe source (`Script` in ServerScriptService).
pub const PROBE_SERVER: &str = include_str!("probe/server.luau");
/// Client mirror source (`LocalScript` in StarterPlayer.StarterPlayerScripts).
pub const PROBE_CLIENT: &str = include_str!("probe/client.luau");

/// The value codec both sides `require` (ModuleScript in ReplicatedStorage).
pub const PROBE_CODEC: &str = include_str!("probe/codec.luau");
/// The live-tree module both sides `require` (ModuleScript in ReplicatedStorage).
pub const PROBE_LIVE: &str = include_str!("probe/live.luau");
/// The debugger module both sides `require` (ModuleScript in ReplicatedStorage).
pub const PROBE_DEBUG: &str = include_str!("probe/debug.luau");
pub const DEBUG_NAME: &str = "__rbxnative_debug";
pub const CODEC_NAME: &str = "__rbxnative_codec";
pub const LIVE_NAME: &str = "__rbxnative_live";

/// Names of everything `inject` adds, so a re-inject can find and replace them.
pub const PROBE_NAME: &str = "__rbxnative_probe";
pub const MIRROR_NAME: &str = "__rbxnative_mirror";
pub const REMOTE_NAME: &str = "__rbxnative_remote";

/// Roblox's limit for a place published through Open Cloud.
pub const MAX_PUBLISH_BYTES: usize = 10 * 1024 * 1024;

/// What the probe needs to call home; stored as attributes on the probe Script.
#[derive(Debug, Clone)]
pub struct ProbeConfig {
    /// Public tunnel URL, no trailing slash. Attribute `BASE`.
    pub base_url: String,
    /// Attribute `TOKEN`.
    pub token: String,
    /// Attribute `SESSION`; also sent as `launchData`.
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injected {
    pub probe: Ref,
    pub mirror: Ref,
    pub remote: Ref,
    pub codec: Ref,
    pub live: Ref,
    pub debug: Ref,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectError {
    /// The DOM holds no instances at all, so it is not a place.
    NoDataModel,
}

/// - Services are the children of a `DataModel` root when there is one, and
///   otherwise the roots themselves (how a `.rbxl` loads: its top-level
///   instances are the services). Finds or creates (by class) the services
///   `HttpService`, `ServerScriptService`, `ReplicatedStorage`, `StarterPlayer`
///   and `StarterPlayer > StarterPlayerScripts`.
/// - Sets `HttpService.HttpEnabled` and `ServerScriptService.LoadStringEnabled`
///   to `Variant::Bool(true)`.
/// - Removes any earlier instances named `PROBE_NAME`/`MIRROR_NAME`/`REMOTE_NAME`
///   in those parents, then adds: `Script` `PROBE_NAME` (Source = `PROBE_SERVER`,
///   attributes BASE/TOKEN/SESSION as strings), `RemoteEvent` `REMOTE_NAME` in
///   ReplicatedStorage, `LocalScript` `MIRROR_NAME` (Source = `PROBE_CLIENT`).
pub fn inject(dom: &mut WeakDom, config: &ProbeConfig) -> Result<Injected, InjectError> {
    if dom.root_refs().is_empty() {
        return Err(InjectError::NoDataModel);
    }
    let root = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "DataModel"));

    let http = find_or_create(dom, root, "HttpService");
    let sss = find_or_create(dom, root, "ServerScriptService");
    let rs = find_or_create(dom, root, "ReplicatedStorage");
    let starter = find_or_create(dom, root, "StarterPlayer");
    let scripts = find_or_create(dom, Some(starter), "StarterPlayerScripts");

    set_prop(dom, http, "HttpEnabled", Variant::Bool(true));
    set_prop(dom, sss, "LoadStringEnabled", Variant::Bool(true));

    remove_named(dom, sss, PROBE_NAME);
    remove_named(dom, scripts, MIRROR_NAME);
    remove_named(dom, rs, REMOTE_NAME);
    remove_named(dom, rs, CODEC_NAME);
    remove_named(dom, rs, LIVE_NAME);
    remove_named(dom, rs, DEBUG_NAME);

    let codec = dom.new_instance("ModuleScript", CODEC_NAME, Some(rs));
    set_prop(
        dom,
        codec,
        "Source",
        Variant::String(PROBE_CODEC.to_owned()),
    );
    let live = dom.new_instance("ModuleScript", LIVE_NAME, Some(rs));
    set_prop(dom, live, "Source", Variant::String(PROBE_LIVE.to_owned()));
    let debug = dom.new_instance("ModuleScript", DEBUG_NAME, Some(rs));
    set_prop(
        dom,
        debug,
        "Source",
        Variant::String(PROBE_DEBUG.to_owned()),
    );

    let probe = dom.new_instance("Script", PROBE_NAME, Some(sss));
    set_prop(
        dom,
        probe,
        "Source",
        Variant::String(PROBE_SERVER.to_owned()),
    );
    let mut attrs = BTreeMap::new();
    attrs.insert("BASE".to_owned(), Variant::String(config.base_url.clone()));
    attrs.insert("TOKEN".to_owned(), Variant::String(config.token.clone()));
    attrs.insert(
        "SESSION".to_owned(),
        Variant::String(config.session.clone()),
    );
    if let Some(raw) = rbx_dom::attributes::encode(&attrs, |_| None) {
        set_prop(
            dom,
            probe,
            "AttributesSerialize",
            Variant::Unknown { type_id: 0x01, raw },
        );
    }

    let remote = dom.new_instance("RemoteEvent", REMOTE_NAME, Some(rs));
    let mirror = dom.new_instance("LocalScript", MIRROR_NAME, Some(scripts));
    set_prop(
        dom,
        mirror,
        "Source",
        Variant::String(PROBE_CLIENT.to_owned()),
    );

    Ok(Injected {
        probe,
        mirror,
        remote,
        codec,
        live,
        debug,
    })
}

/// `parent: None` means the DOM's roots.
fn find_or_create(dom: &mut WeakDom, parent: Option<Ref>, class: &str) -> Ref {
    let siblings = match parent {
        Some(p) => dom.get(p).map(|i| i.children()).unwrap_or_default(),
        None => dom.root_refs(),
    };
    let found = siblings
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.class() == class));
    found.unwrap_or_else(|| dom.new_instance(class, class, parent))
}

fn set_prop(dom: &mut WeakDom, r: Ref, name: &str, value: Variant) {
    // The ref was just found or created, so the property set cannot fail.
    let _ = dom.set_property(r, name, value);
}

fn remove_named(dom: &mut WeakDom, parent: Ref, name: &str) {
    let stale: Vec<Ref> = dom
        .get(parent)
        .map(|p| {
            p.children()
                .iter()
                .copied()
                .filter(|c| dom.get(*c).is_some_and(|i| i.name() == name))
                .collect()
        })
        .unwrap_or_default();
    for r in stale {
        dom.remove(r);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preflight {
    /// Serialized place is over `MAX_PUBLISH_BYTES`; publishing will fail.
    TooLarge { bytes: usize },
    /// Instances of a class the publish API stores incompletely
    /// (`UnionOperation`, `NegateOperation`, `IntersectOperation`,
    /// `SurfaceAppearance`), with how many there are.
    Incomplete { class: String, count: usize },
}

/// Checks to run before publishing; empty means fine. `serialized_len` is
/// the byte length of the `.rbxl` that will be uploaded. Sorted: `TooLarge`
/// first, then `Incomplete` by class name.
pub fn preflight(dom: &WeakDom, serialized_len: usize) -> Vec<Preflight> {
    let mut out = Vec::new();
    if serialized_len > MAX_PUBLISH_BYTES {
        out.push(Preflight::TooLarge {
            bytes: serialized_len,
        });
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let Some(inst) = dom.get(r) else { continue };
        if INCOMPLETE_CLASSES.contains(&inst.class()) {
            *counts.entry(inst.class().to_owned()).or_insert(0) += 1;
        }
        stack.extend_from_slice(inst.children());
    }
    out.extend(
        counts
            .into_iter()
            .map(|(class, count)| Preflight::Incomplete { class, count }),
    );
    out
}

const INCOMPLETE_CLASSES: [&str; 4] = [
    "UnionOperation",
    "NegateOperation",
    "IntersectOperation",
    "SurfaceAppearance",
];
