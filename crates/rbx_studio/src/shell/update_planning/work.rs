//! Planning: picking icons and overlays, editing timers, and building the
//! composites the timeline shows and Start uploads.

use std::path::{Path, PathBuf};

use gpui_kit::*;
use image::RgbaImage;

use crate::update_planning::overlay::{self, Overlay, Style};
use crate::update_planning::plan::{Plan, Timer};
use crate::update_planning::{api, store, when};

use super::{render, Picked, Preview, UpdatePlanningWindow};

/// Stands in for the file picker: `<current>,<new>[,<overlay>]`.
const FILES_VARIABLE: &str = "RBX_STUDIO_UPDATE_PLAN_FILES";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pick {
    Current,
    New,
    /// A custom overlay for timer `i`.
    Overlay(usize),
}

impl Pick {
    fn stand_in(self) -> Option<PathBuf> {
        let list = std::env::var(FILES_VARIABLE).ok()?;
        let index = match self {
            Pick::Current => 0,
            Pick::New => 1,
            Pick::Overlay(_) => 2,
        };
        list.split(',')
            .nth(index)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
    }
}

fn picked(bytes: &[u8], source: String) -> Option<Picked> {
    let pixels = overlay::decode_square(bytes)?;
    Some(Picked {
        preview: render(&pixels)?,
        pixels,
        source,
    })
}

/// A timer's overlay pixels: ours drawn, or the user's PNG.
fn overlay_pixels(root: Option<&Path>, universe_id: u64, timer: &Timer) -> Option<RgbaImage> {
    match &timer.overlay {
        Overlay::Builtin(style) => overlay::render(*style, &timer.label),
        Overlay::File(name) => {
            let path = store::overlay_file(root?, universe_id, name);
            overlay::decode_square(&std::fs::read(path).ok()?)
        }
    }
}

impl UpdatePlanningWindow {
    pub(super) fn select(&mut self, universe_id: u64, cx: &mut Context<Self>) {
        self.selected = Some(universe_id);
        self.replan = false;
        self.timers = match &self.root {
            Some(root) => store::load_timers(root, universe_id),
            None => crate::update_planning::plan::default_timers(),
        };
        self.current = None;
        self.new_icon = None;
        self.status = None;
        self.refresh_previews(cx);
        if self.plan().is_none_or(Plan::finished) {
            self.fetch_current(cx);
        }
        cx.notify();
    }

    /// Redraws every timer's overlay and composite; after an icon or timer
    /// change, not per keystroke.
    pub(super) fn refresh_previews(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        let overlays: Vec<Option<RgbaImage>> = self
            .timers
            .iter()
            .map(|t| overlay_pixels(self.root.as_deref(), id, t))
            .collect();
        self.timer_thumbs = overlays
            .iter()
            .map(|o| o.as_ref().and_then(render))
            .collect();
        self.previews = match &self.current {
            Some(current) => self
                .timers
                .iter()
                .zip(&overlays)
                .enumerate()
                .filter_map(|(i, (timer, overlay))| {
                    let pixels = overlay::composite(&current.pixels, overlay.as_ref()?);
                    Some(Preview {
                        label: timer.label.clone(),
                        timer: Some(i),
                        minutes_before: Some(timer.minutes_before),
                        image: render(&pixels)?,
                        pixels,
                    })
                })
                .collect(),
            None => Vec::new(),
        };
        if let Some(new) = &self.new_icon {
            self.previews.push(Preview {
                label: "New icon".to_string(),
                timer: None,
                minutes_before: None,
                image: new.preview.clone(),
                pixels: new.pixels.clone(),
            });
        }
        cx.notify();
    }

    /// Timer `i`'s row picture: its composite once there is a current icon,
    /// else the overlay alone.
    pub(super) fn timer_thumb(&self, i: usize) -> Option<std::sync::Arc<RenderImage>> {
        self.previews
            .iter()
            .find(|p| p.timer == Some(i))
            .map(|p| p.image.clone())
            .or_else(|| self.timer_thumbs.get(i).cloned().flatten())
    }

    /// The update time typed in, or why it can't be used.
    pub(super) fn update_at(&self, cx: &App) -> Result<i64, String> {
        when::parse(&self.when.read(cx).value(), chrono::Utc::now().timestamp())
    }

    pub(super) fn background<T: Send + 'static>(
        &mut self,
        busy: &str,
        job: impl FnOnce() -> T + Send + 'static,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        self.busy = Some(busy.to_string());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let reply = cx.background_spawn(async move { job() }).await;
            let _ = this.update(cx, move |this, cx| {
                this.busy = None;
                done(this, reply, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn fetch_current(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        self.background(
            "Fetching the current icon\u{2026}",
            move || {
                let api = api::connect().ok_or("Add an API key to fetch the icon")?;
                api.current_icon(id)
            },
            cx,
            move |this, reply, cx| {
                if this.selected != Some(id) {
                    return;
                }
                match reply {
                    Ok((png, language)) => {
                        this.current = picked(&png, "From Roblox".to_string());
                        this.language = language;
                        this.refresh_previews(cx);
                    }
                    Err(err) => {
                        this.status = Some((true, format!("Couldn\u{2019}t fetch the icon: {err}")))
                    }
                }
            },
        );
    }

    pub(super) fn pick(&mut self, target: Pick, cx: &mut Context<Self>) {
        if let Some(path) = target.stand_in() {
            self.picked_file(target, path, cx);
            return;
        }
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose an image".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| this.picked_file(target, path, cx));
            }
        })
        .detach();
    }

    fn picked_file(&mut self, target: Pick, path: PathBuf, cx: &mut Context<Self>) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(bytes) = std::fs::read(&path)
            .ok()
            .filter(|b| overlay::decode_square(b).is_some())
        else {
            self.status = Some((true, format!("{name} isn\u{2019}t an image this can read")));
            cx.notify();
            return;
        };
        match target {
            Pick::Current => self.current = picked(&bytes, name),
            Pick::New => self.new_icon = picked(&bytes, name),
            Pick::Overlay(i) => {
                let (Some(root), Some(id)) = (self.root.clone(), self.selected) else {
                    return;
                };
                if let (Ok(stored), Some(timer)) = (
                    store::import_overlay(&root, id, &bytes),
                    self.timers.get_mut(i),
                ) {
                    timer.overlay = Overlay::File(stored);
                }
                self.save_timers();
            }
        }
        self.refresh_previews(cx);
    }

    fn save_timers(&mut self) {
        if let (Some(root), Some(id)) = (&self.root, self.selected) {
            if let Err(err) = store::save_timers(root, id, &self.timers) {
                self.status = Some((true, format!("Timers not saved: {err}")));
            }
        }
    }

    /// The next built-in style for timer `i` (a custom one goes back to the
    /// first).
    pub(super) fn cycle_style(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(timer) = self.timers.get_mut(i) else {
            return;
        };
        let next = match timer.overlay {
            Overlay::Builtin(style) => {
                let at = Style::ALL.iter().position(|s| *s == style).unwrap_or(0);
                Style::ALL[(at + 1) % Style::ALL.len()]
            }
            Overlay::File(_) => Style::ALL[0],
        };
        timer.overlay = Overlay::Builtin(next);
        self.save_timers();
        self.refresh_previews(cx);
    }

    pub(super) fn remove_timer(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.timers.len() {
            self.timers.remove(i);
            self.save_timers();
            self.refresh_previews(cx);
        }
    }

    pub(super) fn add_timer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.timer_label.read(cx).value().trim().to_string();
        let hours = self.timer_hours.read(cx).value().trim().replace(',', ".");
        let minutes = hours
            .parse::<f64>()
            .ok()
            .filter(|h| *h > 0. && *h <= 24. * 14.);
        let (false, Some(minutes)) = (label.is_empty(), minutes) else {
            self.status = Some((true, "A timer needs a label and 0 to 336 hours".to_string()));
            cx.notify();
            return;
        };
        self.timers.push(Timer {
            label,
            minutes_before: (minutes * 60.).round() as u32,
            overlay: Overlay::Builtin(Style::Hourglass),
        });
        self.timers
            .sort_by_key(|t| std::cmp::Reverse(t.minutes_before));
        self.save_timers();
        for input in [&self.timer_label, &self.timer_hours] {
            input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.refresh_previews(cx);
    }
}
