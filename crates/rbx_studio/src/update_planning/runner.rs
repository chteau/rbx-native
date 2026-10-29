//! Applying due steps: shared by the open editor (every few seconds) and the
//! OS helper (every minute). Each plan is re-read right before and right
//! after the post, so a Cancel pressed meanwhile in the editor survives.

use std::path::Path;

use super::api::IconApi;
use super::store;

/// One step a run applied, for the notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Applied {
    pub(crate) universe_id: u64,
    pub(crate) experience: String,
    pub(crate) label: String,
}

/// Applies every plan's due step. Failures are kept on the step and tried
/// again next run; the successes come back.
pub(crate) fn run_due(api: &dyn IconApi, root: &Path, now: i64) -> Vec<Applied> {
    let mut applied = Vec::new();
    for plan in store::plans(root) {
        let Some(index) = plan.to_apply(now) else {
            continue;
        };
        let step = &plan.steps[index];
        let result = std::fs::read(store::plan_image(root, plan.universe_id, &step.image))
            .map_err(|e| format!("the step's image is missing: {e}"))
            .and_then(|png| api.set_icon(plan.universe_id, &plan.language, &png));
        let Some(mut fresh) = store::load_plan(root, plan.universe_id) else {
            continue;
        };
        match result {
            Ok(()) => {
                fresh.mark_applied(index, now);
                applied.push(Applied {
                    universe_id: plan.universe_id,
                    experience: plan.experience.clone(),
                    label: step.label.clone(),
                });
            }
            Err(error) => {
                eprintln!("rbxstudio: update planning: {}: {error}", plan.experience);
                fresh.mark_failed(index, error);
            }
        }
        if let Err(err) = store::save_plan(root, &fresh) {
            eprintln!("rbxstudio: update planning: plan not saved: {err}");
        }
    }
    applied
}

/// "Pebble Kart: the 6 Hours icon is live" / "...: the new icon is live".
pub(crate) fn message(applied: &Applied) -> String {
    if applied.label == "New icon" {
        format!("{}: the new icon is live", applied.experience)
    } else {
        format!("{}: the {} icon is live", applied.experience, applied.label)
    }
}

/// A desktop notification, best effort.
pub(crate) fn notify(body: &str) {
    const TITLE: &str = "Update planning";
    #[cfg(target_os = "linux")]
    let command = std::process::Command::new("notify-send")
        .args(["--app-name=rbx-native", TITLE, body])
        .status();
    #[cfg(target_os = "macos")]
    let command = std::process::Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "display notification {:?} with title {TITLE:?}",
            body
        ))
        .status();
    // ponytail: Windows has no toast without a registered AppUserModelID;
    // the editor's own banner and the log line carry it there for now.
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let command: std::io::Result<std::process::ExitStatus> =
        Err(std::io::Error::other("no notifier"));
    if !command.is_ok_and(|status| status.success()) {
        eprintln!("rbxstudio: {TITLE}: {body}");
    }
}

#[cfg(test)]
mod tests {
    use super::super::plan::{build_steps, default_timers, Plan};
    use super::*;
    use rbx_cloud::{Moderation, Owner};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        posts: Mutex<Vec<(u64, String, Vec<u8>)>>,
        fail: bool,
    }

    impl IconApi for Fake {
        fn set_icon(&self, universe_id: u64, language: &str, png: &[u8]) -> Result<(), String> {
            if self.fail {
                return Err("HTTP 503".into());
            }
            self.posts
                .lock()
                .unwrap()
                .push((universe_id, language.into(), png.to_vec()));
            Ok(())
        }
        fn upload(&self, _: &str, _: Owner, _: &[u8]) -> Result<u64, String> {
            unreachable!()
        }
        fn review(&self, _: u64) -> Result<Moderation, String> {
            unreachable!()
        }
        fn current_icon(&self, _: u64) -> Result<(Vec<u8>, String), String> {
            unreachable!()
        }
        fn owner(&self, _: u64) -> Result<Owner, String> {
            unreachable!()
        }
    }

    const T: i64 = 1_000_000;

    fn started(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("rbx-up-runner-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let names: Vec<String> = (0..4).map(|i| format!("step-{i}.png")).collect();
        let plan = Plan {
            universe_id: 3,
            experience: "Pebble Kart".into(),
            language: "fr".into(),
            update_at: T,
            started_at: 0,
            steps: build_steps(&default_timers(), &names, "new.png", T, 0),
            cancelled: false,
        };
        let mut images: Vec<(String, Vec<u8>)> = names
            .iter()
            .map(|n| (n.clone(), n.as_bytes().to_vec()))
            .collect();
        images.push(("new.png".into(), b"new".to_vec()));
        store::start(&root, &plan, &images).unwrap();
        root
    }

    #[test]
    fn a_due_step_is_posted_once_and_recorded() {
        let root = started("once");
        let api = Fake::default();
        let applied = run_due(&api, &root, T - 6 * 3600 + 5);
        assert_eq!(applied.len(), 1);
        assert_eq!(
            message(&applied[0]),
            "Pebble Kart: the 6 Hours icon is live"
        );
        assert!(run_due(&api, &root, T - 6 * 3600 + 65).is_empty());
        let posts = api.posts.lock().unwrap();
        assert_eq!(
            posts.as_slice(),
            [(3, "fr".to_string(), b"step-1.png".to_vec())]
        );
        let plan = store::load_plan(&root, 3).unwrap();
        assert!(plan.steps[0].skipped);
        assert!(plan.steps[1].applied_at.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failure_is_kept_and_retried() {
        let root = started("fail");
        let failing = Fake {
            fail: true,
            ..Fake::default()
        };
        assert!(run_due(&failing, &root, T + 10).is_empty());
        let plan = store::load_plan(&root, 3).unwrap();
        assert_eq!(plan.steps[4].error.as_deref(), Some("HTTP 503"));
        let applied = run_due(&Fake::default(), &root, T + 70);
        assert_eq!(message(&applied[0]), "Pebble Kart: the new icon is live");
        assert!(store::load_plan(&root, 3).unwrap().finished());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cancelled_plan_posts_nothing() {
        let root = started("cancel");
        let mut plan = store::load_plan(&root, 3).unwrap();
        plan.cancel();
        store::save_plan(&root, &plan).unwrap();
        let api = Fake::default();
        assert!(run_due(&api, &root, T + 10).is_empty());
        assert!(api.posts.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
