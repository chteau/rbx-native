//! Basic Info's icon and thumbnails as an edit waiting for Save, like every
//! other row: a new icon, thumbnails added and removed. Save turns the edit
//! into Roblox's calls ([`MediaDraft::plan`]), removals first so an add never
//! meets the cap. There's no reordering: the order call Studio's old dialog
//! made (`develop` `POST /v1/universes/{id}/thumbnails/order`) still answers
//! 200 but no longer moves anything, so the order is the Creator Hub's.

use std::sync::Arc;

use gpui_kit::RenderImage;
use rbx_cloud::{ExperienceIcon, Thumbnail, MAX_THUMBNAILS};

/// An image picked from disk, not on Roblox yet.
#[derive(Clone)]
pub(super) struct NewImage {
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) preview: Option<Arc<RenderImage>>,
}

/// One thumbnail slot, in the order the page will show them.
#[derive(Clone)]
pub(super) enum Slot {
    Live(Thumbnail),
    New(NewImage),
}

/// What Roblox holds: the icon and the thumbnails in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Media {
    pub(super) icon: Option<ExperienceIcon>,
    pub(super) thumbnails: Vec<Thumbnail>,
}

#[derive(Default)]
pub(super) struct MediaDraft {
    /// `None` before the first read.
    remote: Option<Media>,
    pub(super) icon: Option<NewImage>,
    pub(super) slots: Vec<Slot>,
}

/// The calls a save makes, in order.
pub(super) struct MediaPlan {
    pub(super) icon: Option<Arc<Vec<u8>>>,
    pub(super) delete: Vec<u64>,
    pub(super) upload: Vec<Arc<Vec<u8>>>,
}

impl MediaDraft {
    pub(super) fn remote(&self) -> Option<&Media> {
        self.remote.as_ref()
    }

    /// Starts over from what Roblox holds, dropping the edit.
    pub(super) fn reset(&mut self, media: Media) {
        self.slots = media.thumbnails.iter().copied().map(Slot::Live).collect();
        self.icon = None;
        self.remote = Some(media);
    }

    pub(super) fn clear(&mut self) {
        *self = MediaDraft::default();
    }

    pub(super) fn revert(&mut self) {
        if let Some(media) = self.remote.take() {
            self.reset(media);
        }
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.remote.is_some() && self.icon.is_some() || self.thumbnails_dirty()
    }

    /// Whether the thumbnails differ from Roblox's, icon aside.
    pub(super) fn thumbnails_dirty(&self) -> bool {
        let Some(remote) = &self.remote else {
            return false;
        };
        self.slots.len() != remote.thumbnails.len()
            || self
                .slots
                .iter()
                .zip(&remote.thumbnails)
                .any(|(slot, live)| !matches!(slot, Slot::Live(t) if t.id == live.id))
    }

    pub(super) fn is_full(&self) -> bool {
        self.slots.len() >= MAX_THUMBNAILS
    }

    /// Adds `image` last; ignored at the cap.
    pub(super) fn add(&mut self, image: NewImage) {
        if !self.is_full() {
            self.slots.push(image.into_slot());
        }
    }

    pub(super) fn remove(&mut self, index: usize) {
        if index < self.slots.len() {
            self.slots.remove(index);
        }
    }

    /// `None` with nothing edited or before the first read.
    pub(super) fn plan(&self) -> Option<MediaPlan> {
        let remote = self.remote.as_ref().filter(|_| self.is_dirty())?;
        let kept: Vec<u64> = self
            .slots
            .iter()
            .filter_map(|slot| match slot {
                Slot::Live(t) => Some(t.id),
                Slot::New(_) => None,
            })
            .collect();
        let delete = remote
            .thumbnails
            .iter()
            .map(|t| t.id)
            .filter(|id| !kept.contains(id))
            .collect();
        let upload = self
            .slots
            .iter()
            .filter_map(|slot| match slot {
                Slot::New(image) => Some(image.bytes.clone()),
                Slot::Live(_) => None,
            })
            .collect();
        Some(MediaPlan {
            icon: self.icon.as_ref().map(|image| image.bytes.clone()),
            delete,
            upload,
        })
    }
}

impl NewImage {
    fn into_slot(self) -> Slot {
        Slot::New(self)
    }
}

/// Reads a PNG or JPEG and decodes it for its preview.
pub(super) fn load_image(
    path: &std::path::Path,
) -> Result<(Vec<u8>, Option<Arc<RenderImage>>), String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("Couldn\u{2019}t read {}: {e}", path.display()))?;
    let format = image::guess_format(&bytes).ok();
    if !matches!(
        format,
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
    ) {
        return Err("Roblox takes PNG or JPEG images.".to_string());
    }
    Ok((bytes.clone(), decode(&bytes)))
}

fn decode(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = rgba.dimensions();
    crate::render_image::to_render_image(rgba.into_raw(), w, h)
}

pub(super) fn download(
    universe_id: u64,
    media: &Media,
    have: &[u64],
) -> Vec<(u64, Arc<RenderImage>)> {
    let client = rbx_cloud::Client::new(None);
    let mut urls = Vec::new();
    if media.icon.is_some_and(|i| i.image_id.is_some())
        && !have.contains(&super::extras::ICON_IMAGE)
    {
        urls.extend(
            client
                .game_icon_urls(&[universe_id])
                .unwrap_or_default()
                .into_iter()
                .map(|(_, url)| (super::extras::ICON_IMAGE, url)),
        );
    }
    let ids: Vec<u64> = media
        .thumbnails
        .iter()
        .map(|t| t.image_id)
        .filter(|id| !have.contains(id))
        .collect();
    urls.extend(
        client
            .game_thumbnail_urls(universe_id, &ids)
            .unwrap_or_default(),
    );
    urls.into_iter()
        .filter_map(|(key, url)| Some((key, decode(&client.download_image(&url).ok()?)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thumb(id: u64) -> Thumbnail {
        Thumbnail {
            id,
            image_id: id + 100,
            approved: true,
        }
    }

    fn image(key: u64) -> NewImage {
        NewImage {
            bytes: Arc::new(vec![key as u8]),
            preview: None,
        }
    }

    fn loaded(ids: &[u64]) -> MediaDraft {
        let mut draft = MediaDraft::default();
        draft.reset(Media {
            icon: None,
            thumbnails: ids.iter().copied().map(thumb).collect(),
        });
        draft
    }

    #[test]
    fn nothing_is_planned_before_a_read_or_without_an_edit() {
        let mut unread = MediaDraft::default();
        unread.add(image(1));
        assert!(unread.plan().is_none() && unread.slots.len() == 1 && !unread.is_dirty());
        assert!(loaded(&[1, 2]).plan().is_none());
    }

    #[test]
    fn a_removal_is_a_delete_with_no_reorder() {
        let mut draft = loaded(&[1, 2, 3]);
        draft.remove(1);
        let plan = draft.plan().unwrap();
        assert_eq!(plan.delete, [2]);
        assert!(plan.upload.is_empty() && plan.icon.is_none());
    }

    #[test]
    fn an_add_is_an_upload() {
        let mut draft = loaded(&[1, 2]);
        draft.add(image(9));
        let plan = draft.plan().unwrap();
        assert_eq!(plan.upload.len(), 1);
        assert!(plan.delete.is_empty());
    }

    #[test]
    fn the_cap_stops_adds_and_revert_drops_every_edit() {
        let mut draft = loaded(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        draft.add(image(10));
        draft.add(image(11));
        assert_eq!(draft.slots.len(), MAX_THUMBNAILS);
        draft.icon = Some(image(12));
        draft.revert();
        assert!(!draft.is_dirty() && draft.icon.is_none() && draft.slots.len() == 9);
    }
}
