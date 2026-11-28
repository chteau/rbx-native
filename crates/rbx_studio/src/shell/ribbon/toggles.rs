//! The ribbon's show/hide tiles (the docks, the Command Bar, Show UI,
//! Fullbright, Pseudo-loc) and the Script tab's Format and Find.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{fullbright_tooltip, show_ui_tooltip, tile, unavailable_tile};
use crate::shell::chrome::{Document, Trigger};
use crate::shell::layout::Panel;
use crate::shell::menu::{self, MenuId};
use crate::shell::{tooltip, Shell};
use crate::tokens;

/// A dock tile's icon and label, Studio's label where Studio has the dock
/// on its ribbon (Script's Analysis, Call Stack, Watch, Output).
fn dock_face(panel: Panel) -> (IconName, &'static str) {
    match panel {
        Panel::Explorer => (IconName::ListTree, "Explorer"),
        Panel::Properties => (IconName::SlidersHorizontal, "Properties"),
        Panel::Toolbox => (IconName::Package, "Toolbox"),
        Panel::Output => (IconName::Terminal, "Output"),
        Panel::Viewport => (IconName::MonitorCog, "Viewport"),
        Panel::Layers => (IconName::Layers, "Layers"),
        Panel::Loren => (IconName::Layers, "Loren"),
        Panel::SecurityAudit => (IconName::ShieldAlert, "Security Audit"),
        Panel::PrePublish => (IconName::ListChecks, "Pre-publish"),
        Panel::Playtests => (IconName::Film, "Playtests"),
        Panel::RemoteInspector => (IconName::Network, "Remote Inspector"),
        Panel::LeakFinder => (IconName::Bug, "Leak Finder"),
        Panel::LoadTest => (IconName::Users, "Load Test"),
        Panel::ScriptAnalysis => (IconName::Bug, "Analysis"),
        Panel::CallStack => (IconName::Layers, "Call Stack"),
        Panel::Watch => (IconName::Eye, "Watch"),
        Panel::Tests => (IconName::CircleCheck, "Tests"),
        Panel::Bytecode => (IconName::Code, "Bytecode"),
        Panel::Argon => (IconName::RefreshCw, "Argon"),
        Panel::Wally => (IconName::Package, "Wally"),
        _ => (IconName::PanelsTopLeft, "Panel"),
    }
}

/// A dock tile's hover text: what the dock is for, where the label alone
/// doesn't say.
fn dock_tip(panel: Panel) -> Option<&'static str> {
    Some(match panel {
        Panel::Toolbox => "Toolbox \u{2014} insert items from the Creator Store",
        Panel::ScriptAnalysis => "Analysis \u{2014} the Script Analysis panel",
        Panel::Bytecode => "Bytecode \u{2014} what Luau compiles this script to",
        Panel::PrePublish => "Pre-publish \u{2014} what to check before publishing",
        _ => return None,
    })
}

impl Shell {
    /// Lit while `on`, the look every toggle tile wears.
    pub(super) fn lit(tile: Stateful<Div>, on: bool) -> Stateful<Div> {
        tile.when(on, |this| {
            this.bg(tokens::accent_soft())
                .text_color(tokens::check_on())
        })
    }

    /// A dock's toggle, lit while it shows. A dock closed from its own tab
    /// has no tab left to bring it back with; this and the Window menu are
    /// the way back.
    pub(super) fn dock_tile(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        let (icon, label) = dock_face(panel);
        let showing = self.is_panel_showing(panel);
        let tip: SharedString = dock_tip(panel)
            .map(Into::into)
            .unwrap_or_else(|| format!("{label} \u{2014} show or hide the {label} panel").into());
        Self::lit(tile(&self.ribbon_nav, label, icon, label, cx), showing)
            .tooltip(move |window, cx| tooltip::text(tip.clone(), window, cx))
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.set_panel_open(panel, !showing, cx);
            }))
            .into_any_element()
    }

    /// Studio's Format: Format Document on the Script Editor's script, with
    /// StyLua, the same as the editor's own button and Shift+Alt+F.
    pub(super) fn format_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let script = (self.document == Document::Scripts)
            .then(|| self.scripts.tabs.active())
            .flatten();
        let Some(script) = script else {
            return unavailable_tile(
                "ribbon-script-format",
                IconName::WandSparkles,
                "Format",
                "no script is open in the Script Editor",
            )
            .into_any_element();
        };
        let tip = match crate::shortcuts::hint(crate::shortcuts::Command::FormatDocument) {
            Some(key) => format!("Format Document \u{2014} format this script with StyLua ({key})"),
            None => "Format Document \u{2014} format this script with StyLua".to_owned(),
        };
        tile(
            &self.ribbon_nav,
            "ribbon-script-format",
            IconName::WandSparkles,
            "Format",
            cx,
        )
        .tooltip(move |window, cx| tooltip::text(tip.clone(), window, cx))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.focus_script(script, window, cx);
            shell.format_script(script, window, cx);
        }))
        .into_any_element()
    }

    /// Studio's Find menu. Both rows open Find in Place, searching every
    /// script in the place, the second with its replace field.
    pub(super) fn find_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let find = |label: &'static str, icon, replace: bool| {
            menu::item(label).icon(icon).on_click(move |shell, cx| {
                shell.with_window(cx, move |shell, window, cx| {
                    shell.open_find_in_place(replace, window, cx)
                })
            })
        };
        let mut find_all = find("Find in Place", IconName::Search, false);
        if let Some(key) = crate::shortcuts::hint(crate::shortcuts::Command::FindAll) {
            find_all = find_all.hint(key);
        }
        menu::dropdown_in_group(
            self,
            MenuId::ScriptFind,
            Trigger::new(tile(
                &self.ribbon_nav,
                "ribbon-script-find",
                IconName::Search,
                "Find",
                cx,
            )),
            vec![find_all, find("Replace in Place", IconName::Replace, true)],
            cx,
        )
        .into_any_element()
    }

    /// Studio's Command: the Command Bar under the docks, lit while it
    /// shows.
    pub(super) fn command_bar_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let showing = !self.layout.command_bar_hidden();
        Self::lit(
            tile(
                &self.ribbon_nav,
                "ribbon-command-bar",
                IconName::SquareTerminal,
                "Command",
                cx,
            ),
            showing,
        )
        .tooltip(|window, cx| tooltip::text("Command \u{2014} the Command Bar", window, cx))
        .on_click(cx.listener(|shell, _, _, cx| shell.toggle_command_bar(cx)))
        .into_any_element()
    }

    pub(super) fn show_ui_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let shown = self.show_screen_gui;
        Self::lit(
            tile(
                &self.ribbon_nav,
                "ribbon-show-ui",
                IconName::Eye,
                "Show UI",
                cx,
            ),
            shown,
        )
        .tooltip(move |window, cx| tooltip::text(show_ui_tooltip(shown), window, cx))
        .on_click(
            cx.listener(|shell, _, _, cx| shell.set_screen_gui_shown(!shell.show_screen_gui, cx)),
        )
        .into_any_element()
    }

    pub(super) fn fullbright_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.fullbright;
        Self::lit(
            tile(
                &self.ribbon_nav,
                "ribbon-fullbright",
                IconName::Sun,
                "Fullbright",
                cx,
            ),
            on,
        )
        .tooltip(move |window, cx| tooltip::text(fullbright_tooltip(on), window, cx))
        .on_click(cx.listener(move |shell, _, _, cx| shell.set_fullbright(!on, cx)))
        .into_any_element()
    }

    /// Draws every GUI text longer and accented, and opens Localization
    /// beside it. Behind its beta.
    pub(super) fn pseudo_loc_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.pseudo_locale().is_some();
        Self::lit(
            tile(
                &self.ribbon_nav,
                "ribbon-pseudo-loc",
                IconName::Globe,
                "Pseudo-loc",
                cx,
            ),
            on,
        )
        .tooltip(|window, cx| {
            tooltip::text(
                "Pseudo-localize \u{2014} draw every GUI text longer and accented, and mark what no longer fits",
                window,
                cx,
            )
        })
        .on_click(cx.listener(move |shell, _, _, cx| {
            let locale = (!on).then(rbx_viewer::PseudoLocale::default);
            shell.set_pseudo_locale(locale, cx);
            if locale.is_some() {
                shell.set_panel_open(Panel::Localization, true, cx);
            }
        }))
        .into_any_element()
    }
}
