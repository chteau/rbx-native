//! An offline Local Run reads assets from the shared cache an offline pack
//! fills: `ContentProvider:PreloadAsync`, `InsertService:LoadAsset` and
//! sounds find what's cached, and what isn't fails with the client's
//! warnings. The network is a fetcher that refuses everything.
//!
//! One test in its own binary: it points `XDG_CACHE_HOME` at a scratch
//! cache for the whole process.

use std::sync::Arc;

use rbx_assets::{AssetCache, FetchError};
use rbx_dom::{Content, Variant, WeakDom};
use rbx_sim::{conformance, Session};

struct Offline;

impl rbx_assets::AssetFetcher for Offline {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Other {
            id,
            message: "offline".into(),
        })
    }
}

fn model() -> Vec<u8> {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    dom.new_instance("Part", "Packed", Some(model));
    rbx_binary::serialize(&dom).unwrap()
}

#[test]
fn an_offline_run_loads_what_the_pack_cached_and_warns_for_the_rest() {
    let dir = std::env::temp_dir().join(format!("rbx_sim_offline_assets_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("XDG_CACHE_HOME", &dir);
    let cache = AssetCache::new(None).unwrap();
    cache.put_id(111, &model()).unwrap();
    cache.put_id(222, b"\x89PNG image").unwrap();

    let mut dom = conformance::bare_place();
    let workspace = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
        .unwrap();
    let decal = dom.new_instance("Decal", "Decal", Some(workspace));
    let _ = dom.set_property(
        decal,
        "Texture",
        Variant::Content(Content::Uri("rbxassetid://222".into())),
    );
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .unwrap();
    let script = dom.new_instance("Script", "Main", Some(sss));
    let _ = dom.set_property(
        script,
        "Source",
        Variant::String(
            "local cp = game:GetService('ContentProvider') \
             cp:PreloadAsync({ 'rbxassetid://222', 'rbxassetid://333', 'rbxasset://textures/x.png' }, \
               function(c, s) print('cb', c, s.Name) end) \
             cp:PreloadAsync({ workspace.Decal }, function(c, s) print('instance', c, s.Name) end) \
             print('status', cp:GetAssetFetchStatus('rbxassetid://333').Name, cp.RequestQueueSize) \
             local insert = game:GetService('InsertService') \
             print('loaded', insert:LoadAsset(111):FindFirstChild('Packed', true).Name) \
             print('missing', (pcall(insert.LoadAsset, insert, 444))) \
             local s = Instance.new('Sound') s.SoundId = 'rbxassetid://555' s.Parent = workspace s:Play()"
                .into(),
        ),
    );
    dom.take_changes();

    let mut session = Session::new(dom).unwrap();
    session.set_fetcher(Arc::new(Offline));
    session.set_services(rbx_lua::server::Services::default());
    session.start();
    let mut lines = Vec::new();
    for _ in 0..90 {
        session.step();
        for (_, output) in session.take_output() {
            lines.extend(output.lines().iter().cloned());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    for line in [
        "cb rbxassetid://222 Success",
        "cb rbxassetid://333 Failure",
        "cb rbxasset://textures/x.png Success",
        "ContentProvider:PreloadAsync() failed for rbxassetid://333",
        "instance rbxassetid://222 Success",
        "status Failure 0",
        "loaded Packed",
        "missing false",
        "Failed to load sound rbxassetid://555: Unable to download sound data",
    ] {
        assert!(lines.iter().any(|l| l == line), "{line}: {lines:#?}");
    }
}
