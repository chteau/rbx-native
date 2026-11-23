//! The Playtests dock and the edit-mode replay behind it: every recording
//! of the open place (`crate::playtests`), shown or hidden one by one and
//! overlaid in the 3D view at one shared moment on a timeline, plus a heat
//! map across the shown ones. Play records into it: see
//! [`Shell::playtests_session_start`].
//!
//! Game code marks moments with `shared.rbxnative.mark(name, data)`, guarded
//! so a published game (where `shared.rbxnative` is nil) skips it:
//!
//! ```lua
//! local rbxnative = shared.rbxnative
//! if rbxnative then rbxnative.mark("boss_down", { boss = "Golem" }) end
//! ```
//!
//! `RBX_STUDIO_PLAYTESTS=<presence|deaths|stuck|on>` opens the dock at
//! startup and, at its first render, shows every recording at 60% of the longest one with that
//! heat map (`on`: none), a screenshot aid.

mod dock;
mod playback;
mod share;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use rbx_play::protocol::Uplink;

use super::Shell;
use crate::command_bar::Feedback;
use crate::playtests::heat::{self, HeatKind};
use crate::playtests::recorder::Recorder;
use crate::playtests::{replay, store, Recording, Summary};

/// See this module's doc comment.
pub(super) const DEBUG_VARIABLE: &str = "RBX_STUDIO_PLAYTESTS";

/// The recording rate a fresh settings file starts with; 0 turns
/// recording off.
pub(crate) const DEFAULT_RATE_HZ: f32 = 10.0;
/// The rates the dock offers.
pub(super) const RATES: [f32; 4] = [0.0, 5.0, 10.0, 20.0];
/// The replay speeds the dock offers.
pub(super) const SPEEDS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];
/// Heat cell size, in studs: time spent is dense enough for a fine grid;
/// deaths and stuck spots are a handful of points, which a fine grid would
/// draw as specks under their own markers.
fn heat_cell(kind: HeatKind) -> f32 {
    match kind {
        HeatKind::Presence => 4.0,
        HeatKind::Deaths | HeatKind::Stuck => 8.0,
    }
}

/// One recording of the open place, as the dock lists it.
pub(super) struct Entry {
    pub(super) path: PathBuf,
    /// Why the file can't be read, when it can't: the dock says so on its row.
    pub(super) summary: Result<Summary, String>,
    /// Overlaid in the 3D view.
    pub(super) visible: bool,
    /// Read once it is first shown.
    pub(super) loaded: Option<Rc<Recording>>,
}

/// The dock's state.
pub(super) struct Playtests {
    pub(super) entries: Vec<Entry>,
    /// The recording in progress while Play runs, when recording is on.
    pub(super) recorder: Option<Recorder>,
    /// Samples per second Play records at; 0 is off. Persisted in settings.
    pub(super) rate_hz: f32,
    /// The replay's moment, seconds from each shown recording's start.
    pub(super) time: f32,
    pub(super) playing: bool,
    pub(super) speed: f32,
    pub(super) heat: Option<HeatKind>,
    /// The entry being renamed, and its field.
    pub(super) renaming: Option<(PathBuf, Entity<InputState>)>,
    /// Advances `time` while `playing`.
    ticker: Option<Task<()>>,
    /// Keeps the rename field's Enter handler alive.
    rename_subscription: Option<Subscription>,
    /// The place key `entries` were listed for.
    listed_key: Option<String>,
    /// The scrubber track's bounds, as last painted.
    scrub_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// `RBX_STUDIO_PLAYTESTS` has been looked at.
    env_applied: bool,
    /// The shared repro this window replays, read-only; the dock lists it
    /// alone.
    pub(super) repro: Option<share::ReproView>,
    /// Export repro… was picked for this recording; the dialog is built at
    /// the next render, which has the window its note field needs.
    export_request: Option<PathBuf>,
    pub(super) export: Option<share::ExportDraft>,
}

impl Playtests {
    pub(super) fn new(rate_hz: f32) -> Playtests {
        Playtests {
            entries: Vec::new(),
            recorder: None,
            rate_hz,
            time: 0.0,
            playing: false,
            speed: 1.0,
            heat: None,
            renaming: None,
            ticker: None,
            rename_subscription: None,
            listed_key: None,
            scrub_bounds: Rc::new(Cell::new(None)),
            env_applied: false,
            repro: None,
            export_request: None,
            export: None,
        }
    }

    /// The shown recordings that are loaded, with their list index.
    pub(super) fn shown(&self) -> Vec<(usize, Rc<Recording>)> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.visible)
            .filter_map(|(i, entry)| entry.loaded.clone().map(|rec| (i, rec)))
            .collect()
    }

    /// The longest shown recording's duration; 0 with none shown.
    pub(super) fn longest(&self) -> f32 {
        self.shown()
            .iter()
            .map(|(_, rec)| rec.duration)
            .fold(0.0, f32::max)
    }
}

impl Shell {
    fn playtests_place_key(&self) -> String {
        store::place_key(
            &self.path,
            crate::home::link_of(&self.path).map(|link| link.place_id),
        )
    }

    fn playtests_dir(&self) -> Option<PathBuf> {
        let root = crate::settings::default_config_dir()?;
        Some(store::place_dir(&root, &self.playtests_place_key()))
    }

    /// Lists again when the open place is not the one last listed.
    pub(super) fn playtests_refresh_if_moved(&mut self, cx: &mut Context<Self>) {
        if self.playtests.listed_key.as_deref() != Some(self.playtests_place_key().as_str()) {
            self.playtests_refresh(cx);
        }
    }

    /// Lists the open place's recordings again, keeping which were shown,
    /// and redraws the overlay.
    pub(super) fn playtests_refresh(&mut self, cx: &mut Context<Self>) {
        let key = self.playtests_place_key();
        if let Some(repro) = &self.playtests.repro {
            let visible = self
                .playtests
                .entries
                .first()
                .is_none_or(|entry| entry.visible);
            self.playtests.entries = vec![Entry {
                path: repro.file.clone(),
                summary: Ok(repro.recording.summary()),
                visible,
                loaded: Some(repro.recording.clone()),
            }];
            self.playtests.listed_key = Some(key);
            self.playtests_redraw(cx);
            return;
        }
        let listed = self
            .playtests_dir()
            .map(|dir| store::list(&dir))
            .unwrap_or_default();
        let mut old = std::mem::take(&mut self.playtests.entries);
        let same_place = self.playtests.listed_key.as_deref() == Some(key.as_str());
        self.playtests.entries = listed
            .into_iter()
            .map(|(path, summary)| {
                let kept = same_place
                    .then(|| old.iter().position(|entry| entry.path == path))
                    .flatten()
                    .map(|i| old.swap_remove(i));
                match kept {
                    Some(entry) => Entry {
                        loaded: if entry.summary == summary {
                            entry.loaded
                        } else {
                            None
                        },
                        visible: entry.visible,
                        path,
                        summary,
                    },
                    None => Entry {
                        path,
                        summary,
                        visible: false,
                        loaded: None,
                    },
                }
            })
            .collect();
        for i in 0..self.playtests.entries.len() {
            if self.playtests.entries[i].visible {
                self.playtests_ensure_loaded(i);
            }
        }
        self.playtests.listed_key = Some(key);
        self.playtests_redraw(cx);
    }

    /// Reads entry `i` if it is not yet; hides it, keeping why, when it
    /// cannot be read.
    pub(super) fn playtests_ensure_loaded(&mut self, i: usize) {
        let Some(entry) = self.playtests.entries.get_mut(i) else {
            return;
        };
        if entry.loaded.is_some() {
            return;
        }
        match store::load(&entry.path) {
            Ok(recording) => entry.loaded = Some(Rc::new(recording)),
            Err(err) => {
                entry.visible = false;
                entry.summary = Err(err);
            }
        }
    }

    /// Sends the shown recordings at the current time, and the heat map,
    /// to the 3D view.
    pub(super) fn playtests_redraw(&mut self, cx: &mut Context<Self>) {
        let shown = self.playtests.shown();
        let (segments, fills) = if shown.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let colored: Vec<(&Recording, [f32; 3])> = shown
                .iter()
                .map(|(i, rec)| (rec.as_ref(), replay::recording_color(*i)))
                .collect();
            let segments = replay::segments(&colored, self.playtests.time);
            let fills = match self.playtests.heat {
                Some(kind) => {
                    let recs: Vec<&Recording> = shown.iter().map(|(_, rec)| rec.as_ref()).collect();
                    replay::fills(&heat::grid(&recs, kind, heat_cell(kind)), kind)
                }
                None => Vec::new(),
            };
            (segments, fills)
        };
        self.viewport
            .update(cx, |view, _| view.set_playtests(segments, fills));
        cx.notify();
    }

    /// Play is starting: a fresh recorder, unless the rate is 0.
    pub(super) fn playtests_session_start(&mut self) {
        let rate = self.playtests.rate_hz;
        if rate <= 0.0 {
            self.playtests.recorder = None;
            return;
        }
        let name = format!(
            "{} {}",
            self.playtests_place_name(),
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        );
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.playtests.recorder = Some(Recorder::new(name, started, rate));
    }

    /// Hands a Play uplink to the recorder; true when nothing else wants it.
    pub(super) fn playtests_uplink(&mut self, uplink: &Uplink) -> bool {
        match &mut self.playtests.recorder {
            Some(recorder) => recorder.uplink(uplink),
            None => matches!(uplink, Uplink::RecSamples { .. } | Uplink::RecEvent(_)),
        }
    }

    /// Play stopped: saves the recording (if any sample came) and lists it.
    pub(super) fn playtests_session_end(&mut self, cx: &mut Context<Self>) {
        let Some(recording) = self.playtests.recorder.take().and_then(Recorder::finish) else {
            return;
        };
        let saved = match self.playtests_dir() {
            Some(dir) => store::save(&dir, &recording).map_err(|err| err.to_string()),
            None => Err("no config folder to keep it in".to_owned()),
        };
        let feedback = match saved {
            Ok(_) => {
                let summary = recording.summary();
                Feedback::Output(format!(
                    "Recorded {} with {} player{} and {} death{}; replay it in the Playtests dock.",
                    playback::clock(summary.duration),
                    summary.players,
                    if summary.players == 1 { "" } else { "s" },
                    summary.deaths,
                    if summary.deaths == 1 { "" } else { "s" },
                ))
            }
            Err(err) => Feedback::Warning(format!("Could not save the playtest recording: {err}")),
        };
        self.output
            .push(super::play::session::PLAY_SOURCE, feedback);
        self.playtests_refresh(cx);
    }
}
