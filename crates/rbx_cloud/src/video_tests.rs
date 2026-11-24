use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use super::*;
use crate::web::serve::serve;

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn mvhd_v0(scale: u32, duration: u32) -> Vec<u8> {
    let mut body = vec![0u8; 12];
    body.extend_from_slice(&scale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&[0u8; 80]);
    boxed(b"mvhd", &body)
}

fn mvhd_v1(scale: u32, duration: u64) -> Vec<u8> {
    let mut body = vec![1, 0, 0, 0];
    body.extend_from_slice(&[0u8; 16]);
    body.extend_from_slice(&scale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&[0u8; 80]);
    boxed(b"mvhd", &body)
}

fn track(handler: &[u8; 4], width: u32, height: u32) -> Vec<u8> {
    let mut tkhd = vec![0u8; 76];
    tkhd.extend_from_slice(&(width << 16).to_be_bytes());
    tkhd.extend_from_slice(&(height << 16).to_be_bytes());
    let mut hdlr = vec![0u8; 8];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0u8; 13]);
    let mdia = boxed(b"mdia", &boxed(b"hdlr", &hdlr));
    boxed(b"trak", &[boxed(b"tkhd", &tkhd), mdia].concat())
}

/// `ftyp`, a stand-in `mdat`, then `moov` last, as a camera writes it.
fn file(mvhd: Vec<u8>, tracks: &[Vec<u8>]) -> Vec<u8> {
    let moov = boxed(b"moov", &[vec![mvhd], tracks.to_vec()].concat().concat());
    [
        boxed(b"ftyp", b"isom\0\0\0\0isom"),
        boxed(b"mdat", &[0u8; 64]),
        moov,
    ]
    .concat()
}

fn probed(bytes: &[u8]) -> Result<VideoInfo, String> {
    probe(&mut Cursor::new(bytes), bytes.len() as u64)
}

#[test]
fn length_and_size_come_from_the_movie_and_its_video_track() {
    let bytes = file(
        mvhd_v0(600, 600 * 95),
        &[track(b"soun", 0, 0), track(b"vide", 1920, 1080)],
    );
    let info = probed(&bytes).unwrap();
    assert_eq!(info.duration, Duration::from_secs(95));
    assert_eq!((info.width, info.height), (1920, 1080));
    assert_eq!(info.bytes, bytes.len() as u64);
    assert_eq!(clock(info.duration), "1:35");
    assert!(limits(&info).is_ok());
}

#[test]
fn a_64_bit_header_and_a_64_bit_box_size_are_read() {
    let moov = [mvhd_v1(1000, 30_500), track(b"vide", 640, 360)].concat();
    let mut large = 1u32.to_be_bytes().to_vec();
    large.extend_from_slice(b"moov");
    large.extend_from_slice(&((moov.len() + 16) as u64).to_be_bytes());
    large.extend_from_slice(&moov);
    let info = probed(&large).unwrap();
    assert_eq!(info.duration, Duration::from_millis(30_500));
    assert_eq!(info.width, 640);
}

#[test]
fn what_roblox_refuses_is_refused_in_words() {
    let ok = VideoInfo {
        duration: MAX_DURATION,
        width: 4096,
        height: 2160,
        bytes: MAX_BYTES - 1,
    };
    assert!(limits(&ok).is_ok(), "the limits themselves are allowed");
    let portrait = VideoInfo {
        width: 2160,
        height: 4096,
        ..ok
    };
    assert!(limits(&portrait).is_ok());

    let long = VideoInfo {
        duration: Duration::from_secs(330),
        ..ok
    };
    assert_eq!(
        limits(&long).unwrap_err(),
        "it's 5:30; Roblox takes videos up to 5 minutes long"
    );
    let big = VideoInfo {
        width: 7680,
        height: 4320,
        ..ok
    };
    assert!(limits(&big).unwrap_err().contains("4096×2160"));
    let heavy = VideoInfo {
        bytes: MAX_BYTES,
        ..ok
    };
    assert!(limits(&heavy).unwrap_err().contains("3.75 GB"));
}

#[test]
fn a_file_without_a_movie_or_video_is_not_taken() {
    assert!(probed(&boxed(b"ftyp", b"isom")).is_err());
    let audio_only = file(mvhd_v0(1, 10), &[track(b"soun", 0, 0)]);
    assert_eq!(
        probed(&audio_only).unwrap_err(),
        "this file has no video track"
    );
    let mut cut = file(mvhd_v0(1, 10), &[track(b"vide", 1, 1)]);
    cut.truncate(cut.len() - 10);
    assert!(probed(&cut).is_err());
}

#[test]
fn only_mp4_and_mov_are_videos() {
    assert_eq!(content_type(Path::new("a/clip.MP4")), Some("video/mp4"));
    assert_eq!(content_type(Path::new("clip.mov")), Some("video/mov"));
    assert_eq!(content_type(Path::new("clip.webm")), None);
    assert!(check(Path::new("clip.webm"))
        .unwrap_err()
        .contains(".mp4 or .mov"));
}

#[test]
fn refusals_are_put_plainly() {
    let refused = |status, message: &str| CloudError::Refused {
        status,
        message: message.into(),
    };
    assert!(explain(&refused(403, "User is not ID verified"))
        .starts_with("Video uploads are only for 13+ ID-verified"));
    assert!(explain(&refused(400, "Video duration exceeds maximum"))
        .starts_with("Roblox says the video is too long"));
    assert!(
        explain(&refused(400, "Insufficient funds")).starts_with("The upload costs 2,000 Robux")
    );
    assert!(explain(&refused(429, "Too many requests")).starts_with("That's the daily limit"));
    assert_eq!(
        explain(&refused(400, "something new")),
        "Roblox refused the video: something new"
    );
}

#[test]
fn a_video_upload_says_its_price_and_reports_pending_review() {
    let (root, served) = serve(vec![
        (200, "", r#"{"path":"operations/v1","done":false}"#),
        (
            200,
            "",
            r#"{"path":"operations/v1","done":true,"response":{"assetId":"77","moderationResult":{"moderationState":"MODERATION_STATE_PENDING"}}}"#,
        ),
    ]);
    let client = Client::new(Some(crate::ApiKey::new("k"))).with_apis_root(root);
    let file = ModelFile {
        name: "clip.mp4",
        content_type: "video/mp4",
        bytes: b"mp4",
    };
    let uploaded = client
        .create_video_asset("Clip", "", Owner::User(5), &file)
        .unwrap();
    assert_eq!(
        uploaded,
        UploadedVideo {
            asset_id: 77,
            moderation_pending: true
        }
    );
    let requests = served.join().unwrap();
    assert!(requests[0].starts_with("POST /assets/v1/assets "));
    assert!(
        requests[0].contains("\"assetType\":\"Video\""),
        "{}",
        requests[0]
    );
    assert!(
        requests[0].contains("\"expectedPrice\":2000"),
        "{}",
        requests[0]
    );
    assert!(requests[0].contains("Content-Type: video/mp4\r\n\r\nmp4"));
    assert!(requests[1].starts_with("GET /assets/v1/operations/v1 "));
}
