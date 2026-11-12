//! Studio's sound preview in Properties: a `Sound`'s `SoundId` (and an
//! `AudioPlayer`'s `Asset`, or its older `AssetId`) opens onto a Preview row
//! with a play/pause button and a rail that shows, and seeks, where the
//! sound is. The asset comes the way the viewport fetches textures (the
//! shared cache first), is decoded once, and plays through `crate::audio`
//! at the instance's `Volume`. Pausing keeps the place, as in Studio; the
//! end goes back to the start. One preview at a time, and nothing about the
//! place changes.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::*;
use rbx_dom::{Ref, Variant};

use crate::audio::{self, Clip, Playback};
use crate::command_bar::Feedback;
use crate::properties::{EditKind, PropertyRow};
use crate::tokens;

use super::rows::{expander, property_expandable, property_row_control, rail_slider};
use super::Shell;

const SOURCE: &str = "Sound Preview";

/// How often a playing preview's rail is moved along.
const POLL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub(super) struct SoundPreview {
    /// The instance and URI the rest is about.
    target: Option<(Ref, String)>,
    loading: bool,
    clip: Option<Arc<Clip>>,
    playback: Option<Playback>,
    /// Where a play starts from, in frames: a pause's place or a seek.
    position: usize,
    /// The rail, made with the first preview row.
    rail: Option<Entity<SliderState>>,
    /// Studio opens the row on its Preview; a click on its chevron folds it.
    folded: bool,
    /// Bumped by every play, so only the latest one's rail loop runs.
    plays: u64,
}

/// The asset properties a preview plays for `class`.
fn is_sound_property(class: &str, name: &str) -> bool {
    matches!(
        (class, name),
        ("Sound", "SoundId") | ("AudioPlayer", "Asset" | "AssetId")
    )
}

impl SoundPreview {
    fn is_for(&self, target: Ref, uri: &str) -> bool {
        self.target
            .as_ref()
            .is_some_and(|(r, u)| *r == target && u == uri)
    }

    fn playing(&self) -> bool {
        self.playback.is_some()
    }

    /// Where the rail sits, 0 to 1.
    fn fraction(&self) -> f32 {
        let frames = self.clip.as_ref().map_or(0, |clip| clip.frames());
        let at = self
            .playback
            .as_ref()
            .map_or(self.position, Playback::position);
        if frames == 0 {
            0.
        } else {
            at as f32 / frames as f32
        }
    }
}

impl Shell {
    /// `row` with its Preview beneath it when it is the selected `Sound`'s
    /// `SoundId` or `AudioPlayer`'s asset and names something to play;
    /// `None` leaves the row to the ordinary builder. `field` is the URI
    /// field that builder made.
    pub(super) fn sound_row(
        &mut self,
        row: &PropertyRow,
        field: AnyElement,
        error: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<AnyElement, AnyElement> {
        let Some((target, uri)) = self.sound_target(row) else {
            return Err(field);
        };
        let open = !self.sound_preview.folded;
        let preview = open.then(|| self.preview_row(row, target, uri, window, cx));
        let handle = cx.entity();
        let toggle = self.properties_nav.claim(
            expander(&row.name, open, move |_, _, cx| {
                handle.update(cx, |shell, cx| {
                    shell.sound_preview.folded = !shell.sound_preview.folded;
                    cx.notify();
                });
            }),
            cx,
        );
        Ok(property_expandable(&row.name, toggle, field, preview, error).into_any_element())
    }

    fn sound_target(&self, row: &PropertyRow) -> Option<(Ref, String)> {
        let target = self.selected().filter(|_| self.selected_all().len() == 1)?;
        if row.mixed || !is_sound_property(self.dom.get(target)?.class(), &row.name) {
            return None;
        }
        let uri = match row.edit.as_ref()? {
            EditKind::Text(text)
            | EditKind::Content {
                object: false,
                text,
            } => text.trim(),
            _ => return None,
        };
        (!uri.is_empty()).then(|| (target, uri.to_owned()))
    }

    /// The Preview row: Studio's play/pause and its rail.
    fn preview_row(
        &mut self,
        row: &PropertyRow,
        target: Ref,
        uri: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ours = self.sound_preview.is_for(target, &uri);
        let state = &self.sound_preview;
        let at = if ours { state.fraction() } else { 0. };
        // Every frame while it plays, not just each poll: a sound shorter
        // than a few polls (a click, a pickup) otherwise ends before the
        // rail visibly moves.
        if ours && state.playing() {
            window.request_animation_frame();
        }
        let (icon, label, color) = match () {
            _ if ours && state.loading => (IconName::LoaderCircle, "Loading", tokens::text_muted()),
            _ if ours && state.playing() => (IconName::Pause, "Pause", tokens::warning()),
            _ => (IconName::Play, "Play", tokens::diff_add()),
        };
        let handle = cx.entity();
        let play_uri = uri.clone();
        let button = super::chrome::icon_button(
            SharedString::from(format!("sound-preview-{}", row.name)),
            icon,
            label,
        )
        .text_color(color)
        .on_click(move |_, _, cx| {
            let uri = play_uri.clone();
            handle.update(cx, |shell, cx| shell.toggle_sound_preview(target, uri, cx));
        });
        let button = self.properties_nav.claim(button, cx);
        let rail = self.preview_rail(cx);
        // The rail follows the sound; only a change is written, so a still
        // rail asks for no redraw.
        if rail.read(cx).value().end() != at {
            rail.update(cx, |rail, cx| rail.set_value(at, window, cx));
        }
        let control = h_flex()
            .w_full()
            .items_center()
            .gap(tokens::label_gap())
            .child(button)
            .child(rail_slider(&rail, cx));
        let child = PropertyRow {
            name: format!("{}{}Preview", row.name, PropertyRow::CHILD),
            value: String::new(),
            category: row.category.clone(),
            edit: None,
            mixed: false,
            children: Vec::new(),
        };
        property_row_control(&child, control, false, None).into_any_element()
    }

    /// The one rail every preview row shows; a drag on it seeks.
    fn preview_rail(&mut self, cx: &mut Context<Self>) -> Entity<SliderState> {
        if let Some(rail) = &self.sound_preview.rail {
            return rail.clone();
        }
        let rail = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.001));
        cx.subscribe(&rail, |shell, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(SliderValue::Single(value)) = event {
                shell.seek_sound_preview(*value, cx);
            }
        })
        .detach();
        self.sound_preview.rail = Some(rail.clone());
        rail
    }
}

impl Shell {
    /// The play/pause button: plays from the pause's place (the start for a
    /// sound not loaded yet), or pauses there.
    fn toggle_sound_preview(&mut self, target: Ref, uri: String, cx: &mut Context<Self>) {
        cx.notify();
        if !self.sound_preview.is_for(target, &uri) {
            // Another sound: this one starts over, and the old one stops.
            self.sound_preview = SoundPreview {
                rail: self.sound_preview.rail.take(),
                folded: self.sound_preview.folded,
                plays: self.sound_preview.plays,
                target: Some((target, uri.clone())),
                ..SoundPreview::default()
            };
        }
        let state = &mut self.sound_preview;
        if state.loading {
            return;
        }
        if let Some(playback) = state.playback.take() {
            state.position = playback.position();
            return;
        }
        match state.clip.clone() {
            Some(clip) => self.start_sound_preview(clip, cx),
            None => self.load_sound_preview(target, uri, cx),
        }
    }

    fn load_sound_preview(&mut self, target: Ref, uri: String, cx: &mut Context<Self>) {
        self.sound_preview.loading = true;
        cx.spawn(async move |shell, cx| {
            let wanted = uri.clone();
            let clip = cx
                .background_spawn(async move {
                    audio::decode(rbx_viewer::asset_bytes(&wanted)?)
                        .map_err(|err| format!("{wanted}: {err}"))
                })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                // Another sound was picked while this one loaded.
                if !shell.sound_preview.is_for(target, &uri) {
                    return;
                }
                shell.sound_preview.loading = false;
                cx.notify();
                match clip {
                    Ok(clip) => {
                        let clip = Arc::new(clip);
                        shell.sound_preview.clip = Some(clip.clone());
                        shell.start_sound_preview(clip, cx);
                    }
                    Err(err) => {
                        shell.sound_preview.target = None;
                        shell.output.push(
                            SOURCE,
                            Feedback::Warning(format!("Couldn\u{2019}t play {err}")),
                        );
                    }
                }
            });
        })
        .detach();
    }

    /// Plays from the kept place, back at the start once it is the end, and
    /// moves the rail along until the sound ends or is paused.
    fn start_sound_preview(&mut self, clip: Arc<Clip>, cx: &mut Context<Self>) {
        let Some((target, _)) = self.sound_preview.target else {
            return;
        };
        let volume = self
            .dom
            .get(target)
            .and_then(|instance| match instance.properties().get("Volume") {
                Some(Variant::Float32(v)) => Some(*v),
                Some(Variant::Float64(v)) => Some(*v as f32),
                _ => None,
            })
            .unwrap_or(1.);
        let state = &mut self.sound_preview;
        if state.position >= clip.frames() {
            state.position = 0;
        }
        state.playback = Some(audio::play(clip, state.position, volume));
        state.plays += 1;
        let play = state.plays;
        cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(POLL).await;
            let going = shell.update(cx, |shell, cx| {
                let state = &mut shell.sound_preview;
                let Some(playback) = state.playback.as_ref().filter(|_| state.plays == play) else {
                    return false;
                };
                cx.notify();
                if !playback.is_finished() {
                    return true;
                }
                if let Some(err) = playback.error() {
                    let uri = state
                        .target
                        .as_ref()
                        .map(|(_, uri)| uri.clone())
                        .unwrap_or_default();
                    shell.output.push(
                        SOURCE,
                        Feedback::Warning(format!("Couldn\u{2019}t play {uri}: {err}")),
                    );
                }
                let state = &mut shell.sound_preview;
                state.playback = None;
                state.position = 0;
                false
            });
            if !matches!(going, Ok(true)) {
                break;
            }
        })
        .detach();
    }

    /// A click or drag on the rail: the sound carries on from there, or
    /// starts there on the next play.
    fn seek_sound_preview(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let state = &mut self.sound_preview;
        let Some(clip) = state.clip.clone() else {
            return;
        };
        state.position = (fraction.clamp(0., 1.) * clip.frames() as f32) as usize;
        if state.playback.take().is_some() {
            self.start_sound_preview(clip, cx);
        }
        cx.notify();
    }
}
