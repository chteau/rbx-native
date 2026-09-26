//! Searching the settings: every page's rows that match the query in their
//! label, their description or their key in `settings.json`, grouped by
//! page, live, with the match marked. The pages build their rows as usual;
//! this keeps the ones that match.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings::argon::Setting;
use crate::tokens;

use super::kit::{self, key_hint, text, Row, Section};
use super::nav::Page;
use super::SettingsWindow;

/// Each row's key in `settings.json`, by its label: what a search for
/// `snap_to_parts` finds.
fn key(label: &str) -> Option<String> {
    let key = match label {
        "Graphics quality" => "quality",
        "Frame rate when unfocused" => "unfocused_fps",
        "Orientation indicator" => "axis_indicator",
        "Selection box behind geometry" => "selection_occluded",
        "Light guides" => "light_guides",
        "Orthographic camera" => "orthographic",
        "Mouse sensitivity" => "camera.sensitivity",
        "Camera speed" => "camera.speed",
        "Smoothing" => "camera.smoothing",
        "Hover ruler" => "dragger.show_hover_ruler",
        "Target snap" => "dragger.show_target_snap",
        "Measurement" => "dragger.show_measurement",
        "Dragged point" => "dragger.show_dragged_point",
        "Snap to parts" => "dragger.snap_to_parts",
        "Align dragged objects" => "dragger.align_dragged_objects",
        "Move increment" => "snap.move",
        "Rotate increment" => "snap.rotate",
        "Show all services" => "show_all_services",
        "Increment names" => "increment_names",
        "Expand to selection" => "expand_on_select",
        "Timestamps" => "output_timestamps",
        "Dock layout" => "docks",
        "Output panel" => "output_collapsed",
        "Reduce motion" => "reduce_motion",
        "Large click targets" => "large_targets",
        "UI scale" => "font_scale",
        "Installed themes" => "theme",
        "Icon pack" => "icon_pack",
        "Tool colours" => "tools",
        "Server address" => "argon_address",
        _ => {
            let (setting, _) = Setting::ALL
                .iter()
                .map(|setting| (setting, super::super::argon_dock::settings::copy(*setting)))
                .find(|(_, (name, _))| *name == label)?;
            return Some(format!("argon.{}", setting.key()));
        }
    };
    Some(key.to_owned())
}

/// Whether `row` matches `query`, ignoring case.
fn matches(row: &Row, query: &str) -> bool {
    let query = query.to_lowercase();
    let has = |text: &str| text.to_lowercase().contains(&query);
    has(row.label)
        || row.description.as_ref().is_some_and(|d| has(d))
        || key(row.label).is_some_and(|key| has(&key))
}

/// One page's matching rows, and the titles of the sections they came from.
struct Group {
    page: Page,
    sections: Vec<&'static str>,
    /// Section heads found by their keywords, shown whole.
    heads: Vec<AnyElement>,
    rows: Vec<Row>,
}

impl SettingsWindow {
    pub(super) fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_owned()
    }

    fn groups(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) -> Vec<Group> {
        let current = self.page;
        let mut groups = Vec::new();
        for page in Page::ALL {
            self.page = page;
            let sections: Vec<Section> = self.sections(window, cx);
            let mut group = Group {
                page,
                sections: Vec::new(),
                heads: Vec::new(),
                rows: Vec::new(),
            };
            for section in sections {
                let lower = query.to_lowercase();
                let keyword = section
                    .keywords
                    .iter()
                    .any(|word| word.contains(lower.as_str()));
                if let Some(head) = section.head.filter(|_| keyword) {
                    group.sections.push(section.title);
                    group.heads.push(head);
                }
                let found: Vec<Row> = section
                    .rows
                    .into_iter()
                    .filter(|row| matches(row, query))
                    .collect();
                if !found.is_empty() && !keyword {
                    group.sections.push(section.title);
                }
                if !found.is_empty() {
                    group.rows.extend(found);
                }
            }
            if !group.rows.is_empty() || !group.heads.is_empty() {
                groups.push(group);
            }
        }
        self.page = current;
        groups
    }

    pub(super) fn search_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.query(cx);
        let groups = self.groups(&query, window, cx);
        self.search_counts = groups
            .iter()
            .map(|group| (group.page, group.rows.len() + group.heads.len()))
            .collect();
        let count: usize = groups
            .iter()
            .map(|group| group.rows.len() + group.heads.len())
            .sum();
        let heading = match count {
            0 => format!("No results for \u{201c}{query}\u{201d}"),
            1 => format!("1 result for \u{201c}{query}\u{201d}"),
            n => format!("{n} results for \u{201c}{query}\u{201d}"),
        };
        let shell = self.shell.clone();
        let groups = groups.into_iter().enumerate().map(|(i, group)| {
            let page = group.page;
            let head = h_flex()
                .id(("search-group", i))
                .h(px(18.))
                .gap(px(6.))
                .items_center()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.page = page;
                    this.search
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    cx.notify();
                }))
                .child(
                    text(10.5, 14.)
                        .font_weight(FontWeight::BOLD)
                        .text_color(tokens::text3())
                        .hover(|this| this.text_color(tokens::text2()))
                        .child(page.label().to_uppercase()),
                )
                .child(
                    div()
                        .text_color(tokens::text3())
                        .child(kit::icon("chevron-right", 10.)),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(tokens::text3())
                        .child(group.sections.join(" \u{b7} ")),
                );
            let section = Section::new("", group.rows);
            v_flex()
                .gap(px(8.))
                .child(head)
                .children(group.heads.into_iter().map(|head| kit::card().child(head)))
                .when(!section.rows.is_empty(), |this| {
                    this.child(kit::section_card(i, section, &shell, Some(&query)))
                })
        });
        v_flex()
            .id("settings-search")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .pt(px(26.))
                    .pr(px(40.))
                    .pb(px(40.))
                    .pl(px(36.))
                    .gap(px(22.))
                    .child(
                        v_flex()
                            .gap(px(4.))
                            .child(text(20., 26.).font_weight(FontWeight::BOLD).child(heading))
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_size(px(12.5))
                                    .line_height(px(18.))
                                    .text_color(tokens::text2())
                                    .child("Change them right here, or open the page they live on.")
                                    .child(key_hint("Esc"))
                                    .child("clears the search."),
                            ),
                    )
                    .children(groups)
                    .when(count > 0, |this| {
                        this.child(text(11.5, 16.).text_color(tokens::text3()).child(
                            "Searches labels, descriptions and the setting\u{2019}s key in settings.json.",
                        ))
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::div;

    use super::{key, matches, Row};

    #[test]
    fn a_row_matches_its_label_description_or_key_ignoring_case() {
        let row = Row::new("Target snap", div())
            .describe("Highlight the face or edge a drag will snap to.");
        assert!(matches(&row, "SNAP"));
        assert!(matches(&row, "edge"));
        assert!(matches(&row, "show_target"));
        assert!(!matches(&row, "rotate"));
    }

    #[test]
    fn an_argon_row_is_found_by_its_plugin_key() {
        assert_eq!(key("Auto Connect").as_deref(), Some("argon.AutoConnect"));
        assert_eq!(
            key("Snap to parts").as_deref(),
            Some("dragger.snap_to_parts")
        );
        assert_eq!(key("Nothing like this"), None);
    }
}
