//! Offline, the catalog calls answer from what the run's cache kept, in
//! the shapes a live server gives (the WebMarket and AvatarApi goldens).
//! One test: the offline switch is the whole process's.

use rbx_dom::Variant;
use rbx_sim::{conformance, Session};

const SCRIPT: &str = r#"
local aes = game:GetService("AvatarEditorService")
local mps = game:GetService("MarketplaceService")
local assets = game:GetService("AssetService")
print("zero", pcall(assets.GetBundleDetailsAsync, assets, 0))
local info = mps:GetProductInfo(777, Enum.InfoType.Bundle)
print("info", info.Name, info.BundleType, #info.Items, info.Items[1].AssetType, info.UserBasePriceInRobux)
local pages = aes:GetBundlesByAssetIdAsync(555)
local page = pages:GetCurrentPage()
print("bundles", pages.ClassName, pages.IsFinished, #page, page[1].BundleType, page[1].Owned, page[1].Taxonomy)
local item = aes:GetItemDetailsAsync(777, Enum.AvatarItemType.Bundle)
print("item", item.BundleType, item.Owned, item.ItemCreatedUtc)
print("missing", pcall(aes.GetItemDetailsAsync, aes, 778, Enum.AvatarItemType.Asset))
"#;

#[test]
fn catalog_calls_answer_offline_from_the_kept_answers() {
    let cache = std::env::temp_dir().join(format!("rbx_sim_web_offline_{}", std::process::id()));
    for (url, answer) in [
        (
            "https://catalog.roblox.com/v1/bundles/777/details",
            r#"{"id":777,"name":"Kept","description":"","bundleType":"BodyParts","items":[{"id":5,"name":"Arm","type":"Asset","assetType":29}],"product":{"priceInRobux":50}}"#,
        ),
        (
            "https://catalog.roblox.com/v1/assets/555/bundles?limit=100",
            r#"{"data":[{"id":777}]}"#,
        ),
        (
            "https://catalog.roblox.com/v1/catalog/items/777/details?itemType=Bundle",
            r#"{"id":777,"name":"Kept","bundleType":1,"itemType":"Bundle","owned":false,"itemCreatedUtc":"2018-08-14","taxonomy":[]}"#,
        ),
    ] {
        let file = rbx_lua::server::web_cache_file(&cache, url, "GET", None);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, answer).unwrap();
    }
    let mut dom = conformance::bare_place();
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .unwrap();
    let script = dom.new_instance("Script", "Server", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(SCRIPT.into()));
    dom.take_changes();

    rbx_net::set_forced(true);
    let mut session = Session::new(dom).unwrap();
    session.set_services(rbx_lua::server::Services {
        cache_dir: Some(cache.clone()),
        ..Default::default()
    });
    session.start();
    let mut lines = Vec::new();
    // The requests run on worker threads: wait on what's printed, not on a
    // number of frames.
    for _ in 0..20_000 {
        session.step();
        for (_, output) in session.take_output() {
            lines.extend(output.lines().to_vec());
        }
        if lines.iter().any(|l| l.starts_with("missing")) {
            break;
        }
    }
    rbx_net::set_forced(false);
    let _ = std::fs::remove_dir_all(&cache);

    let has = |text: &str| lines.iter().any(|l| l == text);
    assert!(has("zero false HTTP 400"), "{lines:#?}");
    assert!(has("info Kept BodyParts 1 LeftArm 50"), "{lines:#?}");
    assert!(
        has("bundles CatalogPages true 1 BodyParts nil nil"),
        "{lines:#?}"
    );
    assert!(has("item BodyParts false nil"), "{lines:#?}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("missing false") && l.contains("offline")),
        "{lines:#?}"
    );
}
