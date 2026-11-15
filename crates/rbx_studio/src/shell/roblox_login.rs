//! The Sign in to Roblox window: hosts a `LoginFlow` (the Quick Log In code
//! and QR, see `crate::login_flow`) under its own topbar
//! (`shell::chrome::window_topbar`), with Try again (after a failure) and
//! Cancel buttons. A finished sign-in goes to `Shell::roblox_signed_in` and
//! closes the window; closing it cancels.

use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::*;

use crate::launcher::ui;
use crate::login_flow::{LoginEvent, LoginFlow};

use super::Shell;

const WIDTH: f32 = 560.;
const HEIGHT: f32 = 330.;

struct RobloxLoginWindow {
    shell: Entity<Shell>,
    flow: Entity<LoginFlow>,
    _subscriptions: [Subscription; 2],
}

impl Shell {
    /// Brings the sign-in window forward, opening it if it isn't (same
    /// pattern as `open_game_settings`: deferred open, handle kept in
    /// `self.roblox_login`).
    pub(in crate::shell) fn open_roblox_login(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.roblox_login {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = RobloxLoginWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.roblox_login = opened);
        });
    }
}

impl RobloxLoginWindow {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            app_id: Some(crate::APP_ID.to_owned()),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(WIDTH), px(HEIGHT)),
                cx,
            ))),
            is_resizable: false,
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Sign in to Roblox".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx: &mut Context<RobloxLoginWindow>| {
                let flow = cx.new(LoginFlow::new);
                let observed = cx.observe(&flow, |_, _, cx| cx.notify());
                let signed_in = cx.subscribe_in(&flow, window, |this, _, event, window, cx| {
                    let LoginEvent::SignedIn(cookie, user_id) = event;
                    let (cookie, user_id) = (cookie.clone(), *user_id);
                    this.shell
                        .update(cx, |shell, cx| shell.roblox_signed_in(cookie, user_id, cx));
                    window.remove_window();
                });
                RobloxLoginWindow {
                    shell,
                    flow,
                    _subscriptions: [observed, signed_in],
                }
            });
            cx.new(|cx| Root::new(crate::window_blur::wrap(view, cx), window, cx))
        })
        .ok()
    }
}

impl Render for RobloxLoginWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut footer = h_flex().gap(px(8.)).justify_end();
        if self.flow.read(cx).failed() {
            footer = footer.child(
                ui::button("login-retry", "Try again", ui::Weight::Primary, false).on_click(
                    cx.listener(|this, _, _, cx| this.flow.update(cx, |f, cx| f.restart(cx))),
                ),
            );
        }
        footer = footer.child(
            ui::button("login-cancel", "Cancel", ui::Weight::Secondary, false)
                .on_click(|_, window, _| window.remove_window()),
        );

        v_flex()
            .size_full()
            .child(crate::shell::chrome::window_topbar(
                "Sign in to Roblox".into(),
                false,
                |window, _| window.remove_window(),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .p(px(20.))
                    .gap(px(12.))
                    .child(self.flow.read(cx).body(cx))
                    .child(footer),
            )
    }
}
