//! The 3D view during a test on macOS: the game runs in the Roblox app
//! (it can't be placed in this view, see `rbx_play::mac_client`), so the
//! view is blurred and says so, with a way back to the game.
//!
//! `RBX_NATIVE_PLAY_VEIL=1` shows it on any OS without a test running, for
//! screenshots.

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::session::StopReason;
use super::Phase;
use crate::launcher::ui::{self, Weight};
use crate::shell::Shell;
use crate::tokens;

const PREVIEW_VARIABLE: &str = "RBX_NATIVE_PLAY_VEIL";

const TITLE: &str = "A Roblox play session is active";
const TEXT: &str = "macOS can\u{2019}t show the game inside this view, so it runs in the \
     Roblox app. Press F8 in the game for the RbxNative panel: Output, stats and the debugger.";

impl Shell {
    fn play_veiled(&self) -> bool {
        (cfg!(target_os = "macos") && self.play.phase == Phase::Running)
            || std::env::var_os(PREVIEW_VARIABLE).is_some()
    }

    /// The veil over the 3D view, when it is up; also blurs or un-blurs the
    /// view's picture to match.
    pub(in crate::shell) fn play_veil(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let veiled = self.play_veiled();
        self.viewport
            .update(cx, |view, cx| view.set_veiled(veiled, window, cx));
        if !veiled {
            return None;
        }
        let card = v_flex()
            .id("play-veil-card")
            .occlude()
            .w(px(400.))
            .max_w(relative(0.9))
            .gap(px(10.))
            .p(px(20.))
            .rounded(px(12.))
            .border_1()
            .border_color(tokens::border2())
            .bg(tokens::dock())
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.3),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(div().size(px(8.)).rounded_full().bg(ui::green()))
                    .child(
                        ui::text(15., 20.)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens::text())
                            .child(TITLE),
                    ),
            )
            .child(ui::text(12.5, 18.).text_color(tokens::text2()).child(TEXT))
            .child(
                h_flex()
                    .pt(px(6.))
                    .gap(px(8.))
                    .justify_end()
                    .child(
                        ui::button("play-veil-stop", "Stop", Weight::Secondary, true).on_click(
                            cx.listener(|shell, _, _, cx| shell.play_stop(StopReason::Stopped, cx)),
                        ),
                    )
                    .child(
                        ui::button("play-veil-show", "Show Roblox", Weight::Primary, true)
                            .on_click(cx.listener(|shell, _, _, cx| shell.play_show_roblox(cx))),
                    ),
            );
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(tokens::veil())
                .child(card)
                .into_any_element(),
        )
    }

    /// Brings the Roblox app to the front (macOS); nothing to bring
    /// elsewhere, where the game is in the view.
    fn play_show_roblox(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        if !rbx_play::mac_client::bring_to_front() {
            self.output.push(
                super::session::PLAY_SOURCE,
                crate::command_bar::Feedback::Warning(
                    "Roblox isn\u{2019}t running anymore.".to_owned(),
                ),
            );
        }
        cx.notify();
    }
}
