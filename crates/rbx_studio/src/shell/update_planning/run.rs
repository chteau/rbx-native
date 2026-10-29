//! Starting and cancelling an update, and the ticker that, while the window
//! is open, runs due steps, uploads the images and follows their review.

use std::path::Path;
use std::time::Duration;

use gpui_kit::*;

use crate::update_planning::overlay;
use crate::update_planning::plan::{build_steps, Plan, Review, Timer};
use crate::update_planning::{api, helper, runner, store};

use super::{render, Preview, UpdatePlanningWindow};

/// Runs due steps and polls reviews every this many ticks (seconds).
const SYNC_EVERY: u32 = 5;

impl UpdatePlanningWindow {
    /// What stops Start, if anything.
    pub(super) fn start_blocker(&self, cx: &App) -> Option<String> {
        if self.current.is_none() {
            return Some("Pick the current icon".to_string());
        }
        if self.new_icon.is_none() {
            return Some("Pick the new icon".to_string());
        }
        self.update_at(cx).err()
    }

    pub(super) fn start(&mut self, cx: &mut Context<Self>) {
        let (Some(root), Some(game), Ok(update_at)) = (
            self.root.clone(),
            self.experience().cloned(),
            self.update_at(cx),
        ) else {
            return;
        };
        if self.start_blocker(cx).is_some() {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let timer_previews: Vec<&Preview> =
            self.previews.iter().filter(|p| p.timer.is_some()).collect();
        let timers: Vec<Timer> = timer_previews
            .iter()
            .filter_map(|p| self.timers.get(p.timer?).cloned())
            .collect();
        let mut images: Vec<(String, Vec<u8>)> = timer_previews
            .iter()
            .enumerate()
            .map(|(i, p)| (format!("step-{i}.png"), overlay::png(&p.pixels)))
            .collect();
        let names: Vec<String> = images.iter().map(|(n, _)| n.clone()).collect();
        let Some(new) = &self.new_icon else {
            return;
        };
        images.push(("new.png".to_string(), overlay::png(&new.pixels)));
        let plan = Plan {
            universe_id: game.universe_id,
            experience: game.name.clone(),
            language: self.language.clone(),
            update_at,
            started_at: now,
            steps: build_steps(&timers, &names, "new.png", update_at, now),
            cancelled: false,
        };
        let count = plan.steps.len();
        self.background(
            "Starting\u{2026}",
            move || {
                store::start(&root, &plan, &images).map_err(|e| e.to_string())?;
                Ok::<_, String>(helper::register().err())
            },
            cx,
            move |this, reply, cx| {
                this.status = Some(match reply {
                    Ok(None) => (false, format!("Started: uploading {count} images for review")),
                    Ok(Some(err)) => (true, format!("Started, but the background helper failed ({err}): steps fire only while the editor is open")),
                    Err(err) => (true, format!("Couldn\u{2019}t start: {err}")),
                });
                this.reload_plans();
                this.sync(cx);
            },
        );
    }

    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        let (Some(root), Some(id)) = (self.root.clone(), self.selected) else {
            return;
        };
        if let Some(mut plan) = store::load_plan(&root, id) {
            plan.cancel();
            if let Err(err) = store::save_plan(&root, &plan) {
                self.status = Some((true, format!("Couldn\u{2019}t cancel: {err}")));
                return cx.notify();
            }
        }
        if !store::any_running(&root) {
            let _ = helper::unregister();
        }
        self.status = Some((
            false,
            "Cancelled: the current icon stays as it is".to_string(),
        ));
        self.reload_plans();
        cx.notify();
    }

    /// Plans as the disk has them, and their step images.
    pub(super) fn reload_plans(&mut self) {
        let Some(root) = &self.root else {
            return;
        };
        self.plans = store::plans(root)
            .into_iter()
            .map(|plan| (plan.universe_id, plan))
            .collect();
        for plan in self.plans.values() {
            for step in &plan.steps {
                let key = (plan.universe_id, step.image.clone());
                if let std::collections::hash_map::Entry::Vacant(slot) = self.step_images.entry(key)
                {
                    let path = store::plan_image(root, plan.universe_id, &step.image);
                    if let Some(image) = std::fs::read(path)
                        .ok()
                        .and_then(|b| image::load_from_memory(&b).ok())
                        .and_then(|i| render(&i.to_rgba8()))
                    {
                        slot.insert(image);
                    }
                }
            }
        }
    }

    pub(super) fn start_ticker(&mut self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let mut tick = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                tick += 1;
                let alive = this.update(cx, |this, cx| {
                    if tick.is_multiple_of(SYNC_EVERY) {
                        this.sync(cx);
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
    }

    /// Runs due steps, uploads what isn't yet and asks after the reviews.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.plans.values().all(Plan::finished) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let applied = cx
                .background_spawn(async move {
                    let api = api::connect()?;
                    let applied =
                        runner::run_due(api.as_ref(), &root, chrono::Utc::now().timestamp());
                    for plan in store::plans(&root).iter().filter(|p| !p.finished()) {
                        sync_reviews(api.as_ref(), &root, plan);
                    }
                    if !store::any_running(&root) {
                        let _ = helper::unregister();
                    }
                    Some(applied)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for done in applied.unwrap_or_default() {
                    let message = runner::message(&done);
                    runner::notify(&message);
                    this.status = Some((false, message));
                }
                this.reload_plans();
                cx.notify();
            });
        })
        .detach();
    }
}

/// Uploads the plan's images not uploaded yet and records Roblox's verdict
/// on those waiting. Each change is written to a fresh read of the plan.
fn sync_reviews(api: &dyn api::IconApi, root: &Path, plan: &Plan) {
    let id = plan.universe_id;
    let pending: Vec<usize> = (0..plan.steps.len())
        .filter(|&i| plan.steps[i].review == Review::Pending)
        .collect();
    let owner = if pending.is_empty() {
        None
    } else {
        api.owner(id).ok()
    };
    let mut changes: Vec<(usize, Option<u64>, Review)> = Vec::new();
    for i in pending {
        let step = &plan.steps[i];
        let png = std::fs::read(store::plan_image(root, id, &step.image));
        let name = format!("{} \u{2014} {}", plan.experience, step.label);
        if let (Some(owner), Ok(png)) = (owner, png) {
            match api.upload(&name, owner, &png) {
                Ok(asset) => changes.push((i, Some(asset), Review::Uploaded)),
                Err(err) => eprintln!("rbxstudio: update planning: upload failed: {err}"),
            }
        }
    }
    for (i, asset) in plan.awaiting_review() {
        if let Ok(state) = api.review(asset) {
            changes.push((i, Some(asset), state.into()));
        }
    }
    if changes.is_empty() {
        return;
    }
    let Some(mut fresh) = store::load_plan(root, id) else {
        return;
    };
    for (i, asset, review) in changes {
        if let Some(step) = fresh.steps.get_mut(i) {
            step.asset_id = asset;
            step.review = review;
        }
    }
    let _ = store::save_plan(root, &fresh);
}
