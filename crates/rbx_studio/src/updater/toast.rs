//! "Updated to vX.Y.Z": the first launch of an update says so once, in the
//! bottom-right corner of whichever window opens first (Home or the editor),
//! with a link to the release notes. It goes away by itself.

use std::time::Duration;

use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::Installed;
use crate::launcher::ui::icon_exists;
use crate::tokens;

/// Long enough to notice and click "What's new".
const SHOWN: Duration = Duration::from_secs(12);

pub(crate) struct UpdateToast {
    notice: Option<Installed>,
}

struct Shared(Entity<UpdateToast>);

impl Global for Shared {}

/// The toast, for a window's root to add; `None` when there was no update or
/// it has gone.
pub(crate) fn toast(cx: &mut App) -> Option<AnyView> {
    if !cx.has_global::<Shared>() {
        let notice = super::take_notice()?;
        let toast = cx.new(|cx| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SHOWN).await;
                let _ = this.update(cx, |toast: &mut UpdateToast, cx| toast.dismiss(cx));
            })
            .detach();
            UpdateToast {
                notice: Some(notice),
            }
        });
        cx.set_global(Shared(toast));
    }
    let toast = cx.global::<Shared>().0.clone();
    toast.read(cx).notice.is_some().then(|| toast.into())
}

impl UpdateToast {
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        cx.notify();
    }
}

fn glyph(name: &'static str, size: f32) -> component::Icon {
    debug_assert!(icon_exists(name), "no icon named {name}");
    component::Icon::empty()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
}

impl Render for UpdateToast {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(notice) = &self.notice else {
            return div().into_any_element();
        };
        let notes = notice.notes.clone();
        div()
            .absolute()
            // Clear of the editor's Command Bar.
            .bottom(px(52.))
            .right(px(16.))
            .child(
                h_flex()
                    .id("update-toast")
                    .debug_selector(|| "update-toast".into())
                    .gap(px(10.))
                    .py(px(10.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .items_center()
                    .rounded(tokens::radius_container())
                    .bg(tokens::field_select())
                    .shadow(tokens::elevation())
                    .font_family(tokens::font_ui())
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .text_color(tokens::text())
                    .child(
                        div()
                            .text_color(tokens::diff_add())
                            .child(glyph("circle-check", 16.)),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Updated to v{}", notice.version)),
                    )
                    .child(
                        div()
                            .id("update-toast-notes")
                            .px(px(6.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_color(tokens::check_on())
                            .hover(|this| this.bg(tokens::hover()))
                            .child("What\u{2019}s new")
                            .on_click(move |_, _, cx| cx.open_url(&notes)),
                    )
                    .child(
                        div()
                            .id("update-toast-close")
                            .p(px(4.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| this.bg(tokens::hover()).text_color(tokens::text()))
                            .child(glyph("x", 13.))
                            .tooltip(|window, cx| {
                                crate::shell::tooltip::text("Dismiss", window, cx)
                            })
                            .on_click(cx.listener(|toast, _, _, cx| toast.dismiss(cx))),
                    ),
            )
            .into_any_element()
    }
}
