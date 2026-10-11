//! Animations from Blender: an FBX or glTF file retargeted onto the bound
//! rig, or the text the Blender "Rbx Animations" addon's *Export animation*
//! puts on the clipboard. Also the reverse, the current clip as that same
//! text.

use std::path::{Path, PathBuf};

use gpui_kit::*;
use rbx_animation::import::{self, rbxanims, Format};
use rbx_animation::Clip;

use super::Shell;

impl Shell {
    pub(super) fn choose_animation_file(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import Animation (FBX or glTF)".into()),
        });
        cx.spawn(async move |shell, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = shell.update(cx, |shell, cx| shell.import_animation_file(&path, cx));
            }
        })
        .detach();
    }

    /// Reads `path` and makes it the clip being edited (unsaved).
    pub(super) fn import_animation_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let result = self.read_animation_file(path);
        self.finish_import(result, cx);
    }

    fn read_animation_file(&self, path: &Path) -> Result<Clip, String> {
        let shown = path.display().to_string();
        let format = Format::of(&shown).ok_or("Pick an .fbx, .glb or .gltf file")?;
        let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {shown}: {e}"))?;
        let bound = self
            .anim
            .bound
            .as_ref()
            .ok_or("Select a rig first: the animation is fitted onto its joints")?;
        let folder: PathBuf = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let external = move |uri: &str| std::fs::read(folder.join(uri)).ok();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Animation".into());
        import::retargeted(format, &bytes, &external, &bound.rig, bound.root, &name)
            .map_err(|e| e.to_string())
    }

    /// Downloads a published animation by its `AnimationId` (anonymously,
    /// then through the stored Open Cloud key if Roblox refuses) and opens
    /// the `KeyframeSequence` it holds.
    pub(super) fn import_animation_id(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(id) = rbx_animation::import::asset::parse_id(text) else {
            self.anim.status = Some("Paste an AnimationId: rbxassetid://… or its number".into());
            cx.notify();
            return;
        };
        self.anim.status = Some(format!("Downloading animation {id}…").into());
        cx.notify();
        cx.spawn(async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let client = rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config());
                    let content = client
                        .asset(id)
                        .map_err(|e| format!("Animation {id}: {e}"))?;
                    rbx_animation::import::asset::clip_from_model(&content.bytes, &id.to_string())
                        .map_err(|e| format!("Animation {id}: {e}"))
                })
                .await;
            let _ = shell.update(cx, |shell, cx| shell.finish_import(result, cx));
        })
        .detach();
    }

    pub(super) fn paste_blender_animation(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.anim.status = Some("The clipboard holds no text".into());
            cx.notify();
            return;
        };
        let result = rbxanims::decode(&text, "Blender Animation").map_err(|e| e.to_string());
        self.finish_import(result, cx);
    }

    pub(super) fn copy_blender_animation(&mut self, cx: &mut Context<Self>) {
        let text = rbxanims::encode(&self.anim.editor.clip);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.anim.status = Some("Copied in the Rbx Animations format".into());
        cx.notify();
    }

    fn finish_import(&mut self, result: Result<Clip, String>, cx: &mut Context<Self>) {
        match result {
            Ok(clip) => {
                let joints = clip.joints().len();
                let keys: usize = clip.keyframes.iter().map(|k| k.poses.len()).sum();
                self.anim.status = Some(
                    format!(
                        "Imported {}: {keys} keys on {joints} joints (unsaved)",
                        clip.name
                    )
                    .into(),
                );
                // Undoably, so an import over unsaved work can be taken back.
                self.anim.editor.edit(|c| *c = clip);
                self.anim.editor.source = None;
                self.anim.editor.time = 0.0;
                self.anim.editor.selected.clear();
                self.anim.fitted = false;
            }
            Err(e) => self.anim.status = Some(e.into()),
        }
        cx.notify();
    }
}
