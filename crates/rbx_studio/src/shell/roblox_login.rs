//! The Roblox sign-in window: shows a Quick Log In code and its QR image
//! (`rbx_cloud::quick_login`), waits for the user to approve it at
//! `CONFIRM_PAGE` or in the Roblox app, then hands the session to
//! `Shell::roblox_signed_in`. No web view: the code flow needs none.
//!
//! Layout (own topbar like every window, `shell::chrome::window_topbar`):
//! QR image on the left (~180 px), on the right "Enter this code at
//! roblox.com/crossdevicelogin/ConfirmCode" (the link opens the browser),
//! the code in large monospace letters, "or scan it with the Roblox app",
//! then a status line ("Waiting for approval…", "Approve it on your other
//! device…" once UserLinked, errors in the error colour) and a Cancel
//! button. Codes live about two minutes: an `Expired` status fetches a new
//! code and QR silently. Closing the window cancels.

use std::sync::Arc;

use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::*;
use rbx_cloud::quick_login::{LoginCode, LoginStatus, QuickLogin, CONFIRM_PAGE};

use crate::launcher::ui;
use crate::tokens;

use super::Shell;

/// How often the status is polled.
pub(super) const POLL: std::time::Duration = std::time::Duration::from_secs(3);

const WIDTH: f32 = 560.;
const HEIGHT: f32 = 330.;

enum State {
    Fetching,
    Showing {
        code: String,
        qr: Option<Arc<RenderImage>>,
        linked: bool,
    },
    Failed(String),
}

struct RobloxLoginWindow {
    shell: Entity<Shell>,
    state: State,
    task: Task<()>,
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

fn decode_qr(png: &[u8]) -> Option<Arc<RenderImage>> {
    let rgba = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = rgba.dimensions();
    crate::render_image::to_render_image(rgba.into_raw(), w, h)
}

/// A new code with its QR image (a missing QR is not fatal: the code works).
fn fetch_code(login: &mut QuickLogin) -> Result<(LoginCode, Option<Vec<u8>>), String> {
    let code = login.create()?;
    let png = login.qr_png(&code).ok();
    Ok((code, png))
}

fn spaced(code: &str) -> String {
    code.chars().map(String::from).collect::<Vec<_>>().join(" ")
}

impl RobloxLoginWindow {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
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
            let view = cx.new(|cx| {
                let mut this = RobloxLoginWindow {
                    shell,
                    state: State::Fetching,
                    task: Task::ready(()),
                };
                this.start(window, cx);
                this
            });
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = State::Fetching;
        self.task = cx.spawn_in(window, async move |this, cx| {
            let executor = cx.background_executor().clone();
            let mut login = QuickLogin::new();
            let (mut login_back, first) = executor
                .spawn(async move {
                    let r = fetch_code(&mut login);
                    (login, r)
                })
                .await;
            let mut code = match first {
                Ok((code, png)) => {
                    Self::show(&this, cx, &code, png);
                    code
                }
                Err(e) => {
                    Self::fail(&this, cx, e);
                    return;
                }
            };
            loop {
                cx.background_executor().timer(POLL).await;
                let poll_code = code.clone();
                let (back, status) = executor
                    .spawn(async move {
                        let mut login = login_back;
                        let r = login.status(&poll_code);
                        (login, r)
                    })
                    .await;
                login_back = back;
                match status {
                    Err(e) => {
                        Self::fail(&this, cx, e);
                        return;
                    }
                    Ok(LoginStatus::Created) => {}
                    Ok(LoginStatus::UserLinked) => {
                        let _ = this.update(cx, |this, cx| {
                            if let State::Showing { linked, .. } = &mut this.state {
                                *linked = true;
                            }
                            cx.notify();
                        });
                    }
                    Ok(LoginStatus::Cancelled) => {
                        Self::fail(&this, cx, "The sign-in was cancelled.".to_owned());
                        return;
                    }
                    Ok(LoginStatus::Expired) => {
                        let (back, fresh) = executor
                            .spawn(async move {
                                let mut login = login_back;
                                let r = fetch_code(&mut login);
                                (login, r)
                            })
                            .await;
                        login_back = back;
                        match fresh {
                            Ok((new_code, png)) => {
                                Self::show(&this, cx, &new_code, png);
                                code = new_code;
                            }
                            Err(e) => {
                                Self::fail(&this, cx, e);
                                return;
                            }
                        }
                    }
                    Ok(LoginStatus::Validated) => {
                        let done_code = code.clone();
                        let (_, finished) = executor
                            .spawn(async move {
                                let mut login = login_back;
                                let r = login.finish(&done_code);
                                (login, r)
                            })
                            .await;
                        match finished {
                            Ok((cookie, user_id)) => {
                                let _ = this.update_in(cx, |this, window, cx| {
                                    this.shell.update(cx, |shell, cx| {
                                        shell.roblox_signed_in(cookie, user_id, cx)
                                    });
                                    window.remove_window();
                                });
                            }
                            Err(e) => Self::fail(&this, cx, e),
                        }
                        return;
                    }
                }
            }
        });
        cx.notify();
    }

    fn show(
        this: &WeakEntity<Self>,
        cx: &mut AsyncWindowContext,
        code: &LoginCode,
        png: Option<Vec<u8>>,
    ) {
        let text = spaced(&code.code);
        let _ = this.update(cx, |this, cx| {
            this.state = State::Showing {
                code: text,
                qr: png.as_deref().and_then(decode_qr),
                linked: false,
            };
            cx.notify();
        });
    }

    fn fail(this: &WeakEntity<Self>, cx: &mut AsyncWindowContext, text: String) {
        let _ = this.update(cx, |this, cx| {
            this.state = State::Failed(text);
            cx.notify();
        });
    }
}

impl Render for RobloxLoginWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (body, status): (AnyElement, Option<(String, bool)>) = match &self.state {
            State::Fetching => (
                div().child("Getting a sign-in code…").into_any_element(),
                None,
            ),
            State::Failed(text) => (div().into_any_element(), Some((text.clone(), true))),
            State::Showing { code, qr, linked } => {
                let image: AnyElement = match qr {
                    Some(image) => img(image.clone()).size(px(180.)).into_any_element(),
                    None => div().size(px(180.)).into_any_element(),
                };
                let message = if *linked {
                    "Approve it on your other device…"
                } else {
                    "Waiting for approval…"
                };
                let right = v_flex()
                    .flex_1()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(tokens::text())
                            .child("Enter this code at"),
                    )
                    .child(
                        div()
                            .id("confirm-link")
                            .text_size(px(13.))
                            .text_color(ui::accent())
                            .cursor_pointer()
                            .on_click(|_, _, cx| cx.open_url(CONFIRM_PAGE))
                            .child("roblox.com/crossdevicelogin/ConfirmCode"),
                    )
                    .child(
                        div()
                            .font_family("monospace")
                            .text_size(px(28.))
                            .text_color(tokens::text())
                            .child(code.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(tokens::text2())
                            .child("or scan it with the Roblox app"),
                    );
                (
                    h_flex()
                        .gap(px(16.))
                        .child(image)
                        .child(right)
                        .into_any_element(),
                    Some((message.to_owned(), false)),
                )
            }
        };
        let failed = matches!(self.state, State::Failed(_));

        let mut footer = h_flex().gap(px(8.)).justify_end();
        if failed {
            footer = footer.child(
                ui::button("login-retry", "Try again", ui::Weight::Primary, false)
                    .on_click(cx.listener(|this, _, window, cx| this.start(window, cx))),
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
                    .text_color(tokens::text())
                    .child(body)
                    .children(status.map(|(text, error)| {
                        div()
                            .text_size(px(13.))
                            .text_color(if error {
                                tokens::text_error()
                            } else {
                                tokens::text2()
                            })
                            .child(text)
                    }))
                    .child(footer),
            )
    }
}
