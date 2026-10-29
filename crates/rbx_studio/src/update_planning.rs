//! Update planning: countdown icons on autopilot (concept by MiniDev, see the
//! README's Acknowledgements). Per experience, reusable timers ("6 Hours")
//! are laid over the current icon; Start update uploads every composite for
//! review early, then each one becomes the experience's icon at its time,
//! and the new icon at the update. Behind the `update-planning` beta switch.
//!
//! The window is `shell::update_planning`; this module is what runs without
//! one: the plan rules (`plan`), the files (`store`), the overlays, the
//! runner both the editor and the OS helper call, and the helper itself.

pub(crate) mod api;
pub(crate) mod helper;
pub(crate) mod mock;
pub(crate) mod overlay;
pub(crate) mod plan;
pub(crate) mod runner;
pub(crate) mod store;
pub(crate) mod when;

pub(crate) const BETA_ID: &str = "update-planning";

/// How long the helper waits on the OS credential store.
const KEY_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// `rbxstudio --update-planning-tick`, what the OS helper runs every
/// minute: applies due steps, notifies, and unregisters itself once no
/// plan has steps left. Returns the exit code.
pub(crate) fn tick_main() -> i32 {
    let Some(root) = store::root() else {
        return 1;
    };
    if store::any_running(&root) {
        let Some(api) = connect_headless() else {
            eprintln!("rbxstudio: update planning: no API key, nothing applied");
            return 1;
        };
        let now = chrono::Utc::now().timestamp();
        for applied in runner::run_due(api.as_ref(), &root, now) {
            runner::notify(&runner::message(&applied));
        }
    }
    if !store::any_running(&root) {
        if let Err(err) = helper::unregister() {
            eprintln!("rbxstudio: update planning: helper not removed: {err}");
        }
    }
    0
}

/// [`api::connect`], with the key read from the OS credential store first
/// the way the editor does at startup, through a windowless GPUI app (the
/// store's API is GPUI's, on every OS).
fn connect_headless() -> Option<Box<dyn api::IconApi>> {
    if !api::mock_enabled() && rbx_cloud::ApiKey::from_env_or_config().is_none() {
        let found = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = found.clone();
        gpui_kit::platform::headless().run(move |cx| {
            cx.spawn(async move |cx| {
                let key = crate::key_store::read(cx).await.ok().flatten();
                *slot.lock().unwrap_or_else(|e| e.into_inner()) = key;
                cx.update(|cx| cx.quit());
            })
            .detach();
            // A credential store that never answers must not wedge the
            // scheduler's next runs behind this one.
            cx.spawn(async move |cx| {
                cx.background_executor().timer(KEY_WAIT).await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
        let key = found.lock().unwrap_or_else(|e| e.into_inner()).take();
        rbx_cloud::ApiKey::install(key);
    }
    api::connect()
}
