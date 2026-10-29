//! Plans and timers on disk, under `<config>/update-planning/<universe id>/`:
//! `timers.json`, `overlays/*.png` (picked overlays), `plan.json` and the
//! started plan's composites in `plan/`. The editor and the OS helper both
//! read and write here; every write is a temp file then a rename.

use std::path::{Path, PathBuf};

use super::plan::{default_timers, Plan, Timer};

/// The folder everything lives in.
pub(crate) fn root() -> Option<PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("update-planning"))
}

fn experience_dir(root: &Path, universe_id: u64) -> PathBuf {
    root.join(universe_id.to_string())
}

pub(crate) fn plan_image(root: &Path, universe_id: u64, name: &str) -> PathBuf {
    experience_dir(root, universe_id).join("plan").join(name)
}

pub(crate) fn overlay_file(root: &Path, universe_id: u64, name: &str) -> PathBuf {
    experience_dir(root, universe_id)
        .join("overlays")
        .join(name)
}

pub(crate) fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// The experience's timers; the defaults until it has saved its own.
pub(crate) fn load_timers(root: &Path, universe_id: u64) -> Vec<Timer> {
    std::fs::read(experience_dir(root, universe_id).join("timers.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(default_timers)
}

pub(crate) fn save_timers(root: &Path, universe_id: u64, timers: &[Timer]) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(timers)?;
    write(
        &experience_dir(root, universe_id).join("timers.json"),
        &json,
    )
}

/// Copies a picked overlay PNG in, named after its content so picking the
/// same file twice reuses it. Returns the stored name.
pub(crate) fn import_overlay(root: &Path, universe_id: u64, png: &[u8]) -> std::io::Result<String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    png.hash(&mut hasher);
    let name = format!("{:016x}.png", hasher.finish());
    write(&overlay_file(root, universe_id, &name), png)?;
    Ok(name)
}

pub(crate) fn load_plan(root: &Path, universe_id: u64) -> Option<Plan> {
    let bytes = std::fs::read(experience_dir(root, universe_id).join("plan.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn save_plan(root: &Path, plan: &Plan) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(plan)?;
    write(
        &experience_dir(root, plan.universe_id).join("plan.json"),
        &json,
    )
}

/// Writes a new plan's composites, then the plan itself, replacing any
/// finished one (its images go with it).
pub(crate) fn start(root: &Path, plan: &Plan, images: &[(String, Vec<u8>)]) -> std::io::Result<()> {
    let dir = experience_dir(root, plan.universe_id).join("plan");
    let _ = std::fs::remove_dir_all(&dir);
    for (name, bytes) in images {
        write(&dir.join(name), bytes)?;
    }
    save_plan(root, plan)
}

/// Every stored plan, finished or not.
pub(crate) fn plans(root: &Path) -> Vec<Plan> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut plans: Vec<Plan> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u64>().ok())
        .filter_map(|id| load_plan(root, id))
        .collect();
    plans.sort_by_key(|plan| plan.universe_id);
    plans
}

/// Whether any plan still has a step to fire: what keeps the OS helper
/// registered.
pub(crate) fn any_running(root: &Path) -> bool {
    plans(root).iter().any(|plan| !plan.finished())
}

#[cfg(test)]
mod tests {
    use super::super::plan::build_steps;
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rbx-update-planning-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn timers_default_then_round_trip() {
        let root = scratch("timers");
        assert_eq!(load_timers(&root, 5), default_timers());
        let mut timers = default_timers();
        timers.truncate(2);
        save_timers(&root, 5, &timers).unwrap();
        assert_eq!(load_timers(&root, 5), timers);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn started_plans_are_listed_and_finish() {
        let root = scratch("plans");
        let timers = default_timers();
        let names: Vec<String> = (0..4).map(|i| format!("step-{i}.png")).collect();
        let mut plan = Plan {
            universe_id: 9,
            experience: "X".into(),
            language: "en".into(),
            update_at: 100_000,
            started_at: 0,
            steps: build_steps(&timers, &names, "new.png", 100_000, 0),
            cancelled: false,
        };
        start(&root, &plan, &[("new.png".into(), b"png".to_vec())]).unwrap();
        assert_eq!(
            std::fs::read(plan_image(&root, 9, "new.png")).unwrap(),
            b"png"
        );
        assert!(any_running(&root));
        plan.cancel();
        save_plan(&root, &plan).unwrap();
        assert_eq!(plans(&root), [plan]);
        assert!(!any_running(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_same_overlay_is_imported_once() {
        let root = scratch("overlay");
        let a = import_overlay(&root, 1, b"one").unwrap();
        assert_eq!(import_overlay(&root, 1, b"one").unwrap(), a);
        assert_ne!(import_overlay(&root, 1, b"two").unwrap(), a);
        let _ = std::fs::remove_dir_all(&root);
    }
}
