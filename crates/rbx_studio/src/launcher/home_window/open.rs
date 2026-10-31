//! Home's flows: opening an experience (LocalCopy, download), a Recent
//! entry, a file or a new place, adding by link, and the key windows.

use crate::home::{self, OpenError, Opened, RecentPlace, Template};
use rbx_cloud::{ApiKey, Client, CloudError, Experience};
use std::path::{Path, PathBuf};

use super::*;

impl HomeWindow {
    /// A card, a Recent linked entry, or a resolved link: the LocalCopy
    /// question when a copy exists, else straight to the download.
    /// In the game picker, the experience is handed over and the picker
    /// closes instead; nothing is downloaded.
    pub(super) fn open_experience(&mut self, experience: Experience, cx: &mut Context<Self>) {
        if !self.hand_over(experience.clone(), None, cx) {
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
    }

    /// In the game picker, hands `experience` over and closes the picker;
    /// false when this is Home.
    pub(super) fn hand_over(
        &mut self,
        experience: Experience,
        note: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(pick) = self.pick.clone() {
            // Deferred: the window can't be removed from inside its own
            // event handler.
            let handle = self.handle;
            cx.defer(move |cx| {
                pick(experience, note, cx);
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
            return true;
        }
        false
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

    /// A card's "Open every place as one game": each place's local copy
    /// (downloaded when missing) in one game window. In the game picker it
    /// is an ordinary pick.
    pub(super) fn open_whole_game(&mut self, experience: Experience, cx: &mut Context<Self>) {
        if self.pick.is_some() {
            return self.open_experience(experience, cx);
        }
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
                    async move { home::open_whole_game(&client, &experience) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.serial != serial {
                    return;
                }
                match result {
                    Ok(path) => {
                        this.dialog = None;
                        this.open_path_later(path, cx);
                    }
                    Err(OpenError { status, message }) => {
                        this.dialog = Some(Dialog::Error {
                            title: format!(
                                "Couldn\u{2019}t open every place of {}",
                                experience.name
                            ),
                            experience: None,
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

    /// Opens `path` in the editor and closes Home, deferred a frame, so the
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
                let (message, mut boot) = *failed;
                // Game Settings was for this open only.
                boot.launch.configure = None;
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

    pub(super) fn new_place(&mut self, template: Template, cx: &mut Context<Self>) {
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

    pub(super) fn new_loren_project(&mut self, cx: &mut Context<Self>) {
        // The folder picker is a desktop portal, which a nested test X
        // server doesn't have.
        if let Some(folder) = std::env::var_os(crate::launcher::fixtures::FOLDER_VARIABLE) {
            return self.create_loren_in(Path::new(&folder), cx);
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Create Loren project here".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = match picked.await {
                Ok(picked) => picked,
                Err(_) => return,
            };
            let _ = this.update(cx, |this, cx| match picked {
                Ok(Some(paths)) => {
                    if let Some(folder) = paths.into_iter().next() {
                        this.create_loren_in(&folder, cx);
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    this.dialog = Some(Dialog::Error {
                        experience: None,
                        title: "Couldn\u{2019}t open the file picker".to_string(),
                        status: None,
                        reason: format!("{err:#}"),
                    });
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn create_loren_in(&mut self, folder: &Path, cx: &mut Context<Self>) {
        match create_loren_project(folder) {
            Ok(place) => self.open_path_later(place, cx),
            Err(reason) => {
                self.dialog = Some(Dialog::Error {
                    experience: None,
                    title: "Couldn\u{2019}t create the Loren project".to_string(),
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
            let picked = match picked.await {
                Ok(picked) => picked,
                Err(_) => return,
            };
            let _ = this.update(cx, |this, cx| match picked {
                Ok(Some(paths)) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.open_path_later(path, cx);
                    }
                }
                Ok(None) => {}
                // No file chooser portal, or no session bus to reach one:
                // say so instead of a button that does nothing.
                Err(err) => {
                    this.dialog = Some(Dialog::Error {
                        experience: None,
                        title: "Couldn\u{2019}t open the file picker".to_string(),
                        status: None,
                        reason: format!("{err:#}"),
                    });
                    cx.notify();
                }
            });
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
    /// The game picker has no Home to come back to, so it opens Roblox
    /// publishing (Home › Manage key) instead.
    pub(super) fn set_up_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pick.is_some() {
            return self.manage_key(cx);
        }
        crate::launcher::open_wizard(
            self.boot.clone(),
            crate::launcher::setup::SetupPlan::KEY_ONLY,
            None,
            cx,
        );
        window.remove_window();
    }
}

/// The picked folder itself when empty, else the first free `LorenGame`,
/// `LorenGame2`, ... inside it.
fn loren_target(picked: &Path) -> PathBuf {
    let empty = std::fs::read_dir(picked).is_ok_and(|mut entries| entries.next().is_none());
    if empty {
        return picked.to_path_buf();
    }
    let mut target = picked.join("LorenGame");
    let mut n = 2;
    while target.exists() {
        target = picked.join(format!("LorenGame{n}"));
        n += 1;
    }
    target
}

/// Scaffolds a Loren project under `picked` and returns its place file.
fn create_loren_project(picked: &Path) -> Result<PathBuf, String> {
    let target = loren_target(picked);
    let folder_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let name = if rbx_loren::names::validate_project_name(folder_name).is_ok() {
        folder_name.to_string()
    } else {
        "LorenGame".to_string()
    };
    let template = rbx_loren::template::template_dir()?;
    // Argon: rbx-native's live sync is its Argon client.
    rbx_loren::commands::init::write_scaffold(
        &template,
        &target,
        &name,
        rbx_loren::ctx::Tool::Argon,
    )?;
    rbx_loren::types::generate_types(&target, &Default::default())
        .map_err(|err| err.to_string())?;
    let mut dom = Template::Baseplate.dom();
    rbx_loren::place::apply_project(&mut dom, &target)?;
    let place = target.join(format!("{name}.rbxl"));
    crate::save::save(&dom, crate::save::Format::Binary, &place)?;
    Ok(place)
}

/// The DownloadError line after the status.
fn download_reason(status: Option<u16>, message: &str) -> String {
    match status {
        Some(401 | 403) => "legacy-asset:manage is not granted for this experience".to_string(),
        Some(404) => "The place doesn\u{2019}t exist any more".to_string(),
        _ => message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui::test` would shadow the `#[test]` attribute.
    use super::{create_loren_project, loren_target};
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rbx-loren-home-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn empty_folder_becomes_the_project() {
        let dir = scratch("empty");
        assert_eq!(loren_target(&dir), dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn busy_folder_gets_the_first_free_loren_game() {
        let dir = scratch("busy");
        std::fs::create_dir(dir.join("LorenGame")).unwrap();
        assert_eq!(loren_target(&dir), dir.join("LorenGame2"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn creates_a_place_with_loren_in_it() {
        let dir = scratch("create");
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let place = create_loren_project(&dir).unwrap();
        assert_eq!(place, dir.join("LorenGame/LorenGame.rbxl"));
        assert!(dir.join("LorenGame/default.project.json").is_file());
        let dom = rbx_binary::deserialize(&std::fs::read(&place).unwrap()).unwrap();
        for name in [
            "LorenRuntime",
            "LorenServer",
            "LorenPackages",
            "Shared",
            "Client",
        ] {
            assert!(
                crate::explorer::find_by_name(&dom, name).is_some(),
                "{name}"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
