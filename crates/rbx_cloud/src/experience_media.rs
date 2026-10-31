//! An experience's icon and thumbnails, as Studio's Game Settings and the
//! Creator Hub change them: run as the web sign-in (see `web`), because the
//! key-scoped `legacy-game-internationalization` routes only reach the
//! translated copies, not the source-language images everyone sees.
//!
//! Reads: `develop` `GET /v1/universes/{id}/icon` and `games`
//! `GET /v1/games/{id}/media` (both seen in Studio's HTTP trace). Writes:
//! `publish` `POST /v1/games/{id}/icon` and `/thumbnail/image` (multipart,
//! field `request.files`) and `develop`
//! `DELETE /v1/universes/{id}/thumbnails/{thumbnailId}`.

use serde::Deserialize;

use crate::error::CloudError;
use crate::heartbeats::SessionCookie;
use crate::web::{self, Image, Method, DEVELOP};

const PUBLISH: &str = "https://publish.roblox.com";
const GAMES: &str = "https://games.roblox.com";
const FILE_FIELD: &str = "request.files";

/// The most thumbnails an experience can have (the Creator Hub's
/// `maxThumbnailsOnEDP`).
pub const MAX_THUMBNAILS: usize = 10;

/// The experience's icon: `None` before one is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExperienceIcon {
    pub image_id: Option<u64>,
    pub approved: bool,
}

/// One thumbnail, in the order the experience page shows them. `id` is the
/// media slot's own id, what deleting takes: given the image's asset id
/// instead, it answers 200 and removes nothing. `image_id` is what the
/// thumbnails API renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thumbnail {
    pub id: u64,
    pub image_id: u64,
    pub approved: bool,
}

/// The current icon.
pub fn experience_icon(
    cookie: &SessionCookie,
    universe_id: u64,
) -> Result<ExperienceIcon, CloudError> {
    icon_at(DEVELOP, cookie, universe_id)
}

/// Replaces the icon with `image` (PNG or JPEG); Roblox reviews it before
/// showing it.
pub fn upload_experience_icon(
    cookie: &SessionCookie,
    universe_id: u64,
    image: &[u8],
) -> Result<(), CloudError> {
    upload_at(
        PUBLISH,
        cookie,
        &format!("/v1/games/{universe_id}/icon"),
        image,
    )
}

/// The thumbnails, images only (a video slot has no image to show).
pub fn experience_thumbnails(
    cookie: &SessionCookie,
    universe_id: u64,
) -> Result<Vec<Thumbnail>, CloudError> {
    thumbnails_at(GAMES, cookie, universe_id)
}

/// Adds `image` as a thumbnail, wherever Roblox puts it (not always last).
pub fn upload_experience_thumbnail(
    cookie: &SessionCookie,
    universe_id: u64,
    image: &[u8],
) -> Result<(), CloudError> {
    upload_at(
        PUBLISH,
        cookie,
        &format!("/v1/games/{universe_id}/thumbnail/image"),
        image,
    )
}

pub fn delete_experience_thumbnail(
    cookie: &SessionCookie,
    universe_id: u64,
    id: u64,
) -> Result<(), CloudError> {
    delete_at(DEVELOP, cookie, universe_id, id)
}

fn delete_at(
    root: &str,
    cookie: &SessionCookie,
    universe_id: u64,
    id: u64,
) -> Result<(), CloudError> {
    let url = format!("{root}/v1/universes/{universe_id}/thumbnails/{id}");
    web::send(cookie, Method::Delete, &url, &[]).map(|_| ())
}

fn icon_at(
    root: &str,
    cookie: &SessionCookie,
    universe_id: u64,
) -> Result<ExperienceIcon, CloudError> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "imageId")]
        image_id: Option<u64>,
        #[serde(rename = "isApproved", default)]
        approved: bool,
    }
    let url = format!("{root}/v1/universes/{universe_id}/icon");
    let raw: Raw = serde_json::from_slice(&web::send(cookie, Method::Get, &url, &[])?)?;
    Ok(ExperienceIcon {
        image_id: raw.image_id.filter(|id| *id != 0),
        approved: raw.approved,
    })
}

fn upload_at(
    root: &str,
    cookie: &SessionCookie,
    path: &str,
    bytes: &[u8],
) -> Result<(), CloudError> {
    let image = Image {
        field: FILE_FIELD,
        bytes,
    };
    web::send(cookie, Method::Image(&image), &format!("{root}{path}"), &[]).map(|_| ())
}

fn thumbnails_at(
    root: &str,
    cookie: &SessionCookie,
    universe_id: u64,
) -> Result<Vec<Thumbnail>, CloudError> {
    let url = format!("{root}/v1/games/{universe_id}/media");
    parse_media(&web::send(cookie, Method::Get, &url, &[])?)
}

fn parse_media(body: &[u8]) -> Result<Vec<Thumbnail>, CloudError> {
    #[derive(Deserialize)]
    struct Item {
        id: Option<u64>,
        #[serde(rename = "assetType", default)]
        asset_type: String,
        #[serde(rename = "imageId")]
        image_id: Option<u64>,
        #[serde(default)]
        approved: bool,
    }
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Item>,
    }
    let page: Page = serde_json::from_slice(body)?;
    Ok(page
        .data
        .into_iter()
        .filter(|item| item.asset_type.is_empty() || item.asset_type == "Image")
        .filter_map(|item| {
            let image_id = item.image_id.filter(|id| *id != 0)?;
            Some(Thumbnail {
                id: item.id.filter(|id| *id != 0)?,
                image_id,
                approved: item.approved,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::serve::serve;

    fn cookie() -> SessionCookie {
        SessionCookie::new("test-cookie")
    }

    #[test]
    fn the_icon_reads_id_and_review_state_and_zero_is_none() {
        let (root, served) = serve(vec![
            (200, "", r#"{"imageId":123,"isApproved":true}"#),
            (200, "", r#"{"imageId":0,"isApproved":false}"#),
        ]);
        let first = icon_at(&root, &cookie(), 42).unwrap();
        let second = icon_at(&root, &cookie(), 42).unwrap();
        let requests = served.join().unwrap();
        assert_eq!(
            first,
            ExperienceIcon {
                image_id: Some(123),
                approved: true
            }
        );
        assert_eq!(second.image_id, None);
        assert!(requests[0].starts_with("GET /v1/universes/42/icon "));
    }

    #[test]
    fn an_upload_is_one_request_files_part_with_the_image_type() {
        let (root, served) = serve(vec![(200, "", r#"{"targetId":9}"#)]);
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
        upload_at(&root, &cookie(), "/v1/games/42/icon", &jpeg).unwrap();
        let request = &served.join().unwrap()[0];
        assert!(request.starts_with("POST /v1/games/42/icon "), "{request}");
        assert!(request
            .to_lowercase()
            .contains("content-type: multipart/form-data; boundary=rbx-native-"));
        assert!(request.contains("name=\"request.files\"; filename=\"image.jpg\""));
        assert!(request.contains("Content-Type: image/jpeg"));
    }

    #[test]
    fn png_bytes_go_as_png() {
        let image = Image {
            field: FILE_FIELD,
            bytes: b"\x89PNG....",
        };
        let text = String::from_utf8_lossy(&image.multipart("B")).into_owned();
        assert!(text.starts_with("--B\r\nContent-Disposition: form-data; name=\"request.files\"; filename=\"image.png\"\r\nContent-Type: image/png\r\n\r\n"));
        assert!(text.ends_with("\r\n--B--\r\n"));
    }

    #[test]
    fn media_keeps_images_in_order_and_skips_videos() {
        let thumbs = parse_media(
            br#"{"data":[
                {"id":50,"assetTypeId":1,"assetType":"Image","imageId":5,"approved":true},
                {"id":60,"assetTypeId":33,"assetType":"YouTubeVideo","imageId":null,"videoHash":"x","approved":true},
                {"id":30,"assetTypeId":1,"assetType":"Image","imageId":3,"approved":false}]}"#,
        )
        .unwrap();
        assert_eq!(
            thumbs,
            [
                Thumbnail {
                    id: 50,
                    image_id: 5,
                    approved: true
                },
                Thumbnail {
                    id: 30,
                    image_id: 3,
                    approved: false
                }
            ]
        );
    }

    #[test]
    fn deleting_names_the_thumbnail() {
        let (root, served) = serve(vec![(200, "", "{}")]);
        delete_at(&root, &cookie(), 42, 5).unwrap();
        let request = &served.join().unwrap()[0];
        assert!(
            request.starts_with("DELETE /v1/universes/42/thumbnails/5 "),
            "{request}"
        );
    }

    #[test]
    fn a_refused_upload_carries_roblox_s_message() {
        let (root, served) = serve(vec![(
            400,
            "",
            r#"{"errors":[{"code":5,"message":"Too many thumbnails"}]}"#,
        )]);
        let err = upload_at(&root, &cookie(), "/v1/games/42/thumbnail/image", b"x").unwrap_err();
        served.join().unwrap();
        assert!(err.to_string().contains("Too many thumbnails"), "{err}");
    }
}
