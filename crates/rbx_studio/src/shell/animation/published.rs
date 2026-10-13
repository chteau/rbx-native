//! Published animations played by `AnimationId`. assetdelivery serves them as
//! ordinary `.rbxm` models, so they are read like any other model. Each play
//! downloads afresh, since an owner can overwrite an animation under the same
//! id; the shared asset cache keeps the latest copy for when the download
//! fails.

use gpui_kit::Context;
use rbx_animation::import::asset::{clip_from_model, content_id};
use rbx_animation::Clip;
use rbx_assets::AssetCache;
use rbx_dom::{Content, Ref, Variant, WeakDom};

use super::Mode;
use crate::anim_editor::place;
use crate::shell::chrome::Document;
use crate::shell::Shell;

/// The clip for a published animation as Roblox serves it now. A download
/// that decodes replaces the cache's copy when it differs from it; one that
/// fails (offline, refused, or not an animation) falls back to that copy.
pub(super) fn fetch_clip_with(
    cache: Option<&AssetCache>,
    id: u64,
    download: impl FnOnce(u64) -> Result<Vec<u8>, String>,
) -> Result<Clip, String> {
    let name = id.to_string();
    let cached = cache.and_then(|c| c.get_id(id));
    let fresh = download(id).and_then(|bytes| {
        let clip = clip_from_model(&bytes, &name).map_err(|e| e.to_string())?;
        Ok((bytes, clip))
    });
    match fresh {
        Ok((bytes, clip)) => {
            if let Some(cache) = cache.filter(|_| cached.as_deref() != Some(&bytes[..])) {
                let _ = cache.put_id(id, &bytes);
            }
            Ok(clip)
        }
        Err(e) => cached
            .and_then(|bytes| clip_from_model(&bytes, &name).ok())
            .ok_or_else(|| format!("Animation {id}: {e}")),
    }
}

/// Blocking: downloads (anonymously, then through the stored Open Cloud key
/// if Roblox refuses), falling back to the shared cache.
pub(super) fn fetch_clip(id: u64) -> Result<Clip, String> {
    let cache = AssetCache::new(None).ok();
    fetch_clip_with(cache.as_ref(), id, |id| {
        let client = rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config());
        client
            .asset(id)
            .map(|content| content.bytes)
            .map_err(|e| e.to_string())
    })
}

/// The asset id and the rig to play it on, if `target` is an `Animation`
/// naming a published id. The rig is the one the animation sits in, else the
/// one the editor is bound to.
pub(in crate::shell) fn playable(
    dom: &WeakDom,
    target: Ref,
    bound: Option<Ref>,
) -> Option<(u64, Ref)> {
    let instance = dom.get(target)?;
    if instance.class() != "Animation" {
        return None;
    }
    let id = match instance.properties().get("AnimationId")? {
        Variant::Content(Content::Uri(text)) | Variant::String(text) => content_id(text)?,
        _ => return None,
    };
    let rig = place::rig_of(dom, target).or(bound)?;
    Some((id, rig))
}

impl Shell {
    /// Opens the animation `target` names in the Animation Editor on its rig
    /// and plays it from the start.
    pub(in crate::shell) fn play_published_animation(
        &mut self,
        target: Ref,
        cx: &mut Context<Self>,
    ) {
        let Some((id, rig)) = playable(&self.dom, target, self.anim_bound_model()) else {
            self.anim
                .warn("This animation has no published AnimationId or no rig to play on");
            cx.notify();
            return;
        };
        self.set_document(Document::Animation, cx);
        self.anim.mode = Mode::DopeSheet;
        if self.anim_bound_model() != Some(rig) {
            self.bind_rig(rig, cx);
        }
        self.anim.say(format!("Downloading animation {id}…"));
        cx.notify();
        cx.spawn(async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { fetch_clip(id) })
                .await;
            let _ = shell.update(cx, |shell, cx| shell.start_published(result, cx));
        })
        .detach();
    }

    fn start_published(&mut self, result: Result<Clip, String>, cx: &mut Context<Self>) {
        let ok = result.is_ok();
        self.finish_import(result, cx);
        if !ok {
            return;
        }
        self.anim.editor.time = 0.0;
        if !self.anim.editor.playing {
            self.anim.editor.toggle_play();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_animation::clip::PoseKey;
    use rbx_animation::Frame;
    use std::cell::Cell;
    use std::path::PathBuf;

    fn model_bytes() -> Vec<u8> {
        let mut clip = Clip::default();
        clip.set_pose(
            0.5,
            "RightUpperArm",
            PoseKey::new(Frame::angles(0., 0., 1.)),
        );
        let mut dom = WeakDom::new();
        rbx_animation::sequence::write(&mut dom, None, &clip, None);
        rbx_binary::serialize(&dom).unwrap()
    }

    fn temp_cache(tag: &str) -> (AssetCache, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rbx-published-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (AssetCache::new(Some(dir.clone())).unwrap(), dir)
    }

    fn other_model_bytes() -> Vec<u8> {
        let mut clip = Clip::default();
        clip.set_pose(0.25, "Head", PoseKey::new(Frame::angles(0., 1., 0.)));
        clip.set_pose(0.75, "Head", PoseKey::new(Frame::angles(0., -1., 0.)));
        let mut dom = WeakDom::new();
        rbx_animation::sequence::write(&mut dom, None, &clip, None);
        rbx_binary::serialize(&dom).unwrap()
    }

    #[test]
    fn an_overwritten_animation_replaces_the_cached_copy() {
        let (cache, dir) = temp_cache("overwritten");
        cache.put_id(7, &model_bytes()).unwrap();
        let now = other_model_bytes();
        let clip = fetch_clip_with(Some(&cache), 7, |_| Ok(now.clone())).unwrap();
        assert_eq!(clip.keyframes.len(), 2);
        assert_eq!(cache.get_id(7), Some(now));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unchanged_animation_still_downloads() {
        let (cache, dir) = temp_cache("unchanged");
        let bytes = model_bytes();
        cache.put_id(8, &bytes).unwrap();
        let calls = Cell::new(0);
        fetch_clip_with(Some(&cache), 8, |_| {
            calls.set(calls.get() + 1);
            Ok(bytes.clone())
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(cache.get_id(8), Some(bytes));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_download_plays_the_cached_copy() {
        let (cache, dir) = temp_cache("offline");
        cache.put_id(9, &model_bytes()).unwrap();
        let clip = fetch_clip_with(Some(&cache), 9, |_| Err("offline".into())).unwrap();
        assert_eq!(clip.keyframes.len(), 1);
        // A download that is not an animation does not displace it either.
        let clip = fetch_clip_with(Some(&cache), 9, |_| Ok(b"junk".to_vec())).unwrap();
        assert_eq!(clip.keyframes.len(), 1);
        assert_eq!(cache.get_id(9), Some(model_bytes()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_first_download_is_cached() {
        let (cache, dir) = temp_cache("first");
        let good = model_bytes();
        fetch_clip_with(Some(&cache), 10, |_| Ok(good.clone())).unwrap();
        assert_eq!(cache.get_id(10), Some(good));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_to_play_says_why() {
        let (cache, dir) = temp_cache("nothing");
        cache.put_id(11, b"junk").unwrap();
        let err = fetch_clip_with(Some(&cache), 11, |_| Err("refused".into())).unwrap_err();
        assert_eq!(err, "Animation 11: refused");
        let err = fetch_clip_with(None, 12, |_| Ok(b"junk".to_vec())).unwrap_err();
        assert!(err.starts_with("Animation 12: "));
        assert_eq!(cache.get_id(12), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn rig(dom: &mut WeakDom) -> Ref {
        let model = dom.new_instance("Model", "Rig", None);
        let a = dom.new_instance("Part", "Torso", Some(model));
        let b = dom.new_instance("Part", "Arm", Some(model));
        let motor = dom.new_instance("Motor6D", "Shoulder", Some(a));
        let _ = dom.set_property(motor, "Part0", Variant::Ref(a));
        let _ = dom.set_property(motor, "Part1", Variant::Ref(b));
        model
    }

    fn animation(dom: &mut WeakDom, parent: Option<Ref>, class: &str, prop: &str) -> Ref {
        let r = dom.new_instance(class, "Walk", parent);
        let _ = dom.set_property(
            r,
            prop,
            Variant::Content(Content::Uri("rbxassetid://507777826".into())),
        );
        r
    }

    #[test]
    fn animation_under_a_rig_is_playable() {
        let mut dom = WeakDom::new();
        let model = rig(&mut dom);
        let anim = animation(&mut dom, Some(model), "Animation", "AnimationId");
        assert_eq!(playable(&dom, anim, None), Some((507777826, model)));
    }

    #[test]
    fn other_classes_and_properties_are_not_playable() {
        let mut dom = WeakDom::new();
        let model = rig(&mut dom);
        let not_anim = animation(&mut dom, Some(model), "Folder", "AnimationId");
        let wrong_prop = animation(&mut dom, Some(model), "Animation", "Other");
        assert_eq!(playable(&dom, not_anim, None), None);
        assert_eq!(playable(&dom, wrong_prop, None), None);
    }

    #[test]
    fn no_rig_and_no_bound_is_not_playable() {
        let mut dom = WeakDom::new();
        let anim = animation(&mut dom, None, "Animation", "AnimationId");
        assert_eq!(playable(&dom, anim, None), None);
    }

    #[test]
    fn the_bound_rig_is_the_fallback() {
        let mut dom = WeakDom::new();
        let model = rig(&mut dom);
        let anim = animation(&mut dom, None, "Animation", "AnimationId");
        assert_eq!(playable(&dom, anim, Some(model)), Some((507777826, model)));
    }
}
