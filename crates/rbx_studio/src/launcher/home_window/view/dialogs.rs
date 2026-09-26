//! Home's dialogs: LocalCopy, Downloading, DownloadError.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::*;
use crate::home::RecentPlace;
use crate::launcher::home_window::{Dialog, HomeWindow};
use crate::launcher::ui::{self, Weight};
use crate::tokens;

impl HomeWindow {
    pub(super) fn dialog_view(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.dialog.as_ref()?;
        Some(match dialog {
            Dialog::LocalCopy {
                experience,
                path,
                replace,
            } => {
                let replace = *replace;
                let radio = |id: &'static str,
                             glyph: &'static str,
                             title: &'static str,
                             text: &'static str,
                             on: bool,
                             value: bool,
                             cx: &mut Context<Self>| {
                    h_flex()
                        .id(id)
                        .items_center()
                        .gap(px(12.))
                        .py(px(12.))
                        .px(px(14.))
                        .rounded(px(8.))
                        .border_1()
                        .cursor_pointer()
                        .map(|this| {
                            if on {
                                this.border_color(tokens::accent_line())
                                    .bg(tokens::accent_soft())
                            } else {
                                this.border_color(tokens::border())
                                    .bg(ui::panel2())
                                    .hover(|this| {
                                        tokens::hover_fx(this).bg(tokens::secondary_hover())
                                    })
                            }
                        })
                        .child(div().size(px(16.)).flex_none().rounded_full().map(|this| {
                            if on {
                                this.border(px(5.)).border_color(ui::accent())
                            } else {
                                this.border_1().border_color(tokens::border2())
                            }
                        }))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .text_size(px(12.5))
                                        .line_height(px(17.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(tokens::text())
                                        .child(ui::icon(glyph, 13.).text_color(tokens::text2()))
                                        .child(title),
                                )
                                .child(ui::text(11.5, 17.).text_color(tokens::text2()).child(text)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(Dialog::LocalCopy { replace, .. }) = &mut this.dialog {
                                *replace = value;
                            }
                            cx.notify();
                        }))
                };
                let saved = std::fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .map(|time| {
                        let secs = time
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        format!(" \u{b7} saved {}", opened_label(secs).to_lowercase())
                    })
                    .unwrap_or_default();
                let body = v_flex()
                    .child(
                        // Only the path gives way: the saved time stays whole.
                        h_flex()
                            .mt(px(12.))
                            .mx(px(20.))
                            .ml(px(78.))
                            .min_w_0()
                            .overflow_hidden()
                            .font_family(tokens::FONT_FAMILY_MONO)
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(tokens::text3())
                            .child(div().flex_1().min_w_0().truncate().child(display_path(path)))
                            .child(div().flex_shrink_0().whitespace_nowrap().child(saved)),
                    )
                    .child(
                        v_flex()
                            .gap(px(8.))
                            .pt(px(16.))
                            .px(px(20.))
                            .child(radio("localcopy-keep", "folder-open", "Open my local copy", "Keep working where you left off.", !replace, false, cx))
                            .child(radio(
                                "localcopy-replace",
                                "download",
                                "Download the published version",
                                "Replaces your local copy. Changes you haven\u{2019}t published are lost.",
                                replace,
                                true,
                                cx,
                            )),
                    )
                    .into_any_element();
                let experience = experience.clone();
                let path = path.clone();
                ui::dialog(
                    520.,
                    self.icon_box(&experience, 44., 8.).into_any_element(),
                    format!("{} is already on this computer", experience.name),
                    "You opened it before. Your local copy may have changes you haven\u{2019}t published.",
                    Some(body),
                    vec![
                        ui::button("localcopy-cancel", "Cancel", Weight::Secondary, false)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)))
                            .into_any_element(),
                        ui::button(
                            "localcopy-go",
                            if replace { "Download and replace" } else { "Open local copy" },
                            Weight::Primary,
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if replace {
                                this.download(experience.clone(), true, cx);
                            } else {
                                this.dialog = None;
                                let _ = crate::home::remember(RecentPlace {
                                    path: path.clone(),
                                    universe_id: Some(experience.universe_id),
                                    place_id: Some(experience.root_place_id),
                                    name: Some(experience.name.clone()),
                                    opened: None,
                                });
                                this.open_path_later(path.clone(), cx);
                            }
                        }))
                        .into_any_element(),
                    ],
                )
                .into_any_element()
            }
            Dialog::Downloading { experience } => {
                let body = v_flex()
                    .gap(px(8.))
                    .pt(px(18.))
                    .pr(px(20.))
                    .pl(px(78.))
                    .child(indeterminate_bar())
                    .child(
                        h_flex()
                            .justify_between()
                            .font_family(tokens::FONT_FAMILY_MONO)
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(tokens::text3())
                            .child("Downloading\u{2026}"),
                    )
                    .into_any_element();
                ui::dialog(
                    520.,
                    self.icon_box(experience, 44., 8.).into_any_element(),
                    format!("Opening {}", experience.name),
                    "Downloading the published place. It opens in the editor when it\u{2019}s done.",
                    Some(body),
                    vec![ui::button("download-cancel", "Cancel", Weight::Secondary, false)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)))
                        .into_any_element()],
                )
                .into_any_element()
            }
            Dialog::Error {
                experience,
                title,
                status,
                reason,
            } => {
                let status_line = status.map(|status| {
                    format!(
                        "{status} {}",
                        match status {
                            401 => "Unauthorized",
                            403 => "Forbidden",
                            404 => "Not Found",
                            429 => "Too Many Requests",
                            _ => "",
                        }
                    )
                });
                let (run, red_len) = match &status_line {
                    Some(line) => (format!("{line} \u{b7} {reason}"), line.len()),
                    None => (reason.clone(), 0),
                };
                let body = div()
                    .mt(px(14.))
                    .mr(px(20.))
                    .ml(px(78.))
                    .py(px(10.))
                    .px(px(12.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::bg())
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(px(11.))
                    .line_height(px(17.))
                    .text_color(tokens::text2())
                    .child(StyledText::new(run).with_highlights([(
                        0..red_len,
                        HighlightStyle {
                            color: Some(ui::red().into()),
                            ..Default::default()
                        },
                    )]))
                    .into_any_element();
                let auth = matches!(status, Some(401 | 403));
                let retry = experience.clone();
                let mut footer = Vec::new();
                if auth {
                    footer.push(
                        ui::icon_button(
                            "error-manage",
                            "key-round",
                            "Manage key",
                            Weight::Secondary,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx)))
                        .into_any_element(),
                    );
                }
                footer.push(
                    ui::button("error-close", "Close", Weight::Secondary, false)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)))
                        .into_any_element(),
                );
                if let Some(retry) = retry {
                    footer.push(
                        ui::button("error-retry", "Try again", Weight::Primary, false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.download(retry.clone(), true, cx)
                            }))
                            .into_any_element(),
                    );
                }
                ui::dialog(
                    520.,
                    ui::dialog_glyph("circle-alert", ui::red(), ui::red_soft()),
                    title.clone(),
                    if experience.is_some() {
                        "Roblox refused the download. Your local copy, if you have one, wasn\u{2019}t changed."
                    } else {
                        "Nothing was changed."
                    },
                    Some(body),
                    footer,
                )
                .into_any_element()
            }
        })
    }
}
