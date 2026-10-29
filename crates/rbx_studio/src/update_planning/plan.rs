//! A started update as data, and the rules that drive it. Pure: no clock,
//! no disk, no network; every function takes `now` (Unix seconds, UTC).
//!
//! A runner never asks "which step is next?", only "which image does the
//! plan say is due now?", and applies it if it hasn't been. So a run that
//! comes late (the PC was off at H-3, on again at H-1:30) applies H-3's
//! successor straight away and marks what it slept through as skipped, and
//! the editor and the OS helper can both run without stepping on each
//! other: the worst case is the same image posted twice.

use rbx_cloud::Moderation;
use serde::{Deserialize, Serialize};

/// One countdown overlay: shown `minutes_before` the update, labelled
/// `label` ("6 Hours").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Timer {
    pub(crate) label: String,
    pub(crate) minutes_before: u32,
    pub(crate) overlay: super::overlay::Overlay,
}

/// The timers every experience starts with.
pub(crate) fn default_timers() -> Vec<Timer> {
    [
        (12, "12 Hours"),
        (6, "6 Hours"),
        (3, "3 Hours"),
        (1, "1 Hour"),
    ]
    .into_iter()
    .map(|(hours, label)| Timer {
        label: label.to_string(),
        minutes_before: hours * 60,
        overlay: super::overlay::Overlay::Builtin(super::overlay::Style::Hourglass),
    })
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Review {
    /// Not uploaded yet (or the upload failed and will be retried).
    Pending,
    Uploaded,
    Approved,
    Rejected,
}

impl From<Moderation> for Review {
    fn from(state: Moderation) -> Review {
        match state {
            Moderation::Approved => Review::Approved,
            Moderation::Rejected => Review::Rejected,
            Moderation::Reviewing | Moderation::Unavailable => Review::Uploaded,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Step {
    /// When it becomes the icon.
    pub(crate) at: i64,
    /// "6 Hours", or "New icon" for the last step.
    pub(crate) label: String,
    /// The composited PNG, a file name in the plan's folder.
    pub(crate) image: String,
    pub(crate) asset_id: Option<u64>,
    pub(crate) review: Review,
    pub(crate) applied_at: Option<i64>,
    #[serde(default)]
    pub(crate) skipped: bool,
    /// The last failed attempt at applying it, retried on the next run.
    #[serde(default)]
    pub(crate) error: Option<String>,
}

/// What the steps view shows on the right of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepState {
    Upcoming,
    /// Its time has come and it hasn't been applied yet.
    Due,
    Applied,
    /// A later step came due before this one was applied.
    Skipped,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub(crate) universe_id: u64,
    pub(crate) experience: String,
    /// The language code the icon is posted for: the experience's source
    /// language.
    pub(crate) language: String,
    pub(crate) update_at: i64,
    pub(crate) started_at: i64,
    /// Ascending by `at`; the last one is the new icon at `update_at`.
    pub(crate) steps: Vec<Step>,
    pub(crate) cancelled: bool,
}

/// The steps a plan starts with: one per timer at `update_at` minus its
/// lead, then the new icon at `update_at`. Of the timers already past at
/// `now`, only the latest is kept (it is the icon due right now); the
/// rest would never show. `images[i]` names timer `i`'s composite, `new`
/// the new icon's.
pub(crate) fn build_steps(
    timers: &[Timer],
    images: &[String],
    new: &str,
    update_at: i64,
    now: i64,
) -> Vec<Step> {
    let mut steps: Vec<Step> = timers
        .iter()
        .zip(images)
        .map(|(timer, image)| {
            step(
                update_at - i64::from(timer.minutes_before) * 60,
                &timer.label,
                image,
            )
        })
        .filter(|step| step.at < update_at)
        .collect();
    steps.sort_by_key(|step| step.at);
    steps.dedup_by_key(|step| step.at);
    if let Some(last_past) = steps.iter().rposition(|step| step.at <= now) {
        steps.drain(..last_past);
    }
    steps.push(step(update_at, "New icon", new));
    steps
}

fn step(at: i64, label: &str, image: &str) -> Step {
    Step {
        at,
        label: label.to_string(),
        image: image.to_string(),
        asset_id: None,
        review: Review::Pending,
        applied_at: None,
        skipped: false,
        error: None,
    }
}

impl Plan {
    /// The step whose image should be the icon at `now`, if any has come.
    pub(crate) fn due(&self, now: i64) -> Option<usize> {
        self.steps.iter().rposition(|step| step.at <= now)
    }

    /// The step a runner should apply now: the due one, unless it is
    /// already applied or the plan was cancelled.
    pub(crate) fn to_apply(&self, now: i64) -> Option<usize> {
        if self.cancelled {
            return None;
        }
        self.due(now)
            .filter(|&i| self.steps[i].applied_at.is_none())
    }

    /// Records step `index` as the icon since `now`; every earlier step
    /// that never was is skipped.
    pub(crate) fn mark_applied(&mut self, index: usize, now: i64) {
        for step in &mut self.steps[..index] {
            if step.applied_at.is_none() {
                step.skipped = true;
            }
        }
        let step = &mut self.steps[index];
        step.applied_at = Some(now);
        step.error = None;
    }

    pub(crate) fn mark_failed(&mut self, index: usize, error: String) {
        self.steps[index].error = Some(error);
    }

    /// Stops it: no further step is applied. What is already the icon
    /// stays the icon.
    pub(crate) fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Nothing left to do: cancelled, or the new icon is applied.
    pub(crate) fn finished(&self) -> bool {
        self.cancelled || self.steps.last().is_some_and(|s| s.applied_at.is_some())
    }

    /// When the next step comes due, for the countdown.
    pub(crate) fn next_at(&self, now: i64) -> Option<i64> {
        if self.cancelled {
            return None;
        }
        self.steps.iter().map(|s| s.at).find(|&at| at > now)
    }

    pub(crate) fn state(&self, index: usize, now: i64) -> StepState {
        let step = &self.steps[index];
        if step.applied_at.is_some() {
            StepState::Applied
        } else if step.skipped || self.due(now).is_some_and(|due| due > index) {
            StepState::Skipped
        } else if self.cancelled {
            StepState::Cancelled
        } else if step.at <= now {
            StepState::Due
        } else {
            StepState::Upcoming
        }
    }

    /// The uploaded steps Roblox has not judged yet, for the review poll.
    pub(crate) fn awaiting_review(&self) -> Vec<(usize, u64)> {
        self.steps
            .iter()
            .enumerate()
            .filter(|(_, s)| s.review == Review::Uploaded)
            .filter_map(|(i, s)| s.asset_id.map(|id| (i, id)))
            .collect()
    }
}

/// "2d 4h", "3h 05m", "4m 09s", "12s": the "update in" countdown.
pub(crate) fn countdown(seconds: i64) -> String {
    let s = seconds.max(0);
    let (d, h, m, sec) = (s / 86_400, s / 3_600 % 24, s / 60 % 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {sec:02}s")
    } else {
        format!("{sec}s")
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
