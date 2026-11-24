//! `Video` uploads through the Assets API, with the limits Roblox documents
//! checked first so a file it would refuse never costs an upload.
//!
//! `creator-docs`, `cloud/guides/usage-assets.md` (the asset type table) and
//! `ui/video-frames.md`: `.mp4` or `.mov` (`video/mp4`, `video/mov`), at
//! most 5 minutes, at most 4096×2160, under 3.75 GB, 20 uploads a day, for
//! 13+ ID-verified accounts, 2,000 Robux per upload. The Create Asset
//! schema's `expectedPrice` makes Roblox fail the upload "with a 400 error"
//! rather than charge more than that.
//!
//! The duration and resolution are read from the file's own ISO base media
//! boxes (`moov/mvhd` and each video track's `tkhd`), which `.mp4` and
//! QuickTime `.mov` share; only the boxes are read, the media is skipped.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::client::Client;
use crate::create_asset::ModelFile;
use crate::error::CloudError;
use crate::universes::Owner;

pub const MAX_DURATION: Duration = Duration::from_secs(5 * 60);
pub const MAX_LONG_SIDE: u32 = 4096;
pub const MAX_SHORT_SIDE: u32 = 2160;
/// "Less than 3.75 GB"; read as binary gigabytes, the larger reading, so
/// a file Roblox might take is never refused here.
pub const MAX_BYTES: u64 = 3 * (1 << 30) + 3 * (1 << 28);
/// What Roblox charges for one video upload, sent as `expectedPrice`.
pub const UPLOAD_PRICE: u64 = 2000;

/// What a video file says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoInfo {
    pub duration: Duration,
    /// The largest video track's display size.
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// The content type the asset table pairs with the file's extension.
pub fn content_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/mov"),
        _ => None,
    }
}

/// Reads the file's duration and size, and refuses what Roblox documents it
/// refuses, in words for the person who picked the file.
pub fn check(path: &Path) -> Result<VideoInfo, String> {
    if content_type(path).is_none() {
        return Err("Roblox takes video as .mp4 or .mov".into());
    }
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let info = probe(&mut file, bytes)?;
    limits(&info)?;
    Ok(info)
}

pub fn limits(info: &VideoInfo) -> Result<(), String> {
    if info.bytes >= MAX_BYTES {
        return Err(format!(
            "it's {:.2} GB; Roblox takes videos under 3.75 GB",
            info.bytes as f64 / (1u64 << 30) as f64
        ));
    }
    if info.duration > MAX_DURATION {
        return Err(format!(
            "it's {}; Roblox takes videos up to 5 minutes long",
            clock(info.duration)
        ));
    }
    let (long, short) = (info.width.max(info.height), info.width.min(info.height));
    if long > MAX_LONG_SIDE || short > MAX_SHORT_SIDE {
        return Err(format!(
            "it's {}×{}; Roblox takes videos up to 4096×2160",
            info.width, info.height
        ));
    }
    Ok(())
}

/// `m:ss`, how a video's length is usually written.
pub fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs_f64().round() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Walks the top-level boxes of a `len`-byte file for `moov` and reads it.
pub fn probe<R: Read + Seek>(reader: &mut R, len: u64) -> Result<VideoInfo, String> {
    let mut at = 0;
    while at < len {
        let Some((kind, body, end)) = header(reader, at, len)? else {
            break;
        };
        if &kind == b"moov" {
            let mut moov = vec![0; usize::try_from(end - body).map_err(|e| e.to_string())?];
            reader
                .seek(SeekFrom::Start(body))
                .map_err(|e| e.to_string())?;
            reader.read_exact(&mut moov).map_err(|e| e.to_string())?;
            return movie(&moov, len);
        }
        at = end;
    }
    Err("this file holds no movie header; is it really a video?".into())
}

/// One box header at `at`: its type, where its body starts and where it ends.
fn header<R: Read + Seek>(
    reader: &mut R,
    at: u64,
    len: u64,
) -> Result<Option<([u8; 4], u64, u64)>, String> {
    if len - at < 8 {
        return Ok(None);
    }
    let mut head = [0u8; 16];
    reader
        .seek(SeekFrom::Start(at))
        .map_err(|e| e.to_string())?;
    reader
        .read_exact(&mut head[..8])
        .map_err(|e| e.to_string())?;
    let size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
    let kind = [head[4], head[5], head[6], head[7]];
    let (size, body) = match size {
        0 => (len - at, at + 8),
        1 => {
            reader
                .read_exact(&mut head[8..])
                .map_err(|e| e.to_string())?;
            (u64::from_be_bytes(head[8..16].try_into().unwrap()), at + 16)
        }
        size => (size, at + 8),
    };
    if size < body - at || at + size > len {
        return Err("the video's boxes are cut short or damaged".into());
    }
    Ok(Some((kind, body, at + size)))
}

/// The boxes directly inside `data`, as (type, body).
fn children(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let head = data.get(at..at + 8)?;
        let size = u32::from_be_bytes(head[..4].try_into().ok()?) as usize;
        let kind: [u8; 4] = head[4..8].try_into().ok()?;
        let (size, body) = match size {
            0 => (data.len() - at, at + 8),
            1 => (
                usize::try_from(u64::from_be_bytes(
                    data.get(at + 8..at + 16)?.try_into().ok()?,
                ))
                .ok()?,
                at + 16,
            ),
            size => (size, at + 8),
        };
        let end = at
            .checked_add(size)
            .filter(|&end| end <= data.len() && size >= body - at)?;
        let found = (kind, &data[body..end]);
        at = end;
        Some(found)
    })
}

fn movie(moov: &[u8], bytes: u64) -> Result<VideoInfo, String> {
    let mvhd = children(moov)
        .find(|(kind, _)| kind == b"mvhd")
        .map(|(_, body)| body)
        .ok_or("the video has no movie header (mvhd)")?;
    let duration = mvhd_duration(mvhd).ok_or("the video's movie header is damaged")?;
    let (mut width, mut height) = (0, 0);
    for (_, trak) in children(moov).filter(|(kind, _)| kind == b"trak") {
        if !is_video_track(trak) {
            continue;
        }
        let Some((w, h)) = children(trak)
            .find(|(kind, _)| kind == b"tkhd")
            .and_then(|(_, tkhd)| tkhd_size(tkhd))
        else {
            continue;
        };
        if u64::from(w) * u64::from(h) > u64::from(width) * u64::from(height) {
            (width, height) = (w, h);
        }
    }
    if width == 0 || height == 0 {
        return Err("this file has no video track".into());
    }
    Ok(VideoInfo {
        duration,
        width,
        height,
        bytes,
    })
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn be64(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// `mvhd` version 0 keeps 32-bit times, version 1 64-bit ones.
fn mvhd_duration(mvhd: &[u8]) -> Option<Duration> {
    let (scale, duration) = match mvhd.first()? {
        0 => (be32(mvhd, 12)?, u64::from(be32(mvhd, 16)?)),
        1 => (be32(mvhd, 20)?, be64(mvhd, 24)?),
        _ => return None,
    };
    (scale > 0).then(|| Duration::from_secs_f64(duration as f64 / f64::from(scale)))
}

/// A `tkhd` ends with the track's width and height in 16.16 fixed point.
fn tkhd_size(tkhd: &[u8]) -> Option<(u32, u32)> {
    let at = tkhd.len().checked_sub(8)?;
    Some((be32(tkhd, at)? >> 16, be32(tkhd, at + 4)? >> 16))
}

/// `trak/mdia/hdlr` names a video track `vide`.
fn is_video_track(trak: &[u8]) -> bool {
    children(trak)
        .filter(|(kind, _)| kind == b"mdia")
        .flat_map(|(_, mdia)| children(mdia))
        .any(|(kind, hdlr)| &kind == b"hdlr" && hdlr.get(8..12) == Some(&b"vide"[..]))
}

/// A finished video upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedVideo {
    pub asset_id: u64,
    /// Roblox still has it in review; it plays once approved.
    pub moderation_pending: bool,
}

impl Client {
    /// Uploads `file` (checked with [`check`] first) as a new `Video` asset
    /// owned by `creator`, for at most [`UPLOAD_PRICE`] Robux, and waits for
    /// Roblox to take it.
    pub fn create_video_asset(
        &self,
        display_name: &str,
        description: &str,
        creator: Owner,
        file: &ModelFile,
    ) -> Result<UploadedVideo, CloudError> {
        let response = self.create_asset_priced(
            "Video",
            display_name,
            description,
            creator,
            file,
            Some(UPLOAD_PRICE),
        )?;
        let state = response
            .pointer("/moderationResult/moderationState")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        Ok(UploadedVideo {
            asset_id: crate::create_asset::asset_id(&response)?,
            moderation_pending: state.contains("pending") || state.contains("review"),
        })
    }
}

/// Roblox's refusal of a video upload in plain words, keeping its own
/// message after them. The Assets API documents no video-specific codes,
/// so this goes by status and by what the message is about.
pub fn explain(err: &CloudError) -> String {
    let (status, said) = match err {
        CloudError::Refused { status, message } => (*status, message.clone()),
        other => return other.to_string(),
    };
    let lower = said.to_ascii_lowercase();
    let plain = if lower.contains("verif") || lower.contains("eligib") || lower.contains("13+") {
        "Video uploads are only for 13+ ID-verified accounts, and Roblox says this one isn't eligible."
    } else if lower.contains("duration") || lower.contains("too long") || lower.contains("length") {
        "Roblox says the video is too long: 5 minutes at most."
    } else if lower.contains("resolution") || lower.contains("dimension") {
        "Roblox says the video is too big: 4096×2160 at most."
    } else if lower.contains("robux")
        || lower.contains("fund")
        || lower.contains("price")
        || lower.contains("balance")
    {
        "The upload costs 2,000 Robux and the account couldn't pay it (or Roblox now asks more)."
    } else if status == 429 || lower.contains("quota") || lower.contains("limit") {
        "That's the daily limit: Roblox takes 20 video uploads a day."
    } else if lower.contains("moderat") || lower.contains("reject") {
        "Roblox moderation turned the video down."
    } else {
        return format!("Roblox refused the video: {said}");
    };
    format!("{plain} (Roblox: {said})")
}

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
