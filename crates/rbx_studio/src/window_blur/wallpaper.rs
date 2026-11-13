//! GNOME's wallpaper, which the compositor paints itself (no X window holds
//! it), rebuilt from its settings at the backdrop's reduced scale.

use std::path::PathBuf;
use std::process::Command;

use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};

/// A rectangle in root-window pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub(super) fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w as i32
            && other.x < self.x + self.w as i32
            && self.y < other.y + other.h as i32
            && other.y < self.y + self.h as i32
    }
}

/// `org.gnome.desktop.background picture-options`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fit {
    None,
    Wallpaper,
    Centered,
    Scaled,
    Stretched,
    Zoom,
    Spanned,
}

impl Fit {
    pub(super) fn parse(value: &str) -> Fit {
        match value {
            "none" => Fit::None,
            "wallpaper" => Fit::Wallpaper,
            "centered" => Fit::Centered,
            "scaled" => Fit::Scaled,
            "stretched" => Fit::Stretched,
            "spanned" => Fit::Spanned,
            _ => Fit::Zoom,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Settings {
    pub image: Option<PathBuf>,
    pub fit: Fit,
    pub color: [u8; 3],
}

fn gsetting(schema: &str, key: &str) -> Option<String> {
    let out = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| unquote(String::from_utf8_lossy(&out.stdout).trim()))
}

/// A GVariant string as `gsettings` prints it, `'…'`, unquoted.
pub(super) fn unquote(value: &str) -> String {
    value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or(value)
        .replace("\\'", "'")
}

pub(super) fn parse_color(value: &str) -> Option<[u8; 3]> {
    let hex = value.strip_prefix('#')?;
    let channel = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    (hex.len() == 6).then_some([channel(0)?, channel(2)?, channel(4)?])
}

pub(super) fn read_settings() -> Settings {
    const SCHEMA: &str = "org.gnome.desktop.background";
    let dark = gsetting("org.gnome.desktop.interface", "color-scheme")
        .is_some_and(|scheme| scheme == "prefer-dark");
    let uri = match dark {
        true => gsetting(SCHEMA, "picture-uri-dark").filter(|uri| !uri.is_empty()),
        false => None,
    }
    .or_else(|| gsetting(SCHEMA, "picture-uri"));
    Settings {
        image: uri.and_then(|uri| url::Url::parse(&uri).ok()?.to_file_path().ok()),
        fit: gsetting(SCHEMA, "picture-options").map_or(Fit::Zoom, |fit| Fit::parse(&fit)),
        color: gsetting(SCHEMA, "primary-color")
            .and_then(|color| parse_color(&color))
            .unwrap_or([0x02, 0x3c, 0x88]),
    }
}

/// Where an image of `size` lands in `area` (one monitor, or the whole
/// screen when spanned). `None` for no image and for tiling, which repeats
/// rather than placing.
pub(super) fn placement(fit: Fit, size: (u32, u32), area: Rect) -> Option<Rect> {
    let (iw, ih) = (size.0.max(1) as f64, size.1.max(1) as f64);
    let (aw, ah) = (area.w as f64, area.h as f64);
    let (w, h) = match fit {
        Fit::None | Fit::Wallpaper => return None,
        Fit::Centered => (iw, ih),
        Fit::Stretched => (aw, ah),
        Fit::Scaled => {
            let scale = (aw / iw).min(ah / ih);
            (iw * scale, ih * scale)
        }
        Fit::Zoom | Fit::Spanned => {
            let scale = (aw / iw).max(ah / ih);
            (iw * scale, ih * scale)
        }
    };
    Some(Rect {
        x: area.x + ((aw - w) / 2.).round() as i32,
        y: area.y + ((ah - h) / 2.).round() as i32,
        w: w.round() as u32,
        h: h.round() as u32,
    })
}

fn scaled(rect: Rect, scale: u32) -> Rect {
    let s = scale as i32;
    Rect {
        x: rect.x.div_euclid(s),
        y: rect.y.div_euclid(s),
        w: rect.w.div_ceil(scale),
        h: rect.h.div_ceil(scale),
    }
}

/// The whole screen's wallpaper at `1/scale`, in BGRA like everything the
/// worker composites.
pub(super) fn canvas(
    settings: &Settings,
    image: Option<&RgbaImage>,
    root: (u32, u32),
    monitors: &[Rect],
    scale: u32,
) -> RgbaImage {
    let [r, g, b] = settings.color;
    let ground = Rgba([b, g, r, 255]);
    let screen = Rect {
        x: 0,
        y: 0,
        w: root.0,
        h: root.1,
    };
    let small = scaled(screen, scale);
    let mut canvas = RgbaImage::from_pixel(small.w, small.h, ground);
    let Some(image) = image else {
        return canvas;
    };
    let size = image.dimensions();
    if settings.fit == Fit::Wallpaper {
        let tile = scaled(
            Rect {
                x: 0,
                y: 0,
                w: size.0,
                h: size.1,
            },
            scale,
        );
        let tile = imageops::resize(image, tile.w.max(1), tile.h.max(1), FilterType::Triangle);
        imageops::tile(&mut canvas, &tile);
        return canvas;
    }
    let areas = match (settings.fit, monitors.is_empty()) {
        (Fit::Spanned, _) | (_, true) => vec![screen],
        _ => monitors.to_vec(),
    };
    for area in areas {
        let Some(at) = placement(settings.fit, size, area) else {
            continue;
        };
        let (area, at) = (scaled(area, scale), scaled(at, scale));
        let fitted = imageops::resize(image, at.w.max(1), at.h.max(1), FilterType::Triangle);
        // Drawn into the monitor's own patch first, so a zoomed image is
        // cropped to its monitor instead of spilling onto the next one.
        let mut patch = RgbaImage::from_pixel(area.w, area.h, ground);
        imageops::overlay(
            &mut patch,
            &fitted,
            (at.x - area.x) as i64,
            (at.y - area.y) as i64,
        );
        imageops::overlay(&mut canvas, &patch, area.x as i64, area.y as i64);
    }
    canvas
}

/// The image, decoded and turned to BGRA. Shrunk first: the canvas is a
/// quarter of the screen, so a 4K source would only cost decode time.
pub(super) fn load(settings: &Settings) -> Option<RgbaImage> {
    let path = settings.image.as_ref()?;
    let image = image::open(path)
        .map_err(|err| eprintln!("rbxstudio: wallpaper {}: {err}", path.display()))
        .ok()?;
    let mut image = image.thumbnail(1920, 1920).to_rgba8();
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(image)
}

/// The dconf database's modification time: a cheap way to notice a new
/// wallpaper without asking `gsettings` on every refresh.
pub(super) fn settings_stamp() -> Option<std::time::SystemTime> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    std::fs::metadata(config.join("dconf/user"))
        .ok()?
        .modified()
        .ok()
}
