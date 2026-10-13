//! A theme on disk: `<config>/themes/<id>/`, read whole and validated into
//! a [`ThemePack`] before anything is applied, so a broken theme is an
//! error message and never a half-switched editor.
//!
//! ```text
//! <config>/themes/<id>/manifest.json   required: name, author, description, version, preview
//! <config>/themes/<id>/theme.json      optional: colours, sizes, effects (see `palette`)
//! <config>/themes/<id>/widgets.json    optional: a GPUI Kit `ThemeSet` for the toolkit's widgets
//! <config>/themes/<id>/icons/*.svg     optional: an icon pack (see `packs::IconOverlay`)
//! ```
//!
//! A plain `<config>/themes/<id>.json` — the widgets-only theme file this
//! editor read before theme folders existed — still loads, as a theme with
//! only a `widgets.json`.

use std::fs;
use std::path::{Component, Path, PathBuf};

use gpui_kit::component::{ThemeConfig, ThemeMode, ThemeSet};
use serde::Deserialize;

use super::palette;
use super::{Palette, DEFAULT_ID, HIGH_CONTRAST_ID, LIGHT_ID};
use crate::packs::{is_plain_name, IconOverlay};
use crate::settings::default_config_dir;

/// A preview or background image; a 4K PNG screenshot is well under this.
pub(crate) const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

/// `manifest.json`, `theme.json`, `widgets.json`: a few kilobytes each.
const MAX_JSON_BYTES: u64 = 1024 * 1024;

const DEFAULT_MANIFEST_JSON: &str = include_str!("../../../../assets/themes/default/manifest.json");
const DEFAULT_WIDGETS_JSON: &str = include_str!("../../../../assets/themes/default/widgets.json");
const LIGHT_WIDGETS_JSON: &str = include_str!("../../../../assets/themes/light/widgets.json");

/// The themes shipped inside the editor besides Default, which is their
/// base: `(id, manifest.json, theme.json, widgets.json)`, in the order a
/// chooser lists them. Their ids are reserved like Default's.
const EMBEDDED: [(&str, &str, &str, &str); 2] = [
    (
        HIGH_CONTRAST_ID,
        include_str!("../../../../assets/themes/high-contrast/manifest.json"),
        include_str!("../../../../assets/themes/high-contrast/theme.json"),
        include_str!("../../../../assets/themes/high-contrast/widgets.json"),
    ),
    (
        LIGHT_ID,
        include_str!("../../../../assets/themes/light/manifest.json"),
        include_str!("../../../../assets/themes/light/theme.json"),
        LIGHT_WIDGETS_JSON,
    ),
];

/// Whether `id` names a theme shipped inside the editor, which no installed
/// theme may shadow or remove.
pub(crate) fn is_reserved(id: &str) -> bool {
    id == DEFAULT_ID || EMBEDDED.iter().any(|(embedded, ..)| *embedded == id)
}

/// Who made a theme and what it is — what a theme chooser shows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Manifest {
    pub(crate) name: String,
    pub(crate) author: String,
    pub(crate) description: String,
    pub(crate) version: String,
    /// Relative to the theme folder.
    pub(crate) preview: String,
}

impl Manifest {
    /// Every field present, non-blank and of a length a chooser can lay out.
    /// With a `dir`, the preview must also be an image inside it.
    pub(crate) fn parse(json: &str, dir: Option<&Path>) -> Result<Self, String> {
        let manifest: Manifest =
            serde_json::from_str(json).map_err(|err| format!("manifest.json: {err}"))?;
        for (field, value, max) in [
            ("name", &manifest.name, 64),
            ("author", &manifest.author, 64),
            ("description", &manifest.description, 500),
            ("version", &manifest.version, 32),
            ("preview", &manifest.preview, 256),
        ] {
            if value.trim().is_empty() {
                return Err(format!("manifest.json: \"{field}\" is empty"));
            }
            if value.chars().count() > max {
                return Err(format!(
                    "manifest.json: \"{field}\" is longer than {max} characters"
                ));
            }
        }
        if let Some(dir) = dir {
            let preview = inside(dir, &manifest.preview).ok_or_else(|| {
                format!(
                    "manifest.json: preview {:?} is not inside the theme",
                    manifest.preview
                )
            })?;
            let is_image = matches!(
                preview
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_lowercase)
                    .as_deref(),
                Some("png" | "jpg" | "jpeg" | "webp")
            );
            let fits = fs::metadata(&preview)
                .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_IMAGE_BYTES);
            if !is_image || !fits {
                return Err(format!(
                    "manifest.json: preview {:?} is not a PNG, JPEG or WebP file in the theme",
                    manifest.preview
                ));
            }
        }
        Ok(manifest)
    }
}

/// A theme, loaded and checked, ready for `theme::apply`.
#[derive(Debug, Clone)]
pub(crate) struct ThemePack {
    pub(crate) id: String,
    pub(crate) manifest: Manifest,
    /// Where it was read from; `None` for Default, which is embedded.
    pub(crate) dir: Option<PathBuf>,
    pub(crate) palette: Palette,
    pub(crate) widgets: ThemeConfig,
    /// `None` draws the built-in icon kit.
    pub(crate) icons: Option<IconOverlay>,
    /// Things worth telling the author that did not stop the theme loading.
    pub(crate) warnings: Vec<String>,
}

impl ThemePack {
    pub(crate) fn builtin() -> Self {
        ThemePack {
            id: DEFAULT_ID.to_owned(),
            manifest: Manifest::parse(DEFAULT_MANIFEST_JSON, None)
                .expect("assets/themes/default/manifest.json is a valid manifest"),
            dir: None,
            palette: Palette::builtin().clone(),
            widgets: widgets_config(DEFAULT_WIDGETS_JSON, ThemeMode::Dark)
                .expect("assets/themes/default/widgets.json has a dark theme"),
            icons: None,
            warnings: Vec::new(),
        }
    }

    /// A theme shipped inside the editor: Default, or one of [`EMBEDDED`].
    pub(crate) fn embedded(id: &str) -> Option<Self> {
        if id == DEFAULT_ID {
            return Some(Self::builtin());
        }
        let (id, manifest, theme, widgets) = EMBEDDED.into_iter().find(|(e, ..)| *e == id)?;
        let pack = || -> Result<Self, String> {
            let (palette, warnings) = palette::build(Some(&palette::parse(theme)?), None)?;
            Ok(ThemePack {
                id: id.to_owned(),
                manifest: Manifest::parse(manifest, None)?,
                dir: None,
                widgets: widgets_config(widgets, palette.mode)?,
                palette,
                icons: None,
                warnings,
            })
        };
        Some(pack().unwrap_or_else(|err| panic!("assets/themes/{id} does not load: {err}")))
    }

    /// The theme `appearance.json` names, from the config directory.
    pub(crate) fn load(id: &str) -> Result<Self, String> {
        if let Some(pack) = Self::embedded(id) {
            return Ok(pack);
        }
        let themes = themes_dir().ok_or("there is no config directory")?;
        Self::load_from(&themes, id)
    }

    pub(crate) fn load_from(themes: &Path, id: &str) -> Result<Self, String> {
        if let Some(pack) = Self::embedded(id) {
            return Ok(pack);
        }
        if !is_plain_name(id) {
            return Err(format!("{id:?} is not a theme folder name"));
        }
        let dir = themes.join(id);
        if !dir.is_dir() {
            let legacy = themes.join(format!("{id}.json"));
            if legacy.is_file() {
                return Self::legacy(id, &legacy);
            }
            return Err(format!("theme {id:?} is not installed"));
        }
        Self::read(id, &dir)
    }

    /// The theme in `dir`, installed under `id` — also how the installer
    /// checks a download before it replaces anything.
    pub(super) fn read(id: &str, dir: &Path) -> Result<Self, String> {
        let dir = dir.to_path_buf();
        let manifest = Manifest::parse(&read_json(&dir.join("manifest.json"))?, Some(&dir))?;
        let theme_file = match optional_json(&dir.join("theme.json"))? {
            Some(json) => Some(palette::parse(&json)?),
            None => None,
        };
        let (palette, warnings) = palette::build(theme_file.as_ref(), Some(&dir))?;
        let widgets = match optional_json(&dir.join("widgets.json"))? {
            Some(json) => widgets_config(&json, palette.mode)?,
            None if palette.mode == ThemeMode::Light => {
                widgets_config(LIGHT_WIDGETS_JSON, ThemeMode::Light)?
            }
            None => widgets_config(DEFAULT_WIDGETS_JSON, ThemeMode::Dark)?,
        };
        let icons_dir = dir.join("icons");
        let icons = if icons_dir.is_dir() {
            IconOverlay::load_from(&icons_dir)
        } else {
            None
        };
        Ok(ThemePack {
            id: id.to_owned(),
            manifest,
            dir: Some(dir),
            palette,
            widgets,
            icons,
            warnings,
        })
    }

    fn legacy(id: &str, path: &Path) -> Result<Self, String> {
        let mut pack = Self::builtin();
        pack.widgets = widgets_config(&read_json(path)?, ThemeMode::Dark)?;
        pack.id = id.to_owned();
        pack.manifest = Manifest {
            name: id.to_owned(),
            author: "unknown".to_owned(),
            description: "A widgets-only theme file.".to_owned(),
            version: "0".to_owned(),
            preview: String::new(),
        };
        pack.dir = Some(path.to_path_buf());
        Ok(pack)
    }
}

/// The first theme of the palette's `mode` in a `ThemeSet`, in the file's
/// own order.
///
/// Applied straight to `Theme::dark_theme` (or `light_theme`) rather than
/// through the kit's registry, which ignores a name it already holds — and
/// a theme being edited, or switched back to, always has a name it already
/// holds.
pub(super) fn widgets_config(json: &str, mode: ThemeMode) -> Result<ThemeConfig, String> {
    let set: ThemeSet = serde_json::from_str(json).map_err(|err| format!("widgets.json: {err}"))?;
    set.themes
        .into_iter()
        .find(|theme| theme.mode == mode)
        .ok_or_else(|| format!("widgets.json defines no {} theme", mode.name()))
}

pub(crate) fn themes_dir() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("themes"))
}

/// Default and the other themes shipped inside the editor first, then
/// every installed theme with a valid manifest, by name. What a theme
/// chooser lists.
pub(crate) fn installed(themes: &Path) -> Vec<(String, Manifest)> {
    let mut found: Vec<(String, Manifest)> = fs::read_dir(themes)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|id| is_plain_name(id) && !is_reserved(id))
        .filter_map(|id| {
            let dir = themes.join(&id);
            let json = read_json(&dir.join("manifest.json")).ok()?;
            Some((id, Manifest::parse(&json, Some(&dir)).ok()?))
        })
        .collect();
    found.sort_by_cached_key(|(id, manifest)| (manifest.name.to_lowercase(), id.clone()));
    let embedded = [DEFAULT_ID]
        .into_iter()
        .chain(EMBEDDED.map(|(id, ..)| id))
        .filter_map(ThemePack::embedded)
        .map(|pack| (pack.id, pack.manifest));
    embedded.chain(found).collect()
}

/// The preview image of an installed theme, for a chooser's card. `None`
/// for the built-in themes (they are drawn) and for a preview that is
/// missing, not a file, or outside the theme folder.
pub(crate) fn preview(themes: &Path, id: &str, manifest: &Manifest) -> Option<PathBuf> {
    if is_reserved(id) || !is_plain_name(id) {
        return None;
    }
    inside(&themes.join(id), &manifest.preview).filter(|path| path.is_file())
}

/// Deletes an installed theme's folder. The themes shipped inside the
/// editor are not folders and cannot be removed.
pub(crate) fn uninstall(themes: &Path, id: &str) -> Result<(), String> {
    if is_reserved(id) {
        return Err(format!("the built-in theme {id:?} cannot be uninstalled"));
    }
    if !is_plain_name(id) {
        return Err(format!("{id:?} is not a theme folder name"));
    }
    let dir = themes.join(id);
    if !dir.is_dir() {
        return Err(format!("theme {id:?} is not installed"));
    }
    fs::remove_dir_all(&dir).map_err(|err| format!("could not remove {}: {err}", dir.display()))
}

/// `relative` joined onto `dir`, or `None` when it could reach outside it:
/// an absolute path, a `..`, a drive prefix, or a symlink that resolves
/// elsewhere. Theme files are a stranger's data.
pub(crate) fn inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    let plain = relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if !plain || relative.as_os_str().is_empty() {
        return None;
    }
    let path = dir.join(relative);
    match (path.canonicalize(), dir.canonicalize()) {
        (Ok(real), Ok(root)) if !real.starts_with(&root) => None,
        _ => Some(path),
    }
}

fn read_json(path: &Path) -> Result<String, String> {
    optional_json(path)?.ok_or_else(|| format!("{} is missing", file_label(path)))
}

/// A missing file is `None`; one that is there but unreadable or too big is
/// an error, since the theme plainly meant it to be used.
fn optional_json(path: &Path) -> Result<Option<String>, String> {
    let Ok(meta) = fs::metadata(path) else {
        return Ok(None);
    };
    if !meta.is_file() || meta.len() > MAX_JSON_BYTES {
        return Err(format!("{} is not a file under 1 MiB", file_label(path)));
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|err| format!("{}: {err}", file_label(path)))
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn manifest(preview: &str) -> Manifest {
        Manifest {
            name: "n".into(),
            author: "a".into(),
            description: "d".into(),
            version: "1".into(),
            preview: preview.into(),
        }
    }

    #[test]
    fn preview_is_the_file_inside_the_theme_folder_only() {
        let root = std::env::temp_dir().join(format!("rbx-preview-test-{}", std::process::id()));
        let dir = root.join("mine");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("p.png"), b"x").unwrap();
        assert_eq!(
            preview(&root, "mine", &manifest("p.png")),
            Some(dir.join("p.png"))
        );
        assert_eq!(preview(&root, "mine", &manifest("missing.png")), None);
        assert_eq!(preview(&root, "mine", &manifest("../mine/p.png")), None);
        assert_eq!(preview(&root, DEFAULT_ID, &manifest("p.png")), None);
        let _ = fs::remove_dir_all(&root);
    }
}
