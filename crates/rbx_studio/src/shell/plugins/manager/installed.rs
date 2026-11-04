//! The Manage Plugins dialog's Installed tab: a search field, a card per
//! plugin found that matches it, a card leading to Popular plugins, then
//! the Studio folder switch.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::super::catalog::view::{row_frame, search_field, BODY_HEIGHT};
use super::super::catalog::Tab;
use super::{kind_label, load_prefs, save_prefs, switch};
use crate::launcher::ui;
use crate::shell::Shell;
use crate::tokens;

impl Shell {
    /// The Installed tab: the search field, a card per plugin found that
    /// matches it (switch, name, kind and creator, status), a card leading
    /// to Popular plugins, then the Studio folder switch.
    pub(super) fn installed_body(&mut self, cx: &mut Context<Self>) -> Div {
        let load_studio = load_prefs().load_studio;
        let query = self
            .plugins
            .filter
            .query
            .as_ref()
            .map(|query| query.read(cx).value().trim().to_owned())
            .unwrap_or_default();
        let creators = self.catalog_creators();
        let mut rows = v_flex()
            .id("plugin-installed-list")
            .size_full()
            .gap(px(6.))
            .overflow_y_scroll();
        let mut shown = 0;
        for (index, found) in self.plugins.found.iter().enumerate() {
            let creator = found
                .path
                .file_name()
                .and_then(|file| creators.get(&*file.to_string_lossy()).copied())
                .filter(|_| !found.from_studio);
            let mut tag = kind_label(&found.kind).to_string();
            if let Some(creator) = creator {
                tag.push_str(&format!(" \u{b7} by {creator}"));
            }
            if found.from_studio {
                tag.push_str(" \u{b7} from Studio");
            }
            if !matches(&query, &[&found.name, creator.unwrap_or_default(), &tag]) {
                continue;
            }
            shown += 1;
            let key = found.path.to_string_lossy().to_string();
            let disabled = self.plugins.disabled.contains(&key);
            let (status, is_error) = self.plugin_status(found, disabled);
            let toggle = switch(format!("plugin-switch-{index}").into(), !disabled).on_click(
                cx.listener(move |shell, _, _, cx| {
                    let mut prefs = load_prefs();
                    if !shell.plugins.disabled.remove(&key) {
                        shell.plugins.disabled.insert(key.clone());
                    }
                    let mut names: Vec<String> = shell.plugins.disabled.iter().cloned().collect();
                    names.sort();
                    prefs.disabled = names;
                    save_prefs(&prefs);
                    cx.notify();
                }),
            );
            rows = rows.child(
                row_frame(("plugin-installed-row", index))
                    .child(toggle)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(tokens::text_md())
                                    .line_height(tokens::line_md())
                                    .font_weight(tokens::WEIGHT_SEMIBOLD)
                                    .text_color(tokens::text())
                                    .child(found.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(tokens::text_sm())
                                    .line_height(tokens::line_sm())
                                    .text_color(tokens::text2())
                                    .child(tag),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(260.))
                            .truncate()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(if is_error {
                                tokens::text_error()
                            } else {
                                tokens::text2()
                            })
                            .child(status),
                    ),
            );
        }
        if shown == 0 && !query.is_empty() {
            rows = rows.child(
                v_flex()
                    .flex_1()
                    .min_h(px(120.))
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .text_color(tokens::text3())
                    .child(ui::icon("search", 18.))
                    .child(
                        div()
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(tokens::text())
                            .child(format!(
                                "No installed plugins match \u{201c}{query}\u{201d}"
                            )),
                    )
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(tokens::text2())
                            .child("Search by name, creator or kind."),
                    ),
            );
        }
        let browse = row_frame("plugin-installed-browse")
            .border_dashed()
            .bg(gpui_kit::transparent_black())
            .cursor_pointer()
            .text_color(tokens::text2())
            .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
            .on_click(cx.listener(|shell, _, window, cx| {
                shell.catalog_show(Tab::Popular, window, cx);
            }))
            .child(
                h_flex()
                    .size(px(40.))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::radius_tile())
                    .bg(tokens::tile())
                    .child(ui::icon("store", 18.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .child(
                        div()
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(tokens::text())
                            .child(if self.plugins.found.is_empty() {
                                "No plugins yet"
                            } else {
                                "Want more?"
                            }),
                    )
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .child(
                                "Install free plugins checked to run here from Popular plugins.",
                            ),
                    ),
            )
            .child(ui::icon("chevron-right", 14.));
        let studio_row = row_frame("plugin-load-studio-row")
            .child(
                switch("plugin-load-studio".into(), load_studio).on_click(cx.listener(
                    |_, _, _, cx| {
                        let mut prefs = load_prefs();
                        prefs.load_studio = !prefs.load_studio;
                        save_prefs(&prefs);
                        cx.notify();
                    },
                )),
            )
            .child(
                v_flex()
                    .flex_1()
                    .child(
                        div()
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(tokens::text())
                            .child("Load Roblox Studio plugins"),
                    )
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(tokens::text2())
                            .child(
                                "Also runs the plugins in Studio's own folder. Applies on Reload.",
                            ),
                    ),
            );
        let hint = if query.is_empty() {
            String::new()
        } else {
            format!("{shown} of {}", self.plugins.found.len())
        };
        v_flex()
            .h(px(BODY_HEIGHT))
            .gap(px(10.))
            .children(
                self.plugins
                    .filter
                    .query
                    .as_ref()
                    .map(|field| search_field(field, hint.into())),
            )
            .child(div().flex_1().min_h_0().child(rows.child(browse)))
            .child(studio_row)
    }
}

/// True when every word of `query` is in one of `fields`, ignoring case.
fn matches(query: &str, fields: &[&str]) -> bool {
    let fields: Vec<String> = fields.iter().map(|field| field.to_lowercase()).collect();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| fields.iter().any(|field| field.contains(word)))
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn every_word_must_match_some_field() {
        let fields = [
            "Building Tools by F3X",
            "GigsD4X",
            "Roblox \u{b7} by GigsD4X",
        ];
        assert!(matches("", &fields));
        assert!(matches("f3x", &fields));
        assert!(matches("  BUILDING   gigs ", &fields));
        assert!(matches("roblox", &fields));
        assert!(!matches("f3x archimedes", &fields));
    }
}
