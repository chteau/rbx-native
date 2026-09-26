//! The theme while the editor runs: switching to another one the moment
//! `appearance.json` names it, re-applying the active one the moment its
//! files are saved, and painting its background image.

use gpui_kit::*;

use super::Shell;
use crate::packs::{self, IconOverlay};
use crate::theme::{self, ThemePack};

impl Shell {
    /// Polls for a theme change for as long as the window lives — see
    /// `theme::watch` for why this polls.
    pub(super) fn watch_theme(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(theme::POLL_INTERVAL).await;
            let alive = shell.update(cx, |shell, cx| {
                if shell.theme_watch.changed() {
                    shell.reload_theme(cx);
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    /// Reads `appearance.json` again and applies the theme it names. A theme
    /// that no longer loads is reported in the Output dock and the one on
    /// screen stays — an author mid-edit sees what is wrong with the file
    /// rather than their theme vanishing.
    fn reload_theme(&mut self, cx: &mut Context<Self>) {
        let id = packs::Appearance::load()
            .theme
            .unwrap_or_else(|| theme::DEFAULT_ID.to_owned());
        let pack = match ThemePack::load(&id) {
            Ok(pack) => pack,
            Err(err) => {
                self.output
                    .push_warning(&format!("theme {id:?} was not applied: {err}"));
                cx.notify();
                return;
            }
        };
        for warning in &pack.warnings {
            self.output
                .push_warning(&format!("theme {id:?}: {warning}"));
        }
        self.theme_watch.retarget(pack.dir.clone());
        theme::apply(&pack, cx);

        let chosen = self
            .appearance
            .icon_pack
            .as_deref()
            .and_then(IconOverlay::load);
        crate::class_icons::set_user_pack(packs::layered(pack.icons.clone(), chosen));
        self.theme = pack;
        self.explorer = std::rc::Rc::new(self.explorer.set_icon_pack(
            self.icon_pack,
            &self.folder_colors,
            &self.path,
        ));
        cx.notify();
    }

    /// The theme's background image, sized to the window; `over` picks the
    /// layer the caller asks for. Never a hit target, so it can sit over the
    /// chrome without taking a click.
    pub(super) fn theme_background(&self, over: bool) -> Option<AnyElement> {
        let background = theme::active().effects.background.clone()?;
        if background.over != over {
            return None;
        }
        Some(
            img(background.path)
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .object_fit(background.fit.into())
                .opacity(background.opacity)
                .into_any_element(),
        )
    }
}
