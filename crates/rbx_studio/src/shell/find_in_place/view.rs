//! Find in Place's panel: the search field with its toggles, the optional
//! Where and Replace fields, the status row and the results tree.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Enter, Escape, MoveDown, MoveUp};
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::scan::File;
use crate::shell::chrome::{self, ScrollbarY as _};
use crate::shell::menu::{self, MenuId};
use crate::shell::rows;
use crate::shell::{Panel, Shell};
use crate::tokens;

mod widgets;

use widgets::{
    clear_button, field_input, hit_row, icon_button, labelled, plural, text_button, toggle,
};

impl Shell {
    pub(in crate::shell) fn find_in_place_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        self.ensure_find_inputs(window, cx);
        self.refresh_find_results(cx);

        let mut items = vec![
            menu::item("Where to find")
                .checked(self.find_place.show_filter)
                .on_click(|shell, cx| {
                    shell.find_place.show_filter = !shell.find_place.show_filter;
                    cx.notify();
                }),
            menu::item("Replace")
                .checked(self.find_place.show_replace)
                .on_click(|shell, cx| {
                    shell.find_place.show_replace = !shell.find_place.show_replace;
                    shell.find_place.confirming = false;
                    cx.notify();
                }),
            menu::separator(),
        ];
        items.extend(self.move_items(Panel::FindInPlace));
        let overflow = menu::dropdown(
            self,
            MenuId::FindInPlaceOverflow,
            chrome::dock_options_trigger(
                "find-in-place-overflow",
                IconName::Ellipsis,
                16.,
                "Find in Place options",
            ),
            items,
            cx,
        );

        let Some(inputs) = &self.find_place.inputs else {
            return (Some(overflow.into_any_element()), None);
        };
        let find = &self.find_place;
        let options = find.options;
        let (query, filter, replacement) = (
            inputs.query.clone(),
            inputs.filter.clone(),
            inputs.replacement.clone(),
        );
        let empty_query = query.read(cx).value().is_empty();

        let search = h_flex()
            .h(px(30.))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded(tokens::radius())
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border())
            .text_color(tokens::text3())
            .capture_action(cx.listener(|shell, _: &MoveUp, window, cx| {
                shell.step_find_selection(false, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &MoveDown, window, cx| {
                shell.step_find_selection(true, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &Enter, window, cx| {
                let row = shell.find_place.selected;
                shell.select_find_row(row, true, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &Escape, window, cx| {
                if std::mem::take(&mut shell.find_place.confirming) {
                    cx.notify();
                } else if let Some(active) = shell.scripts.tabs.active() {
                    shell.focus_script(active, window, cx);
                }
                cx.stop_propagation();
            }))
            .child(Icon::new(IconName::Search).xsmall().flex_none())
            .child(field_input(&query))
            .when(!empty_query, |this| {
                this.child(clear_button("find-clear-query", &query, cx))
            })
            .child(toggle(
                "find-match-case",
                "Aa",
                "Match case",
                false,
                options.match_case,
                cx,
                |o| o.match_case = !o.match_case,
            ))
            .child(toggle(
                "find-whole-word",
                "Ab",
                "Match whole word",
                true,
                options.whole_word,
                cx,
                |o| o.whole_word = !o.whole_word,
            ))
            .child(toggle(
                "find-regex",
                ".*",
                "Use regular expression",
                false,
                options.regex,
                cx,
                |o| o.regex = !o.regex,
            ));

        let results = &find.results;
        let files = results.files.len();
        let status = match (&results.error, query.read(cx).value().is_empty()) {
            (Some(error), _) => error.clone(),
            (None, true) => "Type to search every script in the place".to_owned(),
            (None, false) => format!(
                "{} result{} in {} file{}",
                results.total,
                plural(results.total),
                files,
                plural(files)
            ),
        };
        let can_replace = find.show_replace && results.total > 0 && !self.live_shown();
        let status_row = h_flex()
            .w_full()
            .flex_none()
            .h(px(28.))
            .px(px(8.))
            .gap(px(4.))
            .items_center()
            .text_size(tokens::text_xs())
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(match results.error {
                        Some(_) => tokens::text_error(),
                        None => tokens::text2(),
                    })
                    .child(status),
            )
            .child(icon_button(
                "find-previous",
                IconName::ChevronUp,
                "Previous result",
                cx,
                |shell, window, cx| shell.step_find_selection(false, window, cx),
            ))
            .child(icon_button(
                "find-next",
                IconName::ChevronDown,
                "Next result",
                cx,
                |shell, window, cx| shell.step_find_selection(true, window, cx),
            ))
            .when(find.show_replace, |this| {
                this.child(text_button(
                    "find-replace-one",
                    "Replace",
                    can_replace,
                    cx,
                    |shell, _, cx| shell.replace_find_current(cx),
                ))
                .child(text_button(
                    "find-replace-all",
                    "Replace All",
                    can_replace,
                    cx,
                    |shell, _, cx| {
                        shell.find_place.confirming = true;
                        cx.notify();
                    },
                ))
            });

        let confirm = (find.show_replace && find.confirming).then(|| {
            h_flex()
                .w_full()
                .px(px(8.))
                .pb(px(6.))
                .gap(px(6.))
                .items_center()
                .text_size(tokens::text_xs())
                .text_color(tokens::text())
                .child(div().flex_1().child(format!(
                    "Replace {} match{} in {} script{}? Undo puts them all back.",
                    results.total,
                    if results.total == 1 { "" } else { "es" },
                    files,
                    plural(files)
                )))
                .child(text_button(
                    "find-confirm-yes",
                    "Yes",
                    true,
                    cx,
                    |shell, _, cx| shell.replace_find_all(cx),
                ))
                .child(text_button(
                    "find-confirm-no",
                    "No",
                    true,
                    cx,
                    |shell, _, cx| {
                        shell.find_place.confirming = false;
                        cx.notify();
                    },
                ))
        });

        let rows = find.rows();
        let selected = rows.get(find.selected).copied();
        let mut list = v_flex().w_full();
        for (index, file) in results.files.iter().enumerate() {
            let collapsed = find.collapsed.contains(&file.script);
            list = list.child(self.file_row(file, collapsed, cx));
            if collapsed {
                continue;
            }
            for (at, hit) in file.hits.iter().enumerate() {
                let row = rows.iter().position(|&r| r == (index, at)).unwrap_or(0);
                list = list.child(hit_row(
                    file,
                    at,
                    row,
                    hit,
                    selected == Some((index, at)),
                    cx,
                ));
            }
        }

        let body = v_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_none()
                    .px(px(8.))
                    .pt(px(8.))
                    .gap(px(6.))
                    .child(search)
                    .when(find.show_filter, |this| {
                        this.child(labelled("Where", &filter, cx))
                    })
                    .when(find.show_replace, |this| {
                        this.child(labelled("Replace", &replacement, cx))
                    }),
            )
            .child(status_row)
            .children(confirm)
            .child(
                div()
                    .id("find-in-place-list")
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(tokens::border())
                    .overflow_y_scroll()
                    .track_scroll(&find.scroll)
                    .pr(chrome::SCROLLBAR_GUTTER)
                    .child(list)
                    .scrollbar_y(&find.scroll),
            );
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    fn file_row(&self, file: &File, collapsed: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let script = file.script;
        let icon = crate::explorer::resolve_icon(&file.class, self.icon_pack());
        h_flex()
            .id(("find-file", script.value() as usize))
            .w_full()
            .gap(px(6.))
            .px(px(8.))
            .py(px(3.))
            .items_center()
            .text_size(tokens::text_xs())
            .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
            .on_click(cx.listener(move |shell, _, _, cx| shell.toggle_find_file(script, cx)))
            .child(
                Icon::new(match collapsed {
                    true => IconName::ChevronRight,
                    false => IconName::ChevronDown,
                })
                .xsmall()
                .text_color(tokens::text3()),
            )
            .child(rows::class_icon(icon))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(tokens::text_strong())
                    .child(file.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .rounded(tokens::radius_badge())
                    .bg(tokens::hover_subtle())
                    .text_color(tokens::text2())
                    .child(file.count.to_string()),
            )
    }
}
