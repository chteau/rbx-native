//! The Wally dock: search the registry and add a package to the place.
//!
//! Originally one of the Script Editor's two docks: Argon (`argon-rbx/argon`) two-way file
//! sync and the Wally (`UpliftGames/wally`) package manager — the two
//! scripting tools `ROADMAP.md` names, seated beside Output so a scripter
//! never has to leave the bottom edge to reach them.
//!
//! Both are real clients, not placeholders. Argon's talks
//! `argon-rbx/argon`'s sync protocol (`crate::argon_client`, wired in here
//! by `shell::argon_sync`): Connect really opens an HTTP connection to a
//! locally-running `argon serve`, and the dock's states below
//! (`NotConnected`/`Connecting`/`Connected`/`Error`, plus the batch review
//! prompt) mirror Argon's own Studio plugin's state machine — drawn with
//! this editor's own chrome, tokens and `Button`, not Argon's. Wally's
//! searches the real `api.wally.run` registry and resolves a picked
//! result's whole dependency graph (`crate::wally_client`, wired in here by
//! `shell::wally_sync`).
//!
//! Shown only while the Script Editor document is up — see
//! `Shell::hidden_panels` — since neither means anything over the 3D view
//! or the UI Editor's canvas.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit::*;

use crate::tokens;
use crate::wally_client;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::wally_sync;
use super::workspace::search_field;
use super::Shell;

impl Shell {
    pub(super) fn wally_dock(
        &self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::WallyOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "wally-overflow",
                IconName::Package,
                "Wally dock options",
            )),
            self.move_items(Panel::Wally),
            cx,
        );

        let tab_index = self.tab_order.next();
        // Capped rather than scrolled — a dock this short has no real room
        // for a long list anyway, and a search narrows results faster than
        // scrolling would.
        let rows: Vec<AnyElement> = self
            .wally_results()
            .iter()
            .take(10)
            .enumerate()
            .map(|(index, result)| {
                let picked = result.clone();
                wally_result_row(
                    ("wally-result", index),
                    result,
                    cx.listener(move |shell, _, _, cx| {
                        shell.wally_install(picked.clone(), cx);
                    }),
                )
                .into_any_element()
            })
            .collect();

        let body = v_flex()
            .size_full()
            .gap(tokens::group_gap())
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .child(div().w_full().max_w(px(260.)).child(search_field(
                tab_index,
                &self.wally_query,
                cx,
            )))
            .children(wally_status(self.wally_install_state()))
            .child(v_flex().gap(px(2.)).children(rows));

        (
            Some(overflow.into_any_element()),
            Some(chrome::dock_content(body).into_any_element()),
        )
    }
}

/// One search result: `scope/name`, its description if it has one, a
/// click target for the whole row.
fn status_row(icon: IconName, status: impl Into<SharedString>) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap(tokens::label_gap())
        .child(Icon::new(icon).small().text_color(tokens::text_muted()))
        .child(div().text_color(tokens::text_label()).child(status.into()))
}

fn wally_result_row(
    id: impl Into<ElementId>,
    result: &wally_client::SearchResult,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    v_flex()
        .id(id.into())
        .w_full()
        .rounded(tokens::RADIUS)
        .px(tokens::label_gap())
        .py(px(4.))
        .cursor_pointer()
        .hover(|this| this.bg(tokens::hover()))
        .on_click(on_click)
        .child(
            div()
                .text_color(tokens::text_label())
                .child(format!("{}/{}", result.scope, result.name)),
        )
        .children(result.description.clone().map(|description| {
            div()
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .text_color(tokens::text_muted())
                .child(description)
        }))
}

fn wally_status(state: &wally_sync::InstallState) -> Option<AnyElement> {
    Some(match state {
        wally_sync::InstallState::Idle => return None,
        wally_sync::InstallState::Installing { name } => {
            status_row(IconName::LoaderCircle, format!("Installing {name}…")).into_any_element()
        }
        wally_sync::InstallState::Installed { name, count } => {
            let text = match count {
                1 => format!("Installed {name}"),
                n => format!("Installed {name} and {} more", n - 1),
            };
            status_row(IconName::CircleCheck, text).into_any_element()
        }
        wally_sync::InstallState::Error(message) => v_flex()
            .gap(tokens::label_gap())
            .child(status_row(IconName::CircleAlert, "Couldn't install"))
            .child(
                div()
                    .text_color(tokens::text_error())
                    .child(message.clone()),
            )
            .into_any_element(),
    })
}
