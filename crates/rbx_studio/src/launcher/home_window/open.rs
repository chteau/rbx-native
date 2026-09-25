//! Home's flows: opening an experience (LocalCopy, download), a Recent
//! entry, a file or a new place, adding by link, and the key windows.

use crate::home::{self, OpenError, Opened, RecentPlace, Template};
use rbx_cloud::{ApiKey, Client, CloudError, Experience};
use std::path::{Path, PathBuf};

use super::*;

impl HomeWindow {
    /// A card, a Recent linked entry, or a resolved link: the LocalCopy
    /// question when a copy exists, else straight to the download.
    pub(super) fn open_experience(&mut self, experience: Experience, cx: &mut Context<Self>) {
        match home::local_copy(&experience) {
            Some(path) => {
                self.dialog = Some(Dialog::LocalCopy {
                    experience,
                    path,
                    replace: false,
                })
            }
            None => self.download(experience, true, cx),
        }
        cx.notify();
    }

    pub(super) fn download(
        &mut self,
        experience: Experience,
        replace: bool,
        cx: &mut Context<Self>,
    ) {
        self.serial += 1;
        let serial = self.serial;
        self.dialog = Some(Dialog::Downloading {
            experience: experience.clone(),
        });
        cx.notify();
        let Some(key) = ApiKey::from_env_or_config() else {
            return;
        };
        let client = Client::new(Some(key));
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let experience = experience.clone();
                    async move { home::open_experience(&client, &experience, replace) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.serial != serial {
                    return;
                }
                match result {
                    Ok(Opened::Downloaded(path) | Opened::LocalCopy(path)) => {
                        this.dialog = None;
                        this.open_path_later(path, cx);
                    }
                    Err(OpenError { status, message }) => {
                        this.dialog = Some(Dialog::Error {
                            title: format!("Couldn\u{2019}t download {}", experience.name),
                            experience: Some(experience),
                            status,
                            reason: download_reason(status, &message),
                        })
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cancel_dialog(&mut self, cx: &mut Context<Self>) {
        self.serial += 1;
        self.dialog = None;
        cx.notify();
    }

    /// Opens `path` in the editor and closes Home — deferred a frame, so the
    /// dialog closing paints first and the blocking load doesn't run inside
    /// a click handler.
    pub(super) fn open_path_later(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |this, cx| this.open_path(&path, cx));
        })
        .detach();
    }

    pub(super) fn open_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(boot) = self.boot.borrow_mut().take() else {
            return;
        };
        match crate::open_editor(path, boot, cx) {
            Ok(()) => {
                let _ = self
                    .handle
                    .update(cx, |_, window, _| window.remove_window());
            }
            Err(failed) => {
                let (message, boot) = *failed;
                *self.boot.borrow_mut() = Some(boot);
                self.dialog = Some(Dialog::Error {
                    experience: None,
                    title: format!("Couldn\u{2019}t open {}", file_name(path)),
                    status: None,
                    reason: message,
                });
                cx.notify();
            }
        }
    }

    pub(super) fn open_recent(&mut self, place: &RecentPlace, cx: &mut Context<Self>) {
        self.open_path_later(place.path.clone(), cx);
    }

    pub(super) fn new_place(&mut self, cx: &mut Context<Self>) {
        let template = Template::Baseplate;
        let created = home::new_place_path(template)
            .ok_or_else(|| "no config directory to create the place in".to_string())
            .and_then(|path| template.create(&path).map(|()| path));
        match created {
            Ok(path) => self.open_path_later(path, cx),
            Err(reason) => {
                self.dialog = Some(Dialog::Error {
                    experience: None,
                    title: "Couldn\u{2019}t create the place".to_string(),
                    status: None,
                    reason,
                });
                cx.notify();
            }
        }
    }

    pub(super) fn open_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update(cx, |this, cx| this.open_path_later(path, cx));
                }
            }
        })
        .detach();
    }

    /// Add by place ID or URL: parse locally, resolve the universe, then
    /// the same open flow as a card.
    pub(super) fn add_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.link_state == LinkState::Resolving {
            return;
        }
        let text = self.link.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        let Some(place_id) = rbx_cloud::place_id_from_link(&text) else {
            self.link_state = LinkState::NotALink;
            cx.notify();
            return;
        };
        let Some(key) = ApiKey::from_env_or_config() else {
            self.link_state = LinkState::NoAccess;
            cx.notify();
            return;
        };
        self.link_state = LinkState::Resolving;
        cx.notify();
        let client = Client::new(Some(key));
        let _ = window;
        cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move { client.experience_of_place(place_id) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.link_state = match found {
                    Ok(Some(experience)) => {
                        this.link
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        this.link_open = false;
                        this.open_experience(experience, cx);
                        LinkState::Idle
                    }
                    Ok(None) => LinkState::NoPlace(place_id),
                    Err(CloudError::Http {
                        status: 401 | 403 | 404,
                        ..
                    }) => LinkState::NoAccess,
                    Err(_) => LinkState::Unreachable,
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn manage_key(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        crate::launcher::open_publishing(
            move |cx| {
                let _ = this.update(cx, |this, cx| this.reload(cx));
            },
            cx,
        );
    }

    /// Home without a key: back to the wizard, which comes back here.
    pub(super) fn set_up_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::launcher::open_wizard(self.boot.clone(), cx);
        window.remove_window();
    }
}

/// The DownloadError line after the status.
fn download_reason(status: Option<u16>, message: &str) -> String {
    match status {
        Some(401 | 403) => "legacy-asset:manage is not granted for this experience".to_string(),
        Some(404) => "The place doesn\u{2019}t exist any more".to_string(),
        _ => message.to_string(),
    }
}
