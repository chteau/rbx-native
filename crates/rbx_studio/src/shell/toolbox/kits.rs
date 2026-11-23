//! The Toolbox's Free kits source: CC0 packs from Poly Haven and the zips
//! the user adds (see `crate::kits`), each opened to its models, imported
//! one per click or several at once. An import is Import 3D's upload, then
//! the models land side by side under the centre of the view, like a
//! Creator Store insert, as one undo step.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::Vec3;
use gpui_kit::*;
use rbx_dom::WeakDom;

use super::Load;
use crate::kits::{self, Pack, Thumb};
use crate::shell::Shell;

/// Studs between models imported side by side.
const GAP: f32 = 2.0;
/// Thumbnails fetched at once.
const THUMB_WORKERS: usize = 6;

#[derive(Default)]
pub(crate) struct KitBrowser {
    pub(super) load: Load,
    pub(super) packs: Vec<Pack>,
    /// Why Poly Haven's packs are missing.
    pub(super) offline: Option<String>,
    /// The pack shown, by id.
    pub(super) open: Option<String>,
    /// Ticked models of the open pack, in the order ticked.
    pub(super) selected: Vec<String>,
    /// `None` once a picture failed, so the glyph stays.
    pub(super) thumbs: HashMap<Thumb, Option<Arc<RenderImage>>>,
    asked: HashSet<Thumb>,
    /// The models uploading; one batch at a time.
    pub(super) busy: Vec<String>,
    /// The last import's or added pack's outcome.
    pub(super) status: Option<Result<String, String>>,
}

impl KitBrowser {
    pub(super) fn open_pack(&self) -> Option<&Pack> {
        let id = self.open.as_ref()?;
        self.packs.iter().find(|pack| &pack.id == id)
    }
}

impl Shell {
    /// Lists the packs, Poly Haven's from its cached catalogue when fresh.
    pub(super) fn kits_load(&mut self, cx: &mut Context<Self>) {
        self.toolbox.kits.load = Load::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (packs, offline) = cx.background_spawn(async move { kits::packs() }).await;
            let _ = this.update(cx, |shell, cx| {
                let browser = &mut shell.toolbox.kits;
                browser.load = if packs.is_empty() {
                    Load::Failed(offline.clone().unwrap_or_default())
                } else {
                    Load::Ready
                };
                browser.packs = packs;
                browser.offline = offline;
                let covers: Vec<Thumb> = browser.packs.iter().map(|p| p.thumb().clone()).collect();
                shell.kits_thumbs(covers, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn kits_open(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.kits_clear_filter(window, cx);
        let browser = &mut self.toolbox.kits;
        browser.open = Some(id);
        browser.selected.clear();
        browser.status = None;
        let thumbs: Vec<Thumb> = browser
            .open_pack()
            .map(|pack| pack.models.iter().map(|m| m.thumb.clone()).collect())
            .unwrap_or_default();
        self.kits_thumbs(thumbs, cx);
        cx.notify();
    }

    pub(super) fn kits_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.kits_clear_filter(window, cx);
        let browser = &mut self.toolbox.kits;
        browser.open = None;
        browser.selected.clear();
        cx.notify();
    }

    /// A filter typed for the pack list means nothing among one pack's models.
    fn kits_clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(query) = &self.toolbox.query {
            query.update(cx, |state, cx| state.set_value("", window, cx));
        }
    }

    pub(super) fn kits_toggle(&mut self, id: String, cx: &mut Context<Self>) {
        let selected = &mut self.toolbox.kits.selected;
        match selected.iter().position(|s| *s == id) {
            Some(index) => {
                selected.remove(index);
            }
            None if selected.len() < kits::BATCH_LIMIT => selected.push(id),
            None => {}
        }
        cx.notify();
    }

    fn kits_thumbs(&mut self, thumbs: Vec<Thumb>, cx: &mut Context<Self>) {
        let browser = &mut self.toolbox.kits;
        let wanted: Vec<Thumb> = thumbs
            .into_iter()
            .filter(|thumb| *thumb != Thumb::None && browser.asked.insert(thumb.clone()))
            .collect();
        if wanted.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_spawn(async move {
                    let chunk = wanted.len().div_ceil(THUMB_WORKERS).max(1);
                    std::thread::scope(|scope| {
                        let workers: Vec<_> = wanted
                            .chunks(chunk)
                            .map(|batch| {
                                scope.spawn(move || {
                                    batch
                                        .iter()
                                        .map(|thumb| (thumb.clone(), kits::thumbnail(thumb)))
                                        .collect::<Vec<_>>()
                                })
                            })
                            .collect();
                        workers
                            .into_iter()
                            .flat_map(|worker| worker.join().unwrap_or_default())
                            .collect::<Vec<_>>()
                    })
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.toolbox.kits.thumbs.extend(fetched);
                cx.notify();
            });
        })
        .detach();
    }

    /// The zip picker behind "Add a pack".
    pub(super) fn kits_choose_zip(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add a CC0 pack (.zip)".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(zip) = paths.into_iter().next() else {
                return;
            };
            let added = cx
                .background_spawn(async move { kits::add_pack(&zip) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.toolbox.kits.status = Some(match added {
                    Ok(pack) => Ok(format!(
                        "Added {} ({} models)",
                        pack.name,
                        pack.models.len()
                    )),
                    Err(err) => Err(err),
                });
                shell.kits_load(cx);
            });
        })
        .detach();
    }

    /// Uploads `ids` of the open pack off the UI thread, then lands them.
    pub(super) fn kits_import(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let browser = &mut self.toolbox.kits;
        let Some(pack) = browser.open_pack() else {
            return;
        };
        if !browser.busy.is_empty() || ids.is_empty() || self.toolbox.notice.is_some() {
            return;
        }
        let provider = pack.provider;
        let ids: Vec<String> = ids.into_iter().take(kits::BATCH_LIMIT).collect();
        browser.busy = ids.clone();
        browser.status = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let imported = cx
                .background_spawn(async move {
                    ids.iter()
                        .map(|id| kits::import(&kits::resolve(provider, id)?, None))
                        .collect::<Result<Vec<_>, String>>()
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                let browser = &mut shell.toolbox.kits;
                browser.busy.clear();
                let placed = imported.and_then(|prepared| {
                    let count = prepared.len();
                    let notes: Vec<String> =
                        prepared.iter().filter_map(|p| p.note.clone()).collect();
                    let name = match &prepared[..] {
                        [only] => only
                            .dom
                            .get(only.root)
                            .map_or_else(String::new, |i| i.name().to_owned()),
                        _ => format!("{count} models"),
                    };
                    let contents = row(prepared);
                    let contents: Vec<&WeakDom> = contents.iter().collect();
                    shell.toolbox_place(&contents, name.clone(), cx)?;
                    Ok(match notes.is_empty() {
                        true => format!("Imported {name}"),
                        false => format!("Imported {name}. {}", notes.join(" ")),
                    })
                });
                let browser = &mut shell.toolbox.kits;
                if placed.is_ok() {
                    browser.selected.clear();
                }
                browser.status = Some(placed);
                cx.notify();
            });
        })
        .detach();
    }
}

/// The models side by side along X, centred on the origin.
fn row(prepared: Vec<kits::Prepared>) -> Vec<WeakDom> {
    let width: f32 = prepared.iter().map(|p| p.size.x).sum::<f32>()
        + GAP * prepared.len().saturating_sub(1) as f32;
    let mut left = -width / 2.0;
    prepared
        .into_iter()
        .map(|mut model| {
            let offset = Vec3::new(left + model.size.x / 2.0, 0.0, 0.0);
            kits::translate(&mut model.dom, &[model.root], offset);
            left += model.size.x + GAP;
            model.dom
        })
        .collect()
}
