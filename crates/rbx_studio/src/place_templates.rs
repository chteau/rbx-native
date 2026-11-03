//! Your own New templates, from the DevForum request
//! <https://devforum.roblox.com/t/allow-developers-to-save-places-as-studio-templates/241595>,
//! which real Studio never got: a place saved once as a starting point (a
//! lighting setup, a folder layout) that Home's New row then offers beside
//! Baseplate.
//!
//! Each template is `<name>.rbxl` plus an optional `<name>.png` thumbnail in
//! `<config>/place_templates`. The file is the template, so copying one in
//! by hand works too. Every function here takes the directory explicitly so
//! tests can use a scratch one.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use rbx_dom::WeakDom;

use crate::save::{self, Format};

const MAX_NAME_CHARS: usize = 60;
const THUMBNAIL_SIDE: u32 = 256;
const FORBIDDEN: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

pub(crate) fn dir() -> Option<PathBuf> {
    Some(crate::settings::default_config_dir()?.join("place_templates"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaceTemplate {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) thumbnail: Option<PathBuf>,
}

/// Every place file in `dir`, by name; a missing directory is no templates.
pub(crate) fn list(dir: &Path) -> Vec<PlaceTemplate> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PlaceTemplate> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let ext = path.extension()?.to_str()?;
            if !(ext.eq_ignore_ascii_case("rbxl") || ext.eq_ignore_ascii_case("rbxlx")) {
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let png = dir.join(format!("{name}.png"));
            let thumbnail = png.is_file().then_some(png);
            Some(PlaceTemplate {
                name,
                path,
                thumbnail,
            })
        })
        .collect();
    found.sort_by_key(|template| template.name.to_lowercase());
    found
}

/// The trimmed name when it can be a template's, else the message to show
/// under the field.
pub(crate) fn check_name(dir: &Path, typed: &str) -> Result<String, String> {
    let name = typed.trim();
    if name.is_empty() {
        return Err("Give the template a name".to_owned());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("Keep the name under {MAX_NAME_CHARS} characters"));
    }
    if name
        .chars()
        .any(|c| c.is_control() || FORBIDDEN.contains(&c))
    {
        return Err("A name can't have / \\ : * ? \" < > |".to_owned());
    }
    if name.starts_with('.') {
        return Err("A name can't start with a dot".to_owned());
    }
    let lower = name.to_lowercase();
    if list(dir).iter().any(|t| t.name.to_lowercase() == lower) {
        return Err(format!("There's already a template called {name}"));
    }
    Ok(name.to_owned())
}

/// Writes `dom` as `<name>.rbxl` in `dir`, with the thumbnail beside it.
pub(crate) fn save(
    dir: &Path,
    name: &str,
    dom: &WeakDom,
    thumbnail: Option<&[u8]>,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let path = dir.join(format!("{name}.rbxl"));
    save::save(dom, Format::Binary, &path)?;
    // A thumbnail left behind by a template removed by hand must not dress
    // up a new one of the same name.
    let png = dir.join(format!("{name}.png"));
    let _ = match thumbnail {
        Some(bytes) => std::fs::write(png, bytes),
        None => std::fs::remove_file(png),
    };
    Ok(path)
}

pub(crate) fn remove(template: &PlaceTemplate) -> Result<(), String> {
    std::fs::remove_file(&template.path)
        .map_err(|err| format!("{}: {err}", template.path.display()))?;
    if let Some(thumbnail) = &template.thumbnail {
        let _ = std::fs::remove_file(thumbnail);
    }
    Ok(())
}

/// Copies the template into `places` under the first free name, `Name.rbxl`
/// then `Name 2.rbxl`, and so on.
pub(crate) fn instantiate(template: &PlaceTemplate, places: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(places).map_err(|err| format!("{}: {err}", places.display()))?;
    let ext = template
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("rbxl");
    let target = (1..)
        .map(|n| match n {
            1 => places.join(format!("{}.{ext}", template.name)),
            n => places.join(format!("{} {n}.{ext}", template.name)),
        })
        .find(|path| !path.exists())
        .expect("an unbounded range has a free name");
    std::fs::copy(&template.path, &target).map_err(|err| format!("{}: {err}", target.display()))?;
    Ok(target)
}

/// The centre square of an RGBA frame, scaled to a 256x256 PNG.
pub(crate) fn thumbnail_png(width: u32, height: u32, rgba: Vec<u8>) -> Result<Vec<u8>, String> {
    let frame = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "the frame does not match its size".to_owned())?;
    let side = width.min(height);
    let square =
        image::imageops::crop_imm(&frame, (width - side) / 2, (height - side) / 2, side, side)
            .to_image();
    let small = image::imageops::resize(
        &square,
        THUMBNAIL_SIDE,
        THUMBNAIL_SIDE,
        image::imageops::FilterType::Triangle,
    );
    let mut out = Cursor::new(Vec::new());
    small
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rbx_templates_{test}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn one_part_dom() -> WeakDom {
        let mut dom = WeakDom::new();
        dom.new_instance("Workspace", "Workspace", None);
        dom
    }

    #[test]
    fn list_sorts_pairs_thumbnails_and_ignores_others() {
        let dir = scratch("list");
        std::fs::write(dir.join("beta.rbxl"), b"x").unwrap();
        std::fs::write(dir.join("Alpha.rbxlx"), b"x").unwrap();
        std::fs::write(dir.join("Alpha.png"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::create_dir(dir.join("folder.rbxl")).unwrap();
        let found = list(&dir);
        let names: Vec<&str> = found.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta"]);
        assert!(found[0].thumbnail.is_some());
        assert!(found[1].thumbnail.is_none());
        assert!(list(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_name_rules() {
        let dir = scratch("names");
        std::fs::write(dir.join("Lighting.rbxl"), b"x").unwrap();
        assert!(check_name(&dir, "  ").is_err());
        assert!(check_name(&dir, "a/b").is_err());
        assert!(check_name(&dir, ".hidden").is_err());
        assert!(check_name(&dir, &"a".repeat(61)).is_err());
        assert!(check_name(&dir, "lighting")
            .unwrap_err()
            .contains("already"));
        assert_eq!(check_name(&dir, "  Folders ").unwrap(), "Folders");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_list_round_trips() {
        let dir = scratch("save");
        let path = save(&dir, "Mine", &one_part_dom(), Some(b"png")).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        let found = list(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, path);
        assert!(found[0].thumbnail.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn instantiate_picks_the_first_free_name() {
        let dir = scratch("inst");
        let template = PlaceTemplate {
            name: "Name".to_owned(),
            path: dir.join("Name.rbxl"),
            thumbnail: None,
        };
        std::fs::write(&template.path, b"data").unwrap();
        let places = dir.join("places");
        let first = instantiate(&template, &places).unwrap();
        assert_eq!(first, places.join("Name.rbxl"));
        let second = instantiate(&template, &places).unwrap();
        assert_eq!(second, places.join("Name 2.rbxl"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_deletes_both_files() {
        let dir = scratch("remove");
        save(&dir, "Gone", &one_part_dom(), Some(b"png")).unwrap();
        let template = list(&dir).remove(0);
        remove(&template).unwrap();
        assert!(!dir.join("Gone.rbxl").exists());
        assert!(!dir.join("Gone.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thumbnail_is_a_256_square_png() {
        let png = thumbnail_png(4, 2, vec![200; 4 * 2 * 4]).unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
    }
}
