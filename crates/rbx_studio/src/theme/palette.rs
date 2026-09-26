//! `theme.json`: parsing one, layering it over Default's, and resolving the
//! result into a [`Palette`].
//!
//! A colour is `#RRGGBB`, `#RRGGBBAA`, or a reference to another colour
//! token — `@dock`, or `@check_on/0.12` for the same colour at another
//! alpha. References resolve *after* the theme is layered over Default, so
//! a theme that only changes `dock` also changes `tile` and `chrome`, which
//! Default defines as `@dock`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use gpui_kit::{point, px, rgb, rgba, BoxShadow, Rgba, WindowBackgroundAppearance};
use serde::Deserialize;

use super::pack::{inside, MAX_IMAGE_BYTES};
use super::{Background, Effects, Fit, Palette};

pub(super) const DEFAULT_THEME_JSON: &str =
    include_str!("../../../../assets/themes/default/theme.json");

/// Far past any real chain; what stops `a: @b, b: @a` looping.
const MAX_REFERENCE_DEPTH: usize = 16;

/// A size is a pixel length before the UI scale. Beyond this it is a typo,
/// and one that would lay the window out off-screen.
const MAX_SIZE: f32 = 2000.;

const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ThemeFile {
    colors: HashMap<String, String>,
    sizes: HashMap<String, f32>,
    window: Option<String>,
    background: Option<BackgroundFile>,
    hover: Option<HoverFile>,
}

#[derive(Debug, Deserialize)]
struct BackgroundFile {
    image: String,
    #[serde(default = "opaque")]
    opacity: f32,
    #[serde(default)]
    fit: Option<String>,
    #[serde(default)]
    layer: Option<String>,
}

fn opaque() -> f32 {
    1.
}

#[derive(Debug, Deserialize)]
struct HoverFile {
    glow: Option<String>,
    #[serde(default = "glow_radius")]
    glow_radius: f32,
}

fn glow_radius() -> f32 {
    8.
}

static DEFAULT_FILE: LazyLock<ThemeFile> = LazyLock::new(|| {
    serde_json::from_str(DEFAULT_THEME_JSON)
        .expect("assets/themes/default/theme.json is valid theme JSON")
});

static BUILTIN: LazyLock<Palette> = LazyLock::new(|| {
    let (palette, warnings) = build(None, None).expect("assets/themes/default/theme.json resolves");
    debug_assert!(warnings.is_empty(), "{warnings:?}");
    palette
});

impl Palette {
    /// Default's palette: the editor's own look.
    pub(crate) fn builtin() -> &'static Palette {
        &BUILTIN
    }
}

pub(super) fn parse(json: &str) -> Result<ThemeFile, String> {
    serde_json::from_str(json).map_err(|err| format!("theme.json: {err}"))
}

/// `theme` layered over Default, with its effects' paths resolved against
/// `dir`. A token Default does not have is skipped and named in the
/// warnings — a theme written for a newer editor should still load — but a
/// value that does not parse is an error: guessing a colour is worse than
/// saying which one is wrong.
pub(super) fn build(
    theme: Option<&ThemeFile>,
    dir: Option<&Path>,
) -> Result<(Palette, Vec<String>), String> {
    let base = &*DEFAULT_FILE;
    let mut warnings = Vec::new();

    let mut raw_colors = base.colors.clone();
    let mut sizes = base.sizes.clone();
    if let Some(theme) = theme {
        for (name, value) in &theme.colors {
            match raw_colors.get_mut(name) {
                Some(slot) => *slot = value.clone(),
                None => warnings.push(format!("unknown colour token {name:?}")),
            }
        }
        for (name, value) in &theme.sizes {
            if !(0.0..=MAX_SIZE).contains(value) {
                return Err(format!("size {name:?} is {value}, outside 0–{MAX_SIZE}"));
            }
            match sizes.get_mut(name) {
                Some(slot) => *slot = *value,
                None => warnings.push(format!("unknown size token {name:?}")),
            }
        }
    }

    // Sorted, so a theme with several mistakes is always told about the
    // same one first.
    let mut names: Vec<&String> = raw_colors.keys().collect();
    names.sort();
    let colors = names
        .into_iter()
        .map(|name| {
            let value = resolve(&raw_colors[name], &raw_colors, 0)
                .map_err(|err| format!("colour {name:?}: {err}"))?;
            Ok((name.clone(), value))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;

    let effects = match theme {
        Some(theme) => effects(theme, dir, &raw_colors)?,
        None => Effects::default(),
    };
    Ok((
        Palette {
            colors,
            sizes,
            effects,
        },
        warnings,
    ))
}

fn resolve(value: &str, raw: &HashMap<String, String>, depth: usize) -> Result<Rgba, String> {
    let value = value.trim();
    let Some(reference) = value.strip_prefix('@') else {
        return hex(value).ok_or_else(|| format!("{value:?} is not #RRGGBB or #RRGGBBAA"));
    };
    if depth >= MAX_REFERENCE_DEPTH {
        return Err(format!("{value:?} is part of a reference loop"));
    }
    let (name, alpha) = match reference.split_once('/') {
        Some((name, alpha)) => (name, Some(alpha)),
        None => (reference, None),
    };
    let target = raw
        .get(name)
        .ok_or_else(|| format!("@{name} is not a colour token"))?;
    let color = resolve(target, raw, depth + 1).map_err(|err| format!("@{name}: {err}"))?;
    match alpha {
        // Parsed as `f32` straight from the text, as a Rust `0.12` literal
        // is, so Default's `@check_on/0.12` is bit-identical to the
        // `Rgba { a: 0.12, .. }` it replaced.
        Some(alpha) => match alpha.trim().parse::<f32>() {
            Ok(a) if (0.0..=1.0).contains(&a) => Ok(Rgba { a, ..color }),
            _ => Err(format!("{alpha:?} is not an alpha between 0 and 1")),
        },
        None => Ok(color),
    }
}

/// Through `rgb`/`rgba` themselves, so a hex string lands on exactly the
/// floats the Rust literal it replaced did.
fn hex(value: &str) -> Option<Rgba> {
    let digits = value.strip_prefix('#')?;
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let bits = u32::from_str_radix(digits, 16).ok()?;
    match digits.len() {
        6 => Some(rgb(bits)),
        8 => Some(rgba(bits)),
        _ => None,
    }
}

fn effects(
    theme: &ThemeFile,
    dir: Option<&Path>,
    raw: &HashMap<String, String>,
) -> Result<Effects, String> {
    let window = match theme.window.as_deref().map(str::trim) {
        None | Some("opaque") => WindowBackgroundAppearance::Opaque,
        Some("transparent") => WindowBackgroundAppearance::Transparent,
        Some("blurred") => WindowBackgroundAppearance::Blurred,
        Some(other) => {
            return Err(format!(
                "window {other:?} is not \"opaque\", \"transparent\" or \"blurred\""
            ))
        }
    };

    let background = match &theme.background {
        Some(background) => Some(self::background(background, dir)?),
        None => None,
    };

    let hover_glow = match &theme.hover {
        Some(HoverFile {
            glow: Some(glow),
            glow_radius,
        }) => {
            let color = resolve(glow, raw, 0).map_err(|err| format!("hover glow: {err}"))?;
            if !(0.0..=64.0).contains(glow_radius) {
                return Err(format!("hover glow_radius {glow_radius} is outside 0–64"));
            }
            Some(BoxShadow {
                color: color.into(),
                offset: point(px(0.), px(0.)),
                blur_radius: px(*glow_radius),
                spread_radius: px(0.),
                inset: false,
            })
        }
        _ => None,
    };

    Ok(Effects {
        window,
        background,
        hover_glow,
    })
}

fn background(file: &BackgroundFile, dir: Option<&Path>) -> Result<Background, String> {
    let dir = dir.ok_or("a background image needs a theme folder to live in")?;
    let path = inside(dir, &file.image)
        .ok_or_else(|| format!("background image {:?} is not inside the theme", file.image))?;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!(
            "background image {:?} is not one of {IMAGE_EXTENSIONS:?}",
            file.image
        ));
    }
    let meta = std::fs::metadata(&path)
        .map_err(|err| format!("background image {:?}: {err}", file.image))?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "background image {:?} is not a file under {} MiB",
            file.image,
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    if !(0.0..=1.0).contains(&file.opacity) {
        return Err(format!(
            "background opacity {} is outside 0–1",
            file.opacity
        ));
    }
    let fit = match file.fit.as_deref() {
        None | Some("cover") => Fit::Cover,
        Some("contain") => Fit::Contain,
        Some("fill") => Fit::Fill,
        Some(other) => {
            return Err(format!(
                "background fit {other:?} is not \"cover\", \"contain\" or \"fill\""
            ))
        }
    };
    let over = match file.layer.as_deref() {
        None | Some("behind") => false,
        Some("over") => true,
        Some(other) => {
            return Err(format!(
                "background layer {other:?} is not \"behind\" or \"over\""
            ))
        }
    };
    Ok(Background {
        path,
        opacity: file.opacity,
        fit,
        over,
    })
}

/// Every colour and size token name Default defines, for tests elsewhere.
#[cfg(test)]
pub(super) fn default_names() -> (Vec<String>, Vec<String>) {
    (
        DEFAULT_FILE.colors.keys().cloned().collect(),
        DEFAULT_FILE.sizes.keys().cloned().collect(),
    )
}
