//! `require(assetId)`: the asset is a model, and the `ModuleScript` named
//! `MainModule` in it is what's required, copied into the game with no
//! parent, as a live server loads it (the require page on the creator
//! docs). A require yields while the asset loads: ours blocks instead.

use rbx_assets::AssetCache;
use rbx_dom::{Ref, WeakDom};
use rbx_lua::server::{Arg, CallReply};

use crate::physics::Shared;

/// The engine call `require` makes for an asset id.
pub(crate) const CLASS: &str = "require";
pub(crate) const METHOD: &str = "LoadModule";

pub(crate) fn load(
    shared: &mut Shared,
    dom: &mut WeakDom,
    args: &[Arg],
) -> Result<CallReply, String> {
    let Some(Arg::Number(id)) = args.first() else {
        return Err("Attempted to call require with invalid argument(s).".into());
    };
    let id = *id as u64;
    if let Some(&module) = shared.modules.get(&id) {
        if dom.get(module).is_some() {
            return Ok(CallReply::value(Arg::Instance(module)));
        }
    }
    let failed = || format!("Unable to load module asset {id}");
    let cached = crate::replay::cached(shared.sounds.fetcher.as_ref(), id);
    let bytes = match cached {
        Some(bytes) => bytes,
        None => {
            let fetcher = shared.sounds.fetcher.clone().ok_or_else(failed)?;
            let bytes = fetcher.fetch_id(id).map_err(|_| failed())?;
            if let Ok(cache) = AssetCache::new(None) {
                let _ = cache.put_id(id, &bytes);
            }
            bytes
        }
    };
    let model = crate::load::parse_place(&bytes).map_err(|_| failed())?;
    let main = main_module(&model).ok_or_else(failed)?;
    let module = crate::players::graft(dom, &model, main, None);
    shared.modules.insert(id, module);
    Ok(CallReply::value(Arg::Instance(module)))
}

/// The first `ModuleScript` named `MainModule` in the model, top level
/// first.
fn main_module(model: &WeakDom) -> Option<Ref> {
    let mut queue: std::collections::VecDeque<Ref> = model.root_refs().iter().copied().collect();
    while let Some(at) = queue.pop_front() {
        let instance = model.get(at)?;
        if instance.class() == "ModuleScript" && instance.name() == "MainModule" {
            return Some(at);
        }
        queue.extend(instance.children().iter().copied());
    }
    None
}
