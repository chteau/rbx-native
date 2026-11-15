//! Blurred copies of theme background images. GPUI has no backdrop filter,
//! so a frosted backdrop is the image itself blurred once, cached on disk,
//! and painted in place of the sharp one.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use image::imageops::FilterType;

/// Longest side a blurred copy keeps. Blur throws the fine detail away
/// anyway, and a 4K image at radius 64 would stall theme loading.
pub(super) const MAX_SIDE: u32 = 1280;

/// Where blurred copies live: disposable, so under the cache dir. Tests get
/// their own folder so they never write into the real one.
pub(super) fn cache_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return Some(
            std::env::temp_dir().join(format!("rbx-native-theme-blur-{}", std::process::id())),
        );
    }
    rbx_assets::cache_root().map(|root| root.join("theme-backgrounds"))
}

/// The blurred copy of `source` at `radius` (a Gaussian sigma in the
/// image's own pixels), made on first use and reused while the source keeps
/// its size and modification time. A GIF is blurred from its first frame.
pub(super) fn blurred(source: &Path, radius: f32, cache: &Path) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(source).map_err(|err| err.to_string())?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    radius.to_bits().hash(&mut hasher);
    let target = cache.join(format!("{:016x}.png", hasher.finish()));
    if target.is_file() {
        return Ok(target);
    }

    let mut image = image::open(source).map_err(|err| err.to_string())?;
    let longest = image.width().max(image.height());
    let mut sigma = radius;
    if longest > MAX_SIDE {
        image = image.resize(MAX_SIDE, MAX_SIDE, FilterType::Triangle);
        sigma *= MAX_SIDE as f32 / longest as f32;
    }
    let blurred = image::imageops::fast_blur(&image.to_rgba8(), sigma);

    std::fs::create_dir_all(cache).map_err(|err| err.to_string())?;
    // Written aside then renamed, so a half-written file is never reused.
    let partial = target.with_extension(format!("{}.part", std::process::id()));
    blurred
        .save_with_format(&partial, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    std::fs::rename(&partial, &target).map_err(|err| err.to_string())?;
    Ok(target)
}
