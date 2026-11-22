//! Test › Offline: "Make available offline" for the open place. Every
//! asset the place refers to (and your avatar, the noob and the stock
//! animations) goes into the shared asset cache and is kept there for the
//! place (`rbx_sim::offline_pack`), so a Local Run with no network loads
//! it from there. The tile shows the pack's progress (a click cancels),
//! then its size, with Update and Remove offline data in its menu.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::*;
use rbx_assets::{AssetCache, AssetResolver, NativeContent};
use rbx_sim::offline_pack::{self, Manifest, Progress};

use super::{data, push_row, Fetcher};
use crate::command_bar::Feedback;
use crate::launcher::ui;
use crate::shell::menu::{self, MenuId};
use crate::shell::ribbon::{tile_with_badge, unavailable_tile};
use crate::shell::{chrome, Shell};

/// Failures listed one by one in the Output; past this, a count.
const LISTED_FAILURES: usize = 20;

#[derive(Default)]
pub(crate) struct Offline {
    packing: Option<Packing>,
    /// The open place's pack as last read from disk, by place file.
    summary: RefCell<Option<(PathBuf, Option<Summary>)>>,
}

struct Packing {
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    _task: Task<()>,
}

#[derive(Clone, Copy)]
struct Summary {
    bytes: u64,
    complete: bool,
}

/// `48.2 MB`, the way a pack's size reads on the tile.
fn size_label(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", bytes.div_ceil(1024)),
        _ => format!("{:.1} MB", bytes as f64 / MB),
    }
}

fn cache() -> Option<AssetCache> {
    AssetCache::new(None).ok()
}

impl Offline {
    fn summary(&self, path: &Path) -> Option<Summary> {
        let mut kept = self.summary.borrow_mut();
        if kept.as_ref().is_none_or(|(p, _)| p != path) {
            let read = cache()
                .and_then(|c| offline_pack::load(&c, &data::key(path)))
                .map(|m| Summary {
                    bytes: m.bytes(),
                    complete: m.complete,
                });
            *kept = Some((path.to_path_buf(), read));
        }
        kept.as_ref().and_then(|(_, s)| *s)
    }

    fn forget(&self) {
        self.summary.replace(None);
    }
}

/// Packs `dom` for `key`; blocking (network), off the UI thread.
fn pack(
    dom: &rbx_dom::WeakDom,
    key: &str,
    cancel: &AtomicBool,
    progress: &Mutex<Progress>,
) -> Result<(Manifest, String), String> {
    let cache = AssetCache::new(None).map_err(|e| e.to_string())?;
    let native = NativeContent::new(cache.native_packages_dir());
    let client = rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config());
    let resolver = AssetResolver::new(cache, Box::new(Fetcher(client)), native);
    let mut seeds = rbx_assets::refs::in_dom(dom);
    seeds.extend(offline_pack::engine_seeds());
    let (avatar, note) = super::avatars::pack_seeds();
    seeds.extend(avatar.into_iter().map(rbx_assets::AssetRef::Id));
    let manifest = offline_pack::build(&resolver, key, seeds, cancel, &|p| {
        *progress.lock().unwrap_or_else(|e| e.into_inner()) = p;
    });
    Ok((manifest, note))
}

impl Shell {
    /// Test › Offline.
    pub(in crate::shell) fn offline_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        const ID: &str = "ribbon-offline";
        if !crate::beta::enabled(super::BETA_ID) {
            return unavailable_tile(
                ID,
                IconName::CloudOff,
                "Offline",
                "turn on the Local Run beta in Settings",
            )
            .into_any_element();
        }
        let offline = &self.play.live.local.offline;
        if let Some(packing) = &offline.packing {
            let p = *packing.progress.lock().unwrap_or_else(|e| e.into_inner());
            // Nothing found yet: still asking for your avatar.
            let (badge, tip) = if p.found == 0 {
                (
                    "\u{2026}".to_owned(),
                    "Making this place available offline: getting your avatar. Click to cancel."
                        .to_owned(),
                )
            } else {
                (
                    format!("{}/{}", p.done, p.found),
                    format!(
                        "Making this place available offline: {} of {} assets, {} so far. Click to cancel.",
                        p.done,
                        p.found,
                        size_label(p.bytes)
                    ),
                )
            };
            return tile_with_badge(
                &self.ribbon_nav,
                ID,
                IconName::CloudOff,
                "Offline",
                ui::pill(badge, None, true),
                cx,
            )
            .tooltip(move |window, cx| crate::shell::tooltip::text(tip.clone(), window, cx))
            .on_click(cx.listener(|shell, _, _, cx| shell.offline_cancel(cx)))
            .into_any_element();
        }
        if self.local_run_active() {
            return unavailable_tile(ID, IconName::CloudOff, "Offline", "stop the run first")
                .into_any_element();
        }
        let Some(summary) = offline.summary(&self.path) else {
            return tile_with_badge(
                &self.ribbon_nav,
                ID,
                IconName::CloudOff,
                "Offline",
                ui::pill("Off", None, false),
                cx,
            )
            .tooltip(|window, cx| {
                crate::shell::tooltip::text(
                    "Make available offline: download every mesh, texture, sound, animation, package and font this place uses, so Local Run plays it with no internet.",
                    window,
                    cx,
                )
            })
            .on_click(cx.listener(|shell, _, _, cx| shell.offline_pack(cx)))
            .into_any_element();
        };
        let size = size_label(summary.bytes);
        let trigger = tile_with_badge(
            &self.ribbon_nav,
            ID,
            IconName::CloudOff,
            "Offline",
            ui::pill(size.clone(), None, summary.complete),
            cx,
        );
        menu::dropdown_in_group(
            self,
            MenuId::OfflinePack,
            chrome::Trigger::new(trigger),
            vec![
                menu::item(if summary.complete {
                    "Update offline pack"
                } else {
                    "Resume offline pack"
                })
                .icon(IconName::Download)
                .on_click(|shell, cx| shell.offline_pack(cx)),
                menu::item(format!("Remove offline data ({size})"))
                    .on_click(|shell, cx| shell.offline_remove(cx)),
            ],
            cx,
        )
        .into_any_element()
    }

    /// Make available offline: packs the open place in the background.
    pub(in crate::shell) fn offline_pack(&mut self, cx: &mut Context<Self>) {
        if self.play.live.local.offline.packing.is_some() || self.local_run_active() {
            return;
        }
        if rbx_net::is_offline() {
            push_row(
                &mut self.output,
                Feedback::Error(
                    "Make available offline needs the internet: Roblox can\u{2019}t be reached"
                        .to_owned(),
                ),
            );
            cx.notify();
            return;
        }
        push_row(
            &mut self.output,
            Feedback::Info("Making this place available offline\u{2026}".to_owned()),
        );
        let key = data::key(&self.path);
        let dom = self.dom.clone();
        let progress = Arc::new(Mutex::new(Progress::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        {
            let (progress, cancel, result) = (progress.clone(), cancel.clone(), result.clone());
            std::thread::spawn(move || {
                let done = pack(&dom, &key, &cancel, &progress);
                *result.lock().unwrap_or_else(|e| e.into_inner()) = Some(done);
            });
        }
        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let finished = result.lock().unwrap_or_else(|e| e.into_inner()).take();
            let alive = this.update(cx, |shell, cx| {
                if let Some(finished) = finished {
                    shell.offline_finished(finished);
                }
                cx.notify();
            });
            if alive.is_err()
                || this
                    .read_with(cx, |shell, _| shell.play.live.local.offline.packing.is_none())
                    .unwrap_or(true)
            {
                break;
            }
        });
        self.play.live.local.offline.packing = Some(Packing {
            progress,
            cancel,
            _task: task,
        });
        cx.notify();
    }

    fn offline_finished(&mut self, finished: Result<(Manifest, String), String>) {
        let offline = &mut self.play.live.local.offline;
        offline.packing = None;
        offline.forget();
        let (manifest, note) = match finished {
            Ok(done) => done,
            Err(why) => {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("Make available offline failed: {why}")),
                );
                return;
            }
        };
        for (uri, why) in manifest.failed.iter().take(LISTED_FAILURES) {
            push_row(
                &mut self.output,
                Feedback::Warning(format!("Offline pack: {uri} couldn\u{2019}t be downloaded ({why})")),
            );
        }
        let mut line = format!(
            "{} {} assets, {} ({note})",
            if manifest.complete {
                "Available offline:"
            } else {
                "Offline pack cancelled, kept so far:"
            },
            manifest.assets.len(),
            size_label(manifest.bytes()),
        );
        if !manifest.failed.is_empty() {
            line.push_str(&format!(
                "; {} couldn\u{2019}t be downloaded",
                manifest.failed.len()
            ));
        }
        push_row(&mut self.output, Feedback::Info(line));
    }

    fn offline_cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(packing) = &self.play.live.local.offline.packing {
            packing.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Remove offline data: the pack's manifest and every cached asset no
    /// other place's pack keeps.
    fn offline_remove(&mut self, cx: &mut Context<Self>) {
        let freed = cache().map_or(0, |c| offline_pack::remove(&c, &data::key(&self.path)));
        self.play.live.local.offline.forget();
        push_row(
            &mut self.output,
            Feedback::Info(format!("Offline data removed: {} freed", size_label(freed))),
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::size_label;

    #[test]
    fn sizes_read_in_the_largest_unit_that_fits() {
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(1025), "2 KB");
        assert_eq!(size_label(50 * 1024 * 1024 + 300_000), "50.3 MB");
    }
}
