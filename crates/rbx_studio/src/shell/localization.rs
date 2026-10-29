//! The Localization dock (beta): the pseudo-localization preview's switches
//! and the text objects it no longer fits, and the place's user-facing
//! strings with the `LocalizationTable` they can be written into.
//!
//! The preview is the renderer's alone (`rbx_viewer::PseudoLocale`): the 3D
//! view's overlay and the UI editor's canvas draw it, the DOM never sees it,
//! so saving and undo are as if it were off. Writing the table is an
//! ordinary edit, one undo step.
//!
//! `RBX_STUDIO_PSEUDO_LOC=1` turns the preview on at startup and opens the
//! dock, `rtl` mirrored too, `strings` on its Strings tab: a screenshot aid
//! like the other `RBX_STUDIO_*` variables.

use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_viewer::PseudoLocale;

use crate::localization::{self, Found, Origin};
use crate::shell::chrome::ScrollbarY as _;
use crate::tokens;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::Shell;

/// The stable name `beta.json` stores.
pub(crate) const BETA_ID: &str = "pseudo-localization";

const STARTUP_VARIABLE: &str = "RBX_STUDIO_PSEUDO_LOC";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Overflow,
    Strings,
}

#[derive(Default)]
pub(super) struct LocalizationState {
    locale: Option<PseudoLocale>,
    tab: Tab,
    /// The strings as of DOM mutation `at`, and which of them a table
    /// already holds.
    strings: Vec<Found>,
    known: HashSet<String>,
    at: Option<u64>,
    /// What the last write did, or why it did not, and the DOM mutation it
    /// describes: any later edit, an undo, makes it old news.
    status: Option<(u64, String)>,
    scroll: ScrollHandle,
}

impl Shell {
    pub(super) fn apply_debug_pseudo_locale(&mut self, cx: &mut Context<Self>) {
        let Some(value) = std::env::var_os(STARTUP_VARIABLE) else {
            return;
        };
        if crate::beta::enabled(BETA_ID) {
            let mirror = value == "rtl";
            if value == "strings" {
                self.localization.tab = Tab::Strings;
            }
            self.set_pseudo_locale(Some(PseudoLocale { mirror }), cx);
            self.set_panel_open(Panel::Localization, true, cx);
        }
    }

    pub(super) fn pseudo_locale(&self) -> Option<PseudoLocale> {
        self.localization.locale
    }

    /// Turns the preview on (`Some`) or off, overlay and canvas both.
    pub(super) fn set_pseudo_locale(
        &mut self,
        locale: Option<PseudoLocale>,
        cx: &mut Context<Self>,
    ) {
        self.localization.locale = locale;
        self.viewport
            .update(cx, |viewport, cx| viewport.set_pseudo_locale(locale, cx));
        cx.notify();
    }

    /// The text objects overflowing under the preview: the UI editor's
    /// canvas first while it is up, then the 3D view's overlay, once each.
    fn overflowing(&self, cx: &App) -> Vec<Ref> {
        if self.localization.locale.is_none() {
            return Vec::new();
        }
        let viewport = self.viewport.read(cx);
        let canvas = viewport
            .canvas()
            .filter(|_| self.ui_canvas_active())
            .map(|canvas| canvas.overflows.clone())
            .unwrap_or_default();
        let mut all = canvas;
        for &referent in viewport.overflows() {
            if !all.contains(&referent) {
                all.push(referent);
            }
        }
        all
    }

    fn refresh_strings(&mut self) {
        let stamp = self.edit_dom().mutations();
        if self.localization.at != Some(stamp) {
            self.localization.strings = localization::collect(self.edit_dom());
            self.localization.known = localization::known_sources(self.edit_dom());
            self.localization.at = Some(stamp);
        }
    }

    /// Writes every new string into the place's table as one undo step,
    /// then selects the table.
    fn write_localization_table(&mut self, cx: &mut Context<Self>) {
        if !std::ptr::eq(self.edit_dom(), &self.dom) {
            let stamp = self.edit_dom().mutations();
            self.localization.status = Some((stamp, "Stop the playtest to write the table".into()));
            cx.notify();
            return;
        }
        let found = localization::collect(&self.dom);
        let before = self.history_snapshot();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = localization::write(&mut dom, &found);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.commit_history_step(before, changes);
        let message = match result {
            Ok((table, added)) => {
                self.select(table, cx);
                let name = rbx_audit::full_name(&self.dom, table);
                match added {
                    1 => format!("Added 1 string to {name}"),
                    n => format!("Added {n} strings to {name}"),
                }
            }
            Err(err) => format!("Nothing written: {err}"),
        };
        self.localization.status = Some((self.dom.mutations(), message));
        cx.notify();
    }

    pub(super) fn localization_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::LocalizationOverflow,
            chrome::dock_options_trigger(
                "localization-overflow",
                IconName::Ellipsis,
                16.,
                "Localization dock options",
            ),
            self.move_items(Panel::Localization),
            cx,
        );
        self.refresh_strings();
        let overflowing = self.overflowing(cx);
        let state = &self.localization;
        let fresh = state
            .strings
            .iter()
            .filter(|found| !state.known.contains(&found.text))
            .map(|found| found.text.as_str())
            .collect::<HashSet<_>>()
            .len();

        let tab = |id: &'static str, label: String, which: Tab| {
            chrome::button(id, label, state.tab == which).on_click(cx.listener(
                move |shell, _, _, cx| {
                    shell.localization.tab = which;
                    cx.notify();
                },
            ))
        };
        let locale = state.locale;
        let mut header = h_flex()
            .w_full()
            .flex_none()
            .gap(px(4.))
            .child(tab(
                "localization-tab-overflow",
                format!("Overflow {}", overflowing.len()),
                Tab::Overflow,
            ))
            .child(tab(
                "localization-tab-strings",
                format!("Strings {}", state.strings.len()),
                Tab::Strings,
            ))
            .child(div().flex_1());
        header = match state.tab {
            Tab::Overflow => header
                .child(
                    chrome::button("localization-pseudo", "Pseudo-localize", locale.is_some())
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            let next = match locale {
                                Some(_) => None,
                                None => Some(PseudoLocale::default()),
                            };
                            shell.set_pseudo_locale(next, cx);
                        })),
                )
                .child(
                    chrome::button(
                        "localization-rtl",
                        "Right to left",
                        locale.is_some_and(|locale| locale.mirror),
                    )
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        let mirror = !locale.is_some_and(|locale| locale.mirror);
                        shell.set_pseudo_locale(Some(PseudoLocale { mirror }), cx);
                    })),
                ),
            Tab::Strings => header.child(
                chrome::button(
                    "localization-write",
                    match fresh {
                        0 => "Nothing new to write".to_string(),
                        1 => "Write 1 to table".to_string(),
                        n => format!("Write {n} to table"),
                    },
                    false,
                )
                .when(fresh == 0, |this| this.opacity(0.5).cursor_default())
                .when(fresh > 0, |this| {
                    this.on_click(cx.listener(|shell, _, _, cx| shell.write_localization_table(cx)))
                }),
            ),
        };

        let list = match state.tab {
            Tab::Overflow => self.overflow_rows(&overflowing, cx),
            Tab::Strings => self.string_rows(cx),
        };
        let status = state
            .status
            .clone()
            .filter(|(at, _)| state.tab == Tab::Strings && state.at == Some(*at))
            .map(|(_, message)| message);
        // The frame's 5px inset around a dock's content (`chrome::dock_content`).
        let body = v_flex()
            .size_full()
            .p(px(5.))
            .gap(px(6.))
            .child(header)
            .when_some(status, |this, status| this.child(caption(status)))
            .child(
                div()
                    .id("localization-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.localization.scroll)
                    .pr(chrome::SCROLLBAR_GUTTER)
                    .child(list)
                    .scrollbar_y(&self.localization.scroll),
            );
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    fn overflow_rows(&self, overflowing: &[Ref], cx: &mut Context<Self>) -> AnyElement {
        if self.localization.locale.is_none() {
            return empty("Turn on Pseudo-localize to draw every GUI text longer and accented, and list what no longer fits.");
        }
        if overflowing.is_empty() {
            return empty("Every text still fits. Open the UI Editor or show the UI over the 3D view to check a screen.");
        }
        let dom = self.edit_dom();
        let rows = overflowing.iter().enumerate().map(|(index, &referent)| {
            let name = dom.get(referent).map_or("", |i| i.name()).to_string();
            let path = rbx_audit::full_name(dom, referent);
            row(
                ("localization-overflow-row", index),
                IconName::TriangleAlert,
                name,
                path,
                None,
            )
            .on_click(cx.listener(move |shell, _, _, cx| shell.select(referent, cx)))
            .into_any_element()
        });
        v_flex().w_full().children(rows).into_any_element()
    }

    fn string_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.localization;
        if state.strings.is_empty() {
            return empty("No user-facing strings: text objects with AutoLocalize on, and literals client scripts assign to .Text, show here.");
        }
        let rows = state.strings.iter().enumerate().map(|(index, found)| {
            let (icon, line) = match found.origin {
                Origin::Gui(_) => (IconName::Type, None),
                Origin::Script(_, line) => (IconName::FileCode, Some(line)),
            };
            let known = state.known.contains(&found.text);
            let origin = found.origin.clone();
            row(
                ("localization-string-row", index),
                icon,
                found.text.replace('\n', " "),
                found.context.clone(),
                line.map(|line| format!("Ln {line}")),
            )
            .when(known, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_color(tokens::text3())
                        .child("In a table"),
                )
            })
            .on_click(cx.listener(move |shell, _, window, cx| match origin {
                Origin::Gui(at) => shell.select(at, cx),
                Origin::Script(at, line) => {
                    let pos = lsp_types::Position::new(line.saturating_sub(1) as u32, 0);
                    shell.reveal(at, lsp_types::Range::new(pos, pos), window, cx);
                }
            }))
            .into_any_element()
        });
        v_flex().w_full().children(rows).into_any_element()
    }
}

/// One list row: an icon, the main text, the path it came from, and an
/// optional trailing note.
fn row(
    id: impl Into<ElementId>,
    icon: IconName,
    text: String,
    path: String,
    note: Option<String>,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .gap_2()
        .px_2()
        .py_0p5()
        .rounded(tokens::radius())
        .text_size(tokens::text_xs())
        .cursor_pointer()
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .child(Icon::new(icon).small().text_color(match icon {
            IconName::TriangleAlert => tokens::warning(),
            _ => tokens::text3(),
        }))
        .child(
            div()
                .flex_none()
                .max_w(px(260.))
                .truncate()
                .text_color(tokens::text_strong())
                .child(text),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(tokens::text3())
                .child(path),
        )
        .children(note.map(|note| div().flex_none().text_color(tokens::text3()).child(note)))
}

fn caption(text: String) -> impl IntoElement {
    div()
        .px_2()
        .text_size(tokens::text_xs())
        .text_color(tokens::text2())
        .child(text)
}

fn empty(text: &'static str) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .text_size(tokens::text_xs())
        .text_color(tokens::text3())
        .child(text)
        .into_any_element()
}
