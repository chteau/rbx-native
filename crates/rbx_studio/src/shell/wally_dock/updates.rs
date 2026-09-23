//! The Updates page: one row per installed package the registry has a
//! newer version of, with an Update button, or the all-up-to-date state.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::tokens;

use super::super::wally_sync::{Installed, Update};
use super::super::Shell;
use super::cards::{header, name_span};
use super::states::empty_state;
use super::{Layout, GRID_GAP};

impl Shell {
    pub(super) fn updates_page(
        &mut self,
        layout: Layout,
        installed: &[Installed],
        pending: &[Update],
        cx: &mut Context<Self>,
    ) -> (Div, AnyElement) {
        if pending.is_empty() {
            let detail = match installed.len() {
                1 => "Your 1 installed package is on its newest version.".to_owned(),
                n => format!("Your {n} installed packages are on their newest versions."),
            };
            return (
                header("Updates", None),
                empty_state(IconName::CircleCheck, "Everything is up to date", detail)
                    .into_any_element(),
            );
        }
        let rows: Vec<AnyElement> = pending
            .iter()
            .enumerate()
            .map(|(index, update)| self.update_row(index, update, layout, cx))
            .collect();
        (
            header("Updates", Some(format!("{} available", pending.len()))),
            v_flex().gap(px(GRID_GAP)).children(rows).into_any_element(),
        )
    }

    /// `panel2` on a `border` hairline, 10/14 padding: the package, its
    /// versions old → new in mono, and Update at the right.
    fn update_row(
        &mut self,
        index: usize,
        update: &Update,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let package = &update.installed;
        let versions = h_flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .font_family(tokens::FONT_FAMILY_MONO)
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .child(
                div()
                    .text_color(tokens::text2())
                    .child(format!("v{}", package.version)),
            )
            .child(div().text_color(tokens::text3()).child("→"))
            .child(
                div()
                    .text_color(tokens::text())
                    .child(format!("v{}", update.latest)),
            );
        let name = name_span(&package.scope, &package.name, tokens::text_md());
        let about = if layout.stack_update {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(4.))
                .child(name)
                .child(versions)
        } else {
            h_flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(12.))
                .child(name)
                .child(versions)
        };
        let (scope, pkg, latest, realm) = (
            package.scope.clone(),
            package.name.clone(),
            update.latest.clone(),
            package.realm,
        );
        let button = h_flex()
            .id(("wally-update", index))
            .tab_index(self.tab_order.next())
            .flex_none()
            .h(px(28.))
            .px(px(14.))
            .items_center()
            .rounded(tokens::RADIUS)
            .bg(tokens::check_on())
            .text_size(tokens::text_md())
            .line_height(tokens::line_md())
            .font_weight(tokens::WEIGHT_BOLD)
            .text_color(tokens::black())
            .cursor_pointer()
            .hover(|this| this.bg(tokens::accent_hover()))
            .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::field_select())))
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.wally_install(scope.clone(), pkg.clone(), Some(latest.clone()), realm, cx);
            }))
            .child("Update");
        h_flex()
            .id(("wally-update-row", index))
            .w_full()
            .items_center()
            .gap(px(16.))
            .px(px(14.))
            .py(px(10.))
            .rounded(tokens::RADIUS_TILE)
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border())
            .hover(|this| {
                this.border_color(tokens::border2())
                    .bg(tokens::hover_subtle())
            })
            .child(about)
            .child(button)
            .into_any_element()
    }
}
