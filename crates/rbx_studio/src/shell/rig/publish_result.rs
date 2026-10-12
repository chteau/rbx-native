//! The dialog that reports the auto-rigger's Publish: where the model went
//! (or why it did not), in the launcher's dialog chrome. The Output line is
//! kept too; this is for the user who was looking elsewhere when it landed.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::shell::Shell;
use crate::tokens;

/// How many parts a complete rig is made of.
const PARTS: usize = 15;

/// What Publish ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublishOutcome {
    /// Uploaded as model `id`; `swapped` of the 15 parts took their mesh.
    Published {
        id: u64,
        swapped: usize,
    },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishResult {
    pub(crate) name: String,
    pub(crate) outcome: PublishOutcome,
}

/// The page a published model is managed on.
pub(crate) fn asset_url(id: u64) -> String {
    format!("https://create.roblox.com/store/asset/{id}")
}

/// How to get the model into a place.
pub(crate) fn insert_hint(id: u64) -> String {
    format!("Insert it in Studio from Toolbox \u{203a} Inventory, or with InsertService:LoadAsset({id})")
}

/// The warning for a model whose upload lacked some of the parts.
pub(crate) fn partial_warning(swapped: usize) -> Option<String> {
    (swapped < PARTS)
        .then(|| format!("Only {swapped} of {PARTS} parts were found in the upload; the rest still use local meshes."))
}

impl Shell {
    /// Escape's half of the dialog; returns whether it was open.
    pub(in crate::shell) fn close_publish_result(&mut self) -> bool {
        self.anim.publish_result.take().is_some()
    }

    pub(in crate::shell) fn publish_result_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let result = self.anim.publish_result.as_ref()?;
        let close = ui::button("publish-result-close", "Close", Weight::Secondary, false)
            .on_click(cx.listener(|shell, _, _, cx| {
                shell.close_publish_result();
                cx.notify();
            }))
            .into_any_element();
        let inset = |body: Div| body.mt(px(14.)).mx(px(20.)).ml(px(78.));
        Some(match &result.outcome {
            PublishOutcome::Published { id, swapped } => {
                let id = *id;
                let row = |label: &'static str, value: String| {
                    h_flex()
                        .gap(px(12.))
                        .child(
                            ui::text(12., 18.)
                                .w(px(84.))
                                .flex_none()
                                .text_color(tokens::text2())
                                .child(label),
                        )
                        .child(
                            ui::mono(12., 18.)
                                .min_w_0()
                                .truncate()
                                .text_color(tokens::text())
                                .child(value),
                        )
                };
                let details = inset(v_flex())
                    .p(px(10.))
                    .gap(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::panel2())
                    .child(row("Model", result.name.clone()))
                    .child(row("Asset ID", id.to_string()));
                let warning = partial_warning(*swapped).map(|text| {
                    inset(h_flex())
                        .mt(px(10.))
                        .gap(px(8.))
                        .items_start()
                        .text_color(tokens::warning())
                        .child(ui::icon("triangle-alert", 14.).flex_none())
                        .child(ui::text(12., 18.).min_w_0().child(text))
                });
                let body = v_flex().child(details).children(warning).into_any_element();
                ui::dialog(
                    520.,
                    ui::dialog_glyph("circle-check", ui::green(), ui::green_soft()),
                    "Published to Roblox",
                    insert_hint(id),
                    Some(body),
                    vec![
                        ui::icon_button(
                            "publish-result-copy",
                            "copy",
                            "Copy ID",
                            Weight::Ghost,
                            false,
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(id.to_string()));
                        }))
                        .into_any_element(),
                        ui::external_button(
                            "publish-result-open",
                            "Open on Roblox",
                            Weight::Ghost,
                            false,
                        )
                        .on_click(move |_, _, cx| cx.open_url(&asset_url(id)))
                        .into_any_element(),
                        close,
                    ],
                )
                .into_any_element()
            }
            PublishOutcome::Failed(message) => {
                let body = inset(ui::mono(11.5, 17.))
                    .p(px(10.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(ui::red_line())
                    .bg(ui::red_soft())
                    .text_color(tokens::text())
                    .child(message.clone())
                    .into_any_element();
                ui::dialog(
                    520.,
                    ui::dialog_glyph("circle-alert", ui::red(), ui::red_soft()),
                    "Publish failed",
                    format!(
                        "{} was not published. The rig in the place is unchanged.",
                        result.name
                    ),
                    Some(body),
                    vec![close],
                )
                .into_any_element()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{asset_url, insert_hint, partial_warning};

    #[test]
    fn the_asset_page_hint_and_partial_warning() {
        assert_eq!(asset_url(123), "https://create.roblox.com/store/asset/123");
        assert_eq!(
            insert_hint(123),
            "Insert it in Studio from Toolbox \u{203a} Inventory, or with InsertService:LoadAsset(123)"
        );

        assert_eq!(partial_warning(15), None);
        assert!(partial_warning(9)
            .unwrap()
            .starts_with("Only 9 of 15 parts"));
    }
}
