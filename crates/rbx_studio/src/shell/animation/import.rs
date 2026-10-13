//! Animations from Blender: an FBX or glTF file retargeted onto the bound
//! rig, or the text the Blender "Rbx Animations" addon's *Export animation*
//! puts on the clipboard. Also the reverse, the current clip as that same
//! text.

use std::path::{Path, PathBuf};

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;
use rbx_animation::import::{self, rbxanims, Format};
use rbx_animation::Clip;

use super::Shell;
use crate::launcher::ui::{self, Weight};
use crate::tokens;

impl Shell {
    pub(super) fn choose_animation_file(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import Animation (FBX, glTF or .rbxm)".into()),
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
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Animation".into());
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        if matches!(extension.as_deref(), Some("rbxm" | "rbxmx")) {
            // A saved KeyframeSequence (ours, or Studio's "Save to File"):
            // already in the rig's joint names, so nothing to retarget.
            let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {shown}: {e}"))?;
            return rbx_animation::import::asset::clip_from_model(&bytes, &name)
                .map_err(|e| format!("{shown}: {e}"));
        }
        let format = Format::of(&shown).ok_or("Pick an .fbx, .glb, .gltf or .rbxm file")?;
        let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {shown}: {e}"))?;
        let bound = self
            .anim
            .bound
            .as_ref()
            .ok_or("Select a rig first: the animation is fitted onto its joints")?;
        let folder: PathBuf = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let external = move |uri: &str| std::fs::read(folder.join(uri)).ok();
        import::retargeted(format, &bytes, &external, &bound.rig, bound.root, &name)
            .map_err(|e| e.to_string())
    }

    /// Downloads a published animation by its `AnimationId` (anonymously,
    /// then through the stored Open Cloud key if Roblox refuses) and opens
    /// the `KeyframeSequence` it holds.
    pub(super) fn import_animation_id(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(id) = rbx_animation::import::asset::parse_id(text) else {
            self.anim
                .warn("Paste an AnimationId: rbxassetid://… or its number");
            cx.notify();
            return;
        };
        self.anim.say(format!("Downloading animation {id}…"));
        cx.notify();
        cx.spawn(async move |shell, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { super::published::fetch_clip(id) })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                if let Ok(clip) = &result {
                    shell.anim.published.insert(id, clip.clone());
                }
                shell.finish_import(result, cx)
            });
        })
        .detach();
    }

    pub(super) fn paste_blender_animation(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.anim.warn("The clipboard holds no text");
            cx.notify();
            return;
        };
        let result = rbxanims::decode(&text, "Blender Animation").map_err(|e| e.to_string());
        self.finish_import(result, cx);
    }

    pub(super) fn copy_blender_animation(&mut self, cx: &mut Context<Self>) {
        let text = rbxanims::encode(&self.anim.editor.clip);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.anim.say("Copied in the Rbx Animations format");
        cx.notify();
    }

    pub(super) fn finish_import(&mut self, result: Result<Clip, String>, cx: &mut Context<Self>) {
        match result {
            Ok(clip) => {
                let joints = clip.joints().len();
                let keys: usize = clip.keyframes.iter().map(|k| k.poses.len()).sum();
                self.anim.say(format!(
                    "Imported {}: {keys} keys on {joints} joints (unsaved)",
                    clip.name
                ));
                // Undoably, so an import over unsaved work can be taken back.
                self.anim.editor.edit(|c| *c = clip);
                self.anim.editor.source = None;
                self.anim.editor.time = 0.0;
                self.anim.editor.selected.clear();
                self.anim.fitted = false;
            }
            Err(e) => self.anim.fail(e),
        }
        cx.notify();
    }
}

/// The top bar's Import ▾ › From asset ID…: one field and Fetch, in the
/// launcher's dialog chrome, so an id can be pasted without opening the
/// side panel.
impl Shell {
    pub(in crate::shell) fn open_animation_id_prompt(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.anim.id_prompt.is_none() {
            let state = cx
                .new(|cx| InputState::new(window, cx).placeholder("rbxassetid://… or the number"));
            state.update(cx, |state, cx| state.focus(window, cx));
            self.anim.id_prompt = Some(state);
        }
        cx.notify();
    }

    /// Asks for the prompt from a place without a `Window` (a menu row).
    pub(in crate::shell) fn request_animation_id_prompt(&mut self, cx: &mut Context<Self>) {
        self.anim.dialog_requested = Some(super::AnimDialog::ImportId);
        cx.notify();
    }

    /// Escape's half of the prompt; returns whether it was open.
    pub(in crate::shell) fn close_animation_id_prompt(&mut self) -> bool {
        self.anim.id_prompt.take().is_some()
    }

    fn fetch_prompted_id(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.anim.id_prompt.take() else {
            return;
        };
        let text = state.read(cx).value().to_string();
        self.import_animation_id(&text, cx);
    }

    pub(in crate::shell) fn animation_id_prompt_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.anim.id_prompt.as_ref()?;
        let field = ui::field_frame(Some(360.), ui::panel2(), tokens::border(), "link")
            .mt(px(14.))
            .ml(px(78.))
            .child(
                div().flex_1().min_w_0().h_full().child(
                    Input::new(state)
                        .appearance(false)
                        .h_full()
                        .px(px(0.))
                        .text_size(px(12.))
                        .text_color(tokens::text()),
                ),
            )
            .into_any_element();
        Some(
            ui::dialog(
                480.,
                ui::dialog_glyph("download", ui::accent(), tokens::accent_soft()),
                "Import from asset ID",
                "Downloads a published animation and opens it here, unsaved. Your own private animations download through the stored Open Cloud key.",
                Some(field),
                vec![
                    ui::button("anim-id-cancel", "Cancel", Weight::Ghost, false)
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.close_animation_id_prompt();
                            cx.notify();
                        }))
                        .into_any_element(),
                    ui::button("anim-id-fetch", "Import", Weight::Primary, false)
                        .debug_selector(|| "anim-id-fetch".into())
                        .on_click(cx.listener(|shell, _, _, cx| shell.fetch_prompted_id(cx)))
                        .into_any_element(),
                ],
            )
            .into_any_element(),
        )
    }
}
