//! Play on macOS, where the game can't go in the 3D view (see
//! `rbx_play::mac_client`): Roblox is brought up full screen, the viewport
//! says where the game went, and the in-game dev panel (`panel`) carries
//! the editor's Output and debugger along.

use std::time::Duration;

use gpui_kit::*;
use rbx_play::mac_client::{self, FullScreen};

use super::session::PLAY_SOURCE;
use crate::command_bar::Feedback;
use crate::shell::Shell;

/// How long to wait for the game's window: the launcher downloads updates
/// before it shows one.
const FULL_SCREEN_WAIT: Duration = Duration::from_secs(90);

impl Shell {
    /// Puts the client full screen once its game window shows up; says in
    /// Output what to allow when the Accessibility permission is missing.
    pub(super) fn play_mac_full_screen(&mut self, cx: &mut Context<Self>) {
        let attempt = self.play.attempt;
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async { mac_client::full_screen(FULL_SCREEN_WAIT, &|| false) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                if shell.play.attempt != attempt {
                    return;
                }
                if outcome == FullScreen::NoAccess {
                    shell.output.push(
                        PLAY_SOURCE,
                        Feedback::Output(
                            "Roblox opened in a window. To have Play open it full screen, allow \
                             RbxNative in System Settings \u{203a} Privacy & Security \u{203a} \
                             Accessibility, then Play again."
                                .to_owned(),
                        ),
                    );
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
