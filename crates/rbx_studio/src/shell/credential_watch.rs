//! Expired credentials during an editor session.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::Shell;
use crate::command_bar::Feedback;
use crate::launcher::setup::SetupPlan;
use crate::launcher::ui::{self, Weight};
use crate::tokens;

/// How often the client's refusal flags are read; a local read only.
const POLL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(super) struct CredentialWatch {
    notice: Option<SetupPlan>,
    wizard: Option<WindowHandle<gpui_kit::component::Root>>,
    poll: Option<Task<()>>,
}

impl Shell {
    pub(super) fn start_credential_watch(&mut self, cx: &mut Context<Self>) {
        let fixture = std::env::var("RBX_STUDIO_CREDENTIAL_NOTICE").ok();
        let reports = match fixture.as_deref() {
            Some("session") => Some(rbx_cloud::expiry::Reports {
                key: false,
                session: true,
            }),
            Some("key") => Some(rbx_cloud::expiry::Reports {
                key: true,
                session: false,
            }),
            Some("both") => Some(rbx_cloud::expiry::Reports {
                key: true,
                session: true,
            }),
            Some("env") => Some(rbx_cloud::expiry::Reports {
                key: true,
                session: false,
            }),
            _ => None,
        };
        if let Some(reports) = reports {
            let from_env = fixture.as_deref() == Some("env") || key_from_env();
            self.credential_watch.notice = Some(merged(None, reports, from_env));
        }
        let mut net = rbx_net::status();
        self.credential_watch.poll = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            // The title bar's Offline pill follows the network the same way,
            // and so does everything greyed out while offline.
            let now = rbx_net::status();
            if now != net {
                net = now;
                let followed = this.update(cx, |shell, cx| {
                    crate::menu_bar::follow_network(shell.script_templates.extras(), cx);
                    cx.notify();
                });
                if followed.is_err() {
                    return;
                }
            }
            let reports = rbx_cloud::expiry::take_reports();
            if !(reports.key || reports.session) {
                continue;
            }
            if this
                .update(cx, |shell, cx| shell.credentials_expired(reports, cx))
                .is_err()
            {
                return;
            }
        }));
    }

    fn credentials_expired(&mut self, reports: rbx_cloud::expiry::Reports, cx: &mut Context<Self>) {
        if reports.session {
            self.stop_studio_presence();
            self.studio_presence.session = None;
            self.set_presence_user(None, cx);
        }
        let from_env = key_from_env();
        cx.spawn(async move |this, cx| {
            let mut errors = Vec::new();
            if reports.session {
                if let Err(error) = crate::key_store::forget_session(cx).await {
                    errors.push(format!("Could not remove the Roblox sign-in: {error}"));
                }
            }
            if reports.key && !from_env {
                if let Err(error) = crate::key_store::forget(cx).await {
                    errors.push(format!("Could not remove the API key: {error}"));
                }
            }
            if errors.is_empty() {
                return;
            }
            let _ = this.update(cx, |shell, cx| {
                for message in errors {
                    shell.output.push("Roblox", Feedback::Error(message));
                }
                cx.notify();
            });
        })
        .detach();
        self.credential_watch.notice =
            Some(merged(self.credential_watch.notice, reports, from_env));
        cx.notify();
    }

    pub(super) fn credential_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let plan = self.credential_watch.notice?;
        let label = action_label(&plan);
        let text = plan.reasons().join(" ");
        let weak = cx.entity().downgrade();
        let setup = label.map(|label| {
            ui::button("credential-setup", label, Weight::Primary, true).on_click(cx.listener(
                move |shell, _, _, cx| {
                    if let Some(handle) = shell.credential_watch.wizard {
                        if handle
                            .update(cx, |_, window, _| window.activate_window())
                            .is_ok()
                        {
                            return;
                        }
                    }
                    let weak = weak.clone();
                    cx.defer(move |cx| {
                        let finish_weak = weak.clone();
                        let on_finish: crate::launcher::Finish = Rc::new(move |cx: &mut App| {
                            let _ = finish_weak.update(cx, |shell, cx| {
                                shell.credential_watch.notice = None;
                                shell.credential_watch.wizard = None;
                                shell.restore_roblox_session(cx);
                                cx.notify();
                            });
                        });
                        let handle = crate::launcher::open_wizard(
                            Rc::new(RefCell::new(None)),
                            plan,
                            Some(on_finish),
                            cx,
                        );
                        let _ = weak.update(cx, |shell, _| {
                            shell.credential_watch.wizard = handle;
                        });
                    });
                },
            ))
        });
        let dismiss = ui::button("credential-dismiss", "Dismiss", Weight::Ghost, true).on_click(
            cx.listener(|shell, _, _, cx| {
                shell.credential_watch.notice = None;
                cx.notify();
            }),
        );
        Some(
            h_flex()
                .w_full()
                .h(px(32.))
                .flex_none()
                .px(px(12.))
                .gap(px(10.))
                .items_center()
                .bg(ui::panel())
                .border_b_1()
                .border_color(tokens::border())
                .child(ui::icon("triangle-alert", 14.).text_color(ui::red()))
                .child(ui::text(12., 16.).child(text))
                .child(div().flex_1())
                .children(setup)
                .child(dismiss)
                .into_any_element(),
        )
    }
}

fn key_from_env() -> bool {
    rbx_cloud::ApiKey::source() == "RBX_API_KEY"
}

fn merged(
    current: Option<SetupPlan>,
    reports: rbx_cloud::expiry::Reports,
    key_from_env: bool,
) -> SetupPlan {
    let mut plan = current.unwrap_or_default();
    if reports.session {
        plan.sign_in = true;
        plan.session_expired = true;
    }
    if reports.key {
        if key_from_env {
            plan.env_key_refused = true;
        } else {
            plan.key = true;
            plan.key_expired = true;
        }
    }
    plan
}

fn action_label(plan: &SetupPlan) -> Option<&'static str> {
    match (plan.sign_in, plan.key) {
        (true, false) => Some("Sign in again"),
        (false, true) => Some("Set up key"),
        (true, true) => Some("Set up again"),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{action_label, merged};
    use rbx_cloud::expiry::Reports;

    const KEY: Reports = Reports {
        key: true,
        session: false,
    };
    const SESSION: Reports = Reports {
        key: false,
        session: true,
    };

    #[test]
    fn env_key_is_flagged_not_set_up() {
        let plan = merged(None, KEY, true);
        assert!(plan.env_key_refused && !plan.key && !plan.key_expired);
    }

    #[test]
    fn keychain_key_expires() {
        let plan = merged(None, KEY, false);
        assert!(plan.key && plan.key_expired && !plan.env_key_refused);
    }

    #[test]
    fn session_with_env_key() {
        let plan = merged(Some(merged(None, SESSION, true)), KEY, true);
        assert!(plan.sign_in && plan.env_key_refused);
    }

    #[test]
    fn labels() {
        assert_eq!(
            action_label(&merged(None, SESSION, false)),
            Some("Sign in again")
        );
        assert_eq!(action_label(&merged(None, KEY, false)), Some("Set up key"));
        let both = merged(Some(merged(None, SESSION, false)), KEY, false);
        assert_eq!(action_label(&both), Some("Set up again"));
        assert_eq!(action_label(&merged(None, KEY, true)), None);
    }
}
