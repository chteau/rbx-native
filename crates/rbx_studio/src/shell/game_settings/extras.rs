//! The rest of what runs as the Roblox sign-in (see `team_create` for who
//! that is): Basic Info's icon, thumbnails and genre, Permissions' public or
//! private, and Security's Enable Studio Access to API Services. Read
//! together when the link or the sign-in changes; the edits join the same
//! Save and Cancel as every other row.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gpui_kit::*;
use rbx_cloud::{ExperienceIcon, Thumbnail};

use super::media::{download, load_image, Media, MediaDraft, MediaPlan, NewImage};
use super::model::SwitchDraft;
use super::team_create::{describe, Web};
use super::{GameSettingsWindow, Loading};
use crate::home::Link;

/// `RBX_STUDIO_GAME_SETTINGS_IMAGES=a.png:b.png` answers the icon and
/// thumbnail pickers in turn instead of the file dialog, which a nested X
/// server for a capture can't show.
const IMAGES_VARIABLE: &str = "RBX_STUDIO_GAME_SETTINGS_IMAGES";

fn scripted_image() -> Option<PathBuf> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let paths: Vec<_> = std::env::split_paths(&std::env::var_os(IMAGES_VARIABLE)?).collect();
    let next = NEXT.fetch_add(1, Ordering::Relaxed);
    paths.get(next % paths.len().max(1)).cloned()
}

/// The genre the Creator Hub gave the experience, as Studio shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Genre {
    pub(super) genre: Option<String>,
    pub(super) subgenre: Option<String>,
}

pub(super) struct Extras {
    pub(super) loading: Loading,
    /// Bumped by every read and save, so a reply that lost the race is dropped.
    pub(super) generation: u64,
    pub(super) public: SwitchDraft,
    /// Why Roblox would refuse going public; `None` until read or when the
    /// read failed, which leaves the switch to try anyway.
    pub(super) eligibility: Option<rbx_cloud::PublicEligibility>,
    pub(super) api_access: SwitchDraft,
    pub(super) genre: Option<Genre>,
    pub(super) media: MediaDraft,
    /// Renders of the live icon (key 0) and thumbnails (by id).
    pub(super) images: HashMap<u64, Arc<RenderImage>>,
}

/// What one read brings back.
struct Read {
    public: bool,
    eligibility: Option<rbx_cloud::PublicEligibility>,
    api_access: bool,
    genre: Genre,
    media: Media,
}

/// What a save sends: who as, the universe, and each edit.
pub(super) struct ExtrasSave {
    web: Web,
    universe_id: u64,
    public: Option<bool>,
    api_access: Option<bool>,
    media: Option<MediaPlan>,
}

/// What Roblox holds after an [`ExtrasSave`], per part that was sent.
pub(super) struct ExtrasSaved {
    public: Option<Result<bool, String>>,
    api_access: Option<Result<bool, String>>,
    media: Option<(Option<Media>, Option<String>)>,
}

/// Key 0 in [`Extras::images`] is the icon; thumbnail ids are asset ids,
/// never 0.
pub(super) const ICON_IMAGE: u64 = 0;

impl Default for Extras {
    fn default() -> Self {
        Extras {
            loading: Loading::Idle,
            generation: 0,
            public: SwitchDraft::default(),
            eligibility: None,
            api_access: SwitchDraft::default(),
            genre: None,
            media: MediaDraft::default(),
            images: HashMap::new(),
        }
    }
}

impl Extras {
    pub(super) fn is_dirty(&self) -> bool {
        self.public.pending().is_some()
            || self.api_access.pending().is_some()
            || self.media.is_dirty()
    }

    pub(super) fn revert(&mut self) {
        self.public.revert();
        self.api_access.revert();
        self.media.revert();
    }

    /// What a save sends; `None` with nothing edited, unlinked or signed out.
    pub(super) fn pending(&self, web: Option<&Web>, link: Option<&Link>) -> Option<ExtrasSave> {
        let save = ExtrasSave {
            web: web?.clone(),
            universe_id: link?.universe_id,
            public: self.public.pending(),
            api_access: self.api_access.pending(),
            media: self.media.plan(),
        };
        (save.public.is_some() || save.api_access.is_some() || save.media.is_some()).then_some(save)
    }

    /// Takes in what a save left on Roblox; a refused part keeps its edit.
    pub(super) fn saved(&mut self, saved: ExtrasSaved, refusals: &mut Vec<String>) {
        for (draft, reply) in [
            (&mut self.public, saved.public),
            (&mut self.api_access, saved.api_access),
        ] {
            match reply {
                Some(Ok(on)) => draft.rebase(on),
                Some(Err(text)) => refusals.push(text),
                None => {}
            }
        }
        if let Some((media, refusal)) = saved.media {
            if let Some(media) = media {
                self.media.reset(media);
            }
            refusals.extend(refusal);
        }
    }
}

impl GameSettingsWindow {
    /// Reads everything for the current link, dropping the edits.
    pub(super) fn load_extras(&mut self, cx: &mut Context<Self>) {
        let ex = &mut self.extras;
        ex.generation += 1;
        let generation = ex.generation;
        ex.public = SwitchDraft::default();
        ex.eligibility = None;
        ex.api_access = SwitchDraft::default();
        ex.genre = None;
        ex.media.clear();
        ex.images.clear();
        let (Some(link), Some(web)) = (self.link.clone(), self.team_create.web.clone()) else {
            ex.loading = Loading::Idle;
            cx.notify();
            return;
        };
        ex.loading = Loading::Busy;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let universe_id = link.universe_id;
            let reply = cx
                .background_spawn(async move { read(&web, universe_id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let ex = &mut this.extras;
                if generation != ex.generation {
                    return;
                }
                match reply {
                    Ok(read) => {
                        ex.public.reset(read.public);
                        ex.eligibility = read.eligibility;
                        ex.api_access.reset(read.api_access);
                        ex.genre = Some(read.genre);
                        ex.media.reset(read.media.clone());
                        ex.loading = Loading::Idle;
                        this.fetch_images(universe_id, read.media, cx);
                    }
                    Err(message) => ex.loading = Loading::Failed(message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Downloads the renders of whatever `media` holds that isn't drawn yet.
    fn fetch_images(&mut self, universe_id: u64, media: Media, cx: &mut Context<Self>) {
        if matches!(self.team_create.web, Some(Web::Mock)) {
            return;
        }
        let have: Vec<u64> = self.extras.images.keys().copied().collect();
        let generation = self.extras.generation;
        cx.spawn(async move |this, cx| {
            let images = cx
                .background_spawn(async move { download(universe_id, &media, &have) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if generation == this.extras.generation {
                    this.extras.images.extend(images);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn set_public(&mut self, on: bool, cx: &mut Context<Self>) {
        self.extras.public.set(on);
        self.notice = None;
        cx.notify();
    }

    pub(super) fn set_api_access(&mut self, on: bool, cx: &mut Context<Self>) {
        self.extras.api_access.set(on);
        self.notice = None;
        cx.notify();
    }

    /// Asks for an image file; `icon` picks the icon, otherwise it's a new
    /// thumbnail. An unreadable file says so on the notice line.
    pub(super) fn pick_image(&mut self, icon: bool, cx: &mut Context<Self>) {
        let scripted = scripted_image();
        let paths = scripted.is_none().then(|| {
            cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(if icon { "Choose icon" } else { "Add thumbnail" }.into()),
            })
        });
        cx.spawn(async move |this, cx| {
            let path = match (scripted, paths) {
                (Some(path), _) => path,
                (None, Some(paths)) => {
                    let Ok(Ok(Some(paths))) = paths.await else {
                        return;
                    };
                    let Some(path) = paths.into_iter().next() else {
                        return;
                    };
                    path
                }
                (None, None) => return,
            };
            let loaded = cx.background_spawn(async move { load_image(&path) }).await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok((bytes, preview)) => {
                        let image = NewImage {
                            bytes: Arc::new(bytes),
                            preview,
                        };
                        if icon {
                            this.extras.media.icon = Some(image);
                        } else {
                            this.extras.media.add(image);
                        }
                        this.notice = None;
                    }
                    Err(text) => {
                        this.notice = Some(super::Notice { ok: false, text });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn read(web: &Web, universe_id: u64) -> Result<Read, String> {
    let cookie = match web {
        Web::Live(cookie) => cookie,
        Web::Mock => {
            return Ok(Read {
                public: false,
                eligibility: None,
                api_access: true,
                genre: Genre {
                    genre: Some("obby_and_platformer".to_string()),
                    subgenre: Some("tower_obby".to_string()),
                },
                media: Media {
                    icon: Some(ExperienceIcon {
                        image_id: Some(1),
                        approved: true,
                    }),
                    thumbnails: vec![
                        Thumbnail {
                            id: 11,
                            image_id: 21,
                            approved: true,
                        },
                        Thumbnail {
                            id: 12,
                            image_id: 22,
                            approved: false,
                        },
                    ],
                },
            })
        }
    };
    let err = |e: rbx_cloud::CloudError| describe(&e);
    Ok(Read {
        public: rbx_cloud::experience_public(cookie, universe_id).map_err(err)?,
        eligibility: rbx_cloud::public_eligibility(cookie, universe_id).ok(),
        api_access: rbx_cloud::studio_api_access(cookie, universe_id).map_err(err)?,
        genre: rbx_cloud::experience_genre(cookie, universe_id)
            .map(|(genre, subgenre)| Genre { genre, subgenre })
            .map_err(err)?,
        media: read_media(cookie, universe_id).map_err(err)?,
    })
}

fn read_media(
    cookie: &rbx_cloud::heartbeats::SessionCookie,
    universe_id: u64,
) -> Result<Media, rbx_cloud::CloudError> {
    Ok(Media {
        icon: Some(rbx_cloud::experience_icon(cookie, universe_id)?),
        thumbnails: rbx_cloud::experience_thumbnails(cookie, universe_id)?,
    })
}

/// Sends each edit and returns what Roblox then holds.
pub(super) fn save(save: &ExtrasSave) -> ExtrasSaved {
    let universe_id = save.universe_id;
    let cookie = match &save.web {
        Web::Live(cookie) => cookie,
        Web::Mock => {
            return ExtrasSaved {
                public: save.public.map(Ok),
                api_access: save.api_access.map(Ok),
                media: save.media.as_ref().map(|_| (None, None)),
            }
        }
    };
    let err = |e: rbx_cloud::CloudError| describe(&e);
    ExtrasSaved {
        public: save.public.map(|on| {
            rbx_cloud::set_experience_public(cookie, universe_id, on).map_err(err)?;
            Ok(rbx_cloud::experience_public(cookie, universe_id).unwrap_or(on))
        }),
        api_access: save.api_access.map(|on| {
            rbx_cloud::set_studio_api_access(cookie, universe_id, on).map_err(err)?;
            Ok(rbx_cloud::studio_api_access(cookie, universe_id).unwrap_or(on))
        }),
        media: save.media.as_ref().map(|plan| {
            let refusal = send_media(cookie, universe_id, plan).err().map(err);
            (read_media(cookie, universe_id).ok(), refusal)
        }),
    }
}

/// The icon, removals, then uploads; stops at the first refusal.
fn send_media(
    cookie: &rbx_cloud::heartbeats::SessionCookie,
    universe_id: u64,
    plan: &MediaPlan,
) -> Result<(), rbx_cloud::CloudError> {
    if let Some(icon) = &plan.icon {
        rbx_cloud::upload_experience_icon(cookie, universe_id, icon)?;
    }
    for id in &plan.delete {
        rbx_cloud::delete_experience_thumbnail(cookie, universe_id, *id)?;
    }
    for bytes in &plan.upload {
        rbx_cloud::upload_experience_thumbnail(cookie, universe_id, bytes)?;
    }
    Ok(())
}
