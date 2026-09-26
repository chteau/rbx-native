//! Create a key: the Dashboard walk-through carousel and the settings to
//! use.

use super::ui::{self, Weight};
use crate::tokens;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use std::sync::{Arc, OnceLock};

use super::*;

impl Wizard {
    pub(super) fn create(&self, cx: &mut Context<Self>) -> AnyElement {
        const SLIDES: [(&str, &str); 3] = [
            (
                "Open API Keys and press Create API Key",
                "Creator Dashboard \u{2192} Credentials \u{2192} API Keys. Name it RbxNative.",
            ),
            (
                "Add the three required permissions",
                "universe-places \u{2192} Write, legacy-asset \u{2192} Manage, then Inventory \u{2192} Read.",
            ),
            (
                "Save & Generate, then copy the key",
                "Set Security (your IP, an expiration) first. Roblox shows the key once.",
            ),
        ];
        let slide = self.slide;
        let (title, sub) = SLIDES[slide];
        let arrow = |id: &'static str, glyph: &'static str, enabled: bool, left: bool| {
            h_flex()
                .id(id)
                .absolute()
                .top(px(76.))
                .when(left, |this| this.left(px(12.)))
                .when(!left, |this| this.right(px(12.)))
                .size(px(32.))
                .rounded_full()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(tokens::border2())
                .bg(ui::panel())
                .text_color(if enabled {
                    tokens::text()
                } else {
                    tokens::text3()
                })
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .hover(|this| tokens::hover_fx(this).bg(tokens::secondary_hover()))
                })
                .child(ui::icon(glyph, 16.))
        };
        let row = |label: &'static str, value: AnyElement, last: bool| {
            h_flex()
                .min_h(px(34.))
                .items_center()
                .gap(px(12.))
                .py(px(7.))
                .px(px(14.))
                .when(!last, |this| {
                    this.border_b_1().border_color(tokens::border())
                })
                .child(
                    ui::text(12., 18.)
                        .w(px(120.))
                        .flex_none()
                        .text_color(tokens::text2())
                        .child(label),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(tokens::text())
                        .child(value),
                )
        };
        let chip = |label: &'static str| {
            h_flex()
                .h(px(20.))
                .px(px(6.))
                .items_center()
                .rounded(px(4.))
                .border_1()
                .border_color(tokens::border())
                .bg(ui::panel())
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(px(11.))
                .line_height(px(16.))
                .child(label)
        };
        v_flex()
            .gap(px(18.))
            .child(Self::heading(
                "Create an API key",
                "Follow along on the Creator Dashboard. Keep this window open.",
                Some(
                    ui::external_button("wizard-dashboard", "Open Creator Dashboard", Weight::Secondary, false)
                        .on_click(|_, _, cx| cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL))
                        .into_any_element(),
                ),
            ))
            .child(
                v_flex()
                    .flex_none()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::panel2())
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .h(px(184.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .py(px(14.))
                            .px(px(60.))
                            .bg(rgb(0x131314))
                            .border_b_1()
                            .border_color(tokens::border())
                            .children(slide_image(slide).map(|image| {
                                img(image).max_w_full().max_h_full().object_fit(ObjectFit::Contain).rounded(px(4.))
                            }))
                            .child(arrow("slide-prev", "chevron-left", slide > 0, true).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.slide = this.slide.saturating_sub(1);
                                    cx.notify();
                                }),
                            ))
                            .child(arrow("slide-next", "chevron-right", slide < 2, false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.slide = (this.slide + 1).min(2);
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        h_flex()
                            .h(px(56.))
                            .items_center()
                            .gap(px(12.))
                            .px(px(16.))
                            .child(
                                ui::mono(11., 16.)
                                    .flex_none()
                                    .text_color(tokens::text3())
                                    .child(format!("{} / 3", slide + 1)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(1.))
                                    .child(
                                        ui::text(12.5, 17.)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(tokens::text())
                                            .child(title),
                                    )
                                    .child(ui::text(11.5, 16.).truncate().text_color(tokens::text2()).child(sub)),
                            )
                            .child(h_flex().flex_none().gap(px(5.)).children((0..3).map(|i| {
                                div()
                                    .w(px(if i == slide { 16. } else { 6. }))
                                    .h(px(6.))
                                    .rounded(px(3.))
                                    .bg(if i == slide { ui::accent() } else { tokens::border2() })
                            }))),
                    ),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        ui::text(10., 14.)
                            .h(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text3())
                            .child("USE THESE SETTINGS"),
                    )
                    .child(
                        v_flex()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(tokens::border())
                            .child(row("Name", chip("RbxNative").into_any_element(), false))
                            .child(row(
                                "Required",
                                h_flex()
                                    .gap(px(6.))
                                    .child(chip("universe-places:write"))
                                    .child(chip("legacy-asset:manage"))
                                    .child(chip("user.inventory-item:read"))
                                    .into_any_element(),
                                false,
                            ))
                            .child(row(
                                "Optional",
                                h_flex()
                                    .gap(px(6.))
                                    .min_w_0()
                                    .child(div().truncate().text_color(tokens::text2()).child(
                                        "Anything from the list on the next step. Each one switches a feature on.",
                                    ))
                                    .into_any_element(),
                                false,
                            ))
                            .child(row(
                                "Experiences",
                                v_flex()
                                    .gap(px(2.))
                                    .child(
                                        h_flex()
                                            .gap(px(6.))
                                            .child(ui::tag("RECOMMENDED", ui::accent(), tokens::accent_soft()))
                                            .child("Restrict by Experience on, and pick your games."),
                                    )
                                    .child(div().text_color(tokens::text2()).child(
                                        "The key can\u{2019}t touch anything else.",
                                    ))
                                    .child(div().text_color(tokens::text3()).child(
                                        "Leaving it off reaches every current and future experience.",
                                    ))
                                    .into_any_element(),
                                false,
                            ))
                            .child(row(
                                "Security",
                                h_flex()
                                    .gap(px(6.))
                                    .child(ui::tag("RECOMMENDED", ui::accent(), tokens::accent_soft()))
                                    .child(div().text_color(tokens::text2()).child(
                                        "Allow only your IP and set an expiration, so a copied key is useless elsewhere.",
                                    ))
                                    .into_any_element(),
                                true,
                            )),
                    ),
            )
            .into_any_element()
    }
}

/// The carousel's Dashboard screenshots (the key in the third is blurred
/// at the source), decoded once.
fn slide_image(index: usize) -> Option<Arc<RenderImage>> {
    static SLIDES: OnceLock<Vec<Option<Arc<RenderImage>>>> = OnceLock::new();
    const PNGS: [&[u8]; 3] = [
        include_bytes!("../../../../../assets/launcher/dashboard-create-key.png"),
        include_bytes!("../../../../../assets/launcher/dashboard-permissions.png"),
        include_bytes!("../../../../../assets/launcher/dashboard-copy-key.png"),
    ];
    SLIDES
        .get_or_init(|| {
            PNGS.iter()
                .map(|bytes| {
                    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    crate::render_image::to_render_image(rgba.into_raw(), w, h)
                })
                .collect()
        })
        .get(index)
        .cloned()
        .flatten()
}
