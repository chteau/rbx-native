//! One search result: the package, then the realm switch, the version
//! picker and Add — beside it, or under it in two rows when the page is
//! narrow.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;
use crate::wally_client::{Realm, SearchResult};

use super::super::menu::{self, MenuId};
use super::super::wally_sync::package_id;
use super::super::{chrome, Shell};
use super::cards::name_span;
use super::Layout;

impl Shell {
    pub(super) fn result_card(
        &mut self,
        index: usize,
        result: &SearchResult,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = package_id(&result.scope, &result.name);
        let pick = self.wally_pick(&id);
        // The package's own realm, once its metadata is in; Shared until
        // then, which every section accepts.
        let package_realm = self
            .wally_listing(&id)
            .map(|listing| listing.realm)
            .unwrap_or_default();
        let realm = pick.realm.unwrap_or(package_realm);
        let mut versions: Vec<semver::Version> = result
            .versions
            .iter()
            .filter_map(|text| semver::Version::parse(text).ok())
            .collect();
        versions.sort_by(|a, b| b.cmp(a));

        let about = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(4.))
            .child(name_span(
                &result.scope,
                &result.name,
                tokens::text_action(),
            ))
            .children(result.description.clone().map(|text| {
                div()
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text2())
                    .child(text)
            }));
        let switch = self.realm_switch(
            index,
            id.clone(),
            realm,
            package_realm,
            layout.stack_result,
            cx,
        );
        let picker = self.version_picker(
            index,
            id.clone(),
            &versions,
            pick.version.as_ref(),
            layout.stack_result,
            cx,
        );
        let add = {
            let (scope, name) = id.clone();
            let version = pick.version.clone();
            h_flex()
                .id(("wally-add", index))
                .tab_index(self.tab_order.next())
                .flex_none()
                .h(px(28.))
                .px(px(14.))
                .gap(px(6.))
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
                    shell.wally_install(scope.clone(), name.clone(), version.clone(), realm, cx);
                }))
                .child(Icon::new(IconName::Plus).size(px(12.)))
                .child("Add")
        };

        let frame = |direction: Div| {
            direction
                .w_full()
                .px(px(14.))
                .py(px(12.))
                .rounded(tokens::RADIUS_TILE)
                .bg(tokens::field_select())
                .border_1()
                .border_color(tokens::accent_line())
        };
        if layout.stack_result {
            frame(v_flex().gap(px(10.)))
                .child(about)
                .child(
                    v_flex()
                        .gap(px(8.))
                        .child(switch)
                        .child(h_flex().gap(px(8.)).items_center().child(picker).child(add)),
                )
                .into_any_element()
        } else {
            frame(h_flex().gap(px(16.)).items_center())
                .child(about)
                .child(
                    h_flex()
                        .flex_none()
                        .gap(px(8.))
                        .items_center()
                        .child(switch)
                        .child(picker)
                        .child(add),
                )
                .into_any_element()
        }
    }

    /// Shared / Server / Dev, 22px segments on a 2px-padded track. A
    /// section that can't take the package (Wally's rule, `Realm::
    /// accepts`) is `text3`, not focusable, no hover. Full width on
    /// `panel` when stacked, since the card is already `panel2`.
    fn realm_switch(
        &mut self,
        index: usize,
        id: (String, String),
        current: Realm,
        package_realm: Realm,
        stacked: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let segments = Realm::ALL.map(|realm| {
            let selected = realm == current;
            let allowed = realm.accepts(package_realm);
            let id = id.clone();
            h_flex()
                .id(("wally-realm", index * 3 + realm as usize))
                .h(px(22.))
                .px(px(10.))
                .items_center()
                .justify_center()
                .when(stacked, |this| this.flex_1())
                .rounded(tokens::RADIUS_SEGMENT)
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .map(|this| {
                    if selected {
                        this.bg(tokens::accent_soft())
                            .text_color(tokens::check_on())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                    } else if allowed {
                        this.tab_index(self.tab_order.next())
                            .cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| this.bg(tokens::hover()))
                            .focus_visible(|this| {
                                this.shadow(tokens::focus_ring(tokens::field_select()))
                            })
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.wally_pick_realm(id.clone(), realm, cx);
                            }))
                    } else {
                        this.text_color(tokens::text3())
                    }
                })
                .child(realm.label())
        });
        h_flex()
            .when(stacked, |this| this.w_full())
            .flex_none()
            .p(px(2.))
            .gap(px(2.))
            .rounded(tokens::RADIUS)
            .bg(if stacked {
                tokens::dock()
            } else {
                tokens::field_select()
            })
            .children(segments)
    }

    /// 112×28 (flexible when stacked): "Latest" or the picked version,
    /// and a chevron; the menu lists Latest then every version, newest
    /// first.
    fn version_picker(
        &mut self,
        index: usize,
        id: (String, String),
        versions: &[semver::Version],
        picked: Option<&semver::Version>,
        stacked: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let label = picked
            .map(|version| format!("v{version}"))
            .unwrap_or_else(|| "Latest".to_owned());
        let mut items = vec![menu::item("Latest").checked(picked.is_none()).on_click({
            let id = id.clone();
            move |shell, cx| shell.wally_pick_version(id.clone(), None, cx)
        })];
        for version in versions {
            let id = id.clone();
            let version = version.clone();
            items.push(
                menu::item(format!("v{version}"))
                    .checked(picked == Some(&version))
                    .on_click(move |shell, cx| {
                        shell.wally_pick_version(id.clone(), Some(version.clone()), cx)
                    }),
            );
        }
        let trigger = h_flex()
            .id(("wally-version", index))
            .tab_index(self.tab_order.next())
            .map(|this| {
                if stacked {
                    this.w_full()
                } else {
                    this.flex_none().w(px(112.))
                }
            })
            .h(px(28.))
            .px(px(9.))
            .items_center()
            .justify_between()
            .rounded(tokens::RADIUS)
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border())
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text2())
            .cursor_pointer()
            .hover(|this| this.border_color(tokens::border2()))
            .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::field_select())))
            .child(div().min_w_0().truncate().child(label))
            .child(Icon::new(IconName::ChevronDown).size(px(10.)));
        // The popover wraps its trigger in a box of its own, so the
        // stretch goes on that box: the trigger fills it.
        div()
            .map(|this| {
                if stacked {
                    this.flex_1().min_w_0()
                } else {
                    this.flex_none()
                }
            })
            .child(menu::dropdown(
                self,
                MenuId::WallyVersion(index),
                chrome::Trigger::new(trigger),
                items,
                cx,
            ))
    }
}
