//! The Manage Plugins dialog's Installed tab: a card per plugin found, a
//! card leading to Popular plugins, then the Studio folder switch.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::super::catalog::view::{row_frame, BODY_HEIGHT};
use super::super::catalog::Tab;
use super::{kind_label, load_prefs, save_prefs, switch};
use crate::launcher::ui;
use crate::shell::Shell;
use crate::tokens;

impl Shell {
    /// The Installed tab: a card per plugin found (switch, name, kind,
    /// status), a card leading to Popular plugins, then the Studio folder
    /// switch.
    pub(super) fn installed_body(&mut self, cx: &mut Context<Self>) -> Div {
        let load_studio = load_prefs().load_studio;
        let mut rows = v_flex()
            .id("plugin-installed-list")
            .size_full()
            .gap(px(6.))
            .overflow_y_scroll();
        for (index, found) in self.plugins.found.iter().enumerate() {
            let key = found.path.to_string_lossy().to_string();
            let disabled = self.plugins.disabled.contains(&key);
            let (status, is_error) = self.plugin_status(found, disabled);
            let tag = if found.from_studio {
                format!("{} \u{b7} from Studio", kind_label(&found.kind))
            } else {
                kind_label(&found.kind).to_string()
            };
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
        v_flex()
            .h(px(BODY_HEIGHT))
            .gap(px(10.))
            .child(div().flex_1().min_h_0().child(rows.child(browse)))
            .child(studio_row)
    }
}
