//! User-installable icon packs and themes: the two halves of the editor's
//! look that used to be compiled in.
//!
//! Both live beside `settings.json`, under the config directory
//! `settings::default_config_dir` owns, so a pack survives `rm -rf ~/.cache`
//! the way a setting should and can be published, copied or deleted as
//! ordinary files without forking the project:
//!
//! ```text
//! <config>/icon_packs/<pack>/<Name>.svg     one folder per pack
//! <config>/themes/<theme>/                  a theme folder (see `theme`)
//! <config>/appearance.json                  {"icon_pack": "<pack>", "theme": "<theme>"}
//! ```
//!
//! An icon pack is an *overlay*, not a replacement: whatever it leaves out is
//! still drawn from the built-in kit, and whatever the kit does not cover is
//! still a Lucide glyph (`explorer::resolve_icon`), so a pack of three icons
//! is a valid pack. A file is named by the class it stands for
//! (`Part.svg`) or by the kit's own tile slug (`humanoid-description.svg`),
//! the latter reaching every class that shares a tile — see
//! `class_icons::CLASS_ICON_SLUGS`.
//!
//! A theme can carry an icon pack of its own; a pack chosen here is layered
//! over it (see [`layered`]).
//!
//! Everything here reads files a stranger may have written, so it refuses what
//! it cannot use — an over-large or non-SVG file, a path that is not a plain
//! name — and never fails the editor starting.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::script_templates::ScriptTemplates;
use crate::settings::{default_config_dir, write_atomic};
use crate::theme::ThemePack;

/// A single icon is a 16x16 drawing; 512 KiB is far past any real one and
/// keeps a stray or generated file from being read whole into memory.
const MAX_SVG_BYTES: u64 = 512 * 1024;

fn root() -> Option<PathBuf> {
    default_config_dir()
}

/// Whether `name` is safe to join onto a directory: one plain segment, no
/// separators, no `..`, no drive or device tricks. The name comes out of
/// `appearance.json`, which is data, so it is never trusted to be one.
pub(crate) fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.starts_with('.')
}

// ---------------------------------------------------------------- selection

/// Which installed pack and theme are switched on. `None` is the built-in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Appearance {
    pub(crate) icon_pack: Option<String>,
    pub(crate) theme: Option<String>,
    /// The user's accent, `#RRGGBB`; `None` keeps the theme's.
    pub(crate) accent: Option<String>,
    /// Transform tool colours by tool key (`"move"`), `#RRGGBB` each.
    pub(crate) tools: BTreeMap<String, String>,
}

impl Appearance {
    pub(crate) fn load() -> Self {
        root()
            .map(|dir| Self::load_from(&dir.join("appearance.json")))
            .unwrap_or_default()
    }

    pub(crate) fn load_from(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else {
            return Self::default();
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Self::default();
        };
        let name = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|name| is_plain_name(name))
                .map(str::to_owned)
        };
        let color = |value: &serde_json::Value| {
            value
                .as_str()
                .and_then(crate::accent::parse_hex)
                .map(crate::accent::hex)
        };
        Appearance {
            icon_pack: name("icon_pack"),
            theme: name("theme"),
            accent: value.get("accent").and_then(color),
            tools: value
                .get("tools")
                .and_then(serde_json::Value::as_object)
                .map(|tools| {
                    tools
                        .iter()
                        .filter_map(|(tool, value)| Some((tool.clone(), color(value)?)))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Remembers the chosen icon pack, and nothing else.
    pub(crate) fn save_icon_pack(&self) -> Result<(), String> {
        let Some(dir) = root() else {
            return Err("no config directory".to_owned());
        };
        self.save_icon_pack_to(&dir.join("appearance.json"))
    }

    /// See [`save_key`]: a pack that was briefly unreadable at startup must
    /// not be persisted away by an unrelated change.
    pub(crate) fn save_icon_pack_to(&self, path: &Path) -> Result<(), String> {
        let icon_pack = self.icon_pack.clone().map(serde_json::Value::from);
        save_key(path, "icon_pack", icon_pack)
    }

    /// Remembers the chosen theme, and nothing else.
    pub(crate) fn save_theme(&self) -> Result<(), String> {
        let Some(dir) = root() else {
            return Err("no config directory".to_owned());
        };
        let theme = self.theme.clone().map(serde_json::Value::from);
        save_key(&dir.join("appearance.json"), "theme", theme)
    }

    /// Remembers the accent and the tool colours, and nothing else.
    pub(crate) fn save_colors(&self) -> Result<(), String> {
        let Some(dir) = root() else {
            return Err("no config directory".to_owned());
        };
        self.save_colors_to(&dir.join("appearance.json"))
    }

    pub(crate) fn save_colors_to(&self, path: &Path) -> Result<(), String> {
        save_key(
            path,
            "accent",
            self.accent.clone().map(serde_json::Value::from),
        )?;
        let tools = (!self.tools.is_empty()).then(|| serde_json::json!(self.tools));
        save_key(path, "tools", tools)
    }

    /// What these colours lay over the theme.
    pub(crate) fn overrides(&self) -> crate::theme::Overrides {
        crate::theme::Overrides {
            accent: self.accent.as_deref().and_then(crate::accent::parse_hex),
            tools: self
                .tools
                .iter()
                .filter_map(|(tool, color)| {
                    Some((format!("tool_{tool}"), crate::accent::parse_hex(color)?))
                })
                .collect(),
        }
    }
}

/// Writes one key into the file's own JSON rather than rebuilding the file
/// from what [`Appearance::load_from`] accepted: a value it refused (or a
/// key from a newer version) belongs to whoever wrote it. `None` removes
/// the key. A file that is not a JSON object has nothing worth keeping and
/// is replaced.
fn save_key(path: &Path, key: &str, entry: Option<serde_json::Value>) -> Result<(), String> {
    {
        let mut value = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(object) = value.as_object_mut() {
            match entry {
                Some(entry) => object.insert(key.to_owned(), entry),
                None => object.remove(key),
            };
        }
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        write_atomic(path, &bytes).map_err(|e| e.to_string())
    }
}

/// Everything the editor reads from the config directory beyond
/// `settings.json`, gathered in one pass at startup — before the window
/// exists, where the file reads cannot stall the UI thread, and once, so
/// `appearance.json` is not parsed by three different callers.
pub(crate) struct UserContent {
    pub(crate) appearance: Appearance,
    /// The chosen pack's drawings, for `main` to install
    /// (`class_icons::set_user_pack`) before the place loads and resolves its
    /// first icons. `None` when no pack is chosen or the chosen one would not
    /// load — in which case `appearance.icon_pack` has been cleared too, so
    /// the Explorer's menu never shows a pack as chosen that is not drawn.
    pub(crate) icon_overlay: Option<IconOverlay>,
    /// Every installed pack's name, for the Explorer menu. Listed once: the
    /// menu is rebuilt every frame.
    pub(crate) icon_packs: Vec<String>,
    /// The theme `appearance.json` names, or Default when it names none or
    /// one that will not load — reported on stderr, since no window exists
    /// yet to show it in.
    pub(crate) theme: ThemePack,
    pub(crate) script_templates: ScriptTemplates,
}

impl UserContent {
    pub(crate) fn load() -> Self {
        let mut appearance = Appearance::load();
        let icon_overlay = appearance.icon_pack.as_deref().and_then(IconOverlay::load);
        if icon_overlay.is_none() {
            appearance.icon_pack = None;
        }
        let theme = load_theme(appearance.theme.as_deref());
        UserContent {
            appearance,
            icon_overlay,
            icon_packs: installed_icon_packs(),
            theme,
            script_templates: ScriptTemplates::load(),
        }
    }
}

/// The theme named `id`, or Default. Problems go to stderr: this runs
/// before there is an Output dock to put them in.
pub(crate) fn load_theme(id: Option<&str>) -> ThemePack {
    let Some(id) = id else {
        return ThemePack::builtin();
    };
    match ThemePack::load(id) {
        Ok(pack) => {
            for warning in &pack.warnings {
                eprintln!("rbxstudio: theme {id:?}: {warning}");
            }
            pack
        }
        Err(err) => {
            eprintln!("rbxstudio: theme {id:?} could not be loaded, using Default: {err}");
            ThemePack::builtin()
        }
    }
}

// -------------------------------------------------------------- icon packs

/// `top`'s drawings over `base`'s: a theme's own icons under the pack
/// chosen in the Explorer, so choosing a pack never loses the theme's icons
/// for classes the pack leaves out.
pub(crate) fn layered(base: Option<IconOverlay>, top: Option<IconOverlay>) -> Option<IconOverlay> {
    match (base, top) {
        (Some(mut base), Some(top)) => {
            base.files.extend(top.files);
            Some(base)
        }
        (base, top) => top.or(base),
    }
}

/// One installed pack's SVGs, keyed by lower-cased file stem, read whole at
/// load so the Explorer never touches the disk while it draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IconOverlay {
    files: HashMap<String, Arc<[u8]>>,
}

impl IconOverlay {
    /// An overlay holding one drawing, for tests elsewhere that need a pack
    /// without a directory to read it from.
    #[cfg(test)]
    pub(crate) fn with(stem: &str, svg: &[u8]) -> Self {
        IconOverlay {
            files: HashMap::from([(stem.to_lowercase(), Arc::from(svg))]),
        }
    }

    /// `<config>/icon_packs/<name>`, or `None` when the name is not a plain
    /// one, there is no config directory, or the folder is missing.
    pub(crate) fn load(name: &str) -> Option<Self> {
        if !is_plain_name(name) {
            return None;
        }
        Self::load_from(&root()?.join("icon_packs").join(name))
    }

    pub(crate) fn load_from(dir: &Path) -> Option<Self> {
        let entries = fs::read_dir(dir).ok()?;
        let mut files = HashMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("svg") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if !meta.is_file() || meta.len() > MAX_SVG_BYTES {
                continue;
            }
            if let Ok(bytes) = fs::read(&path) {
                files.insert(stem.to_lowercase(), Arc::from(bytes));
            }
        }
        Some(IconOverlay { files })
    }

    /// The pack's drawing for a class, by its own name first and then by the
    /// kit tile it falls under — so `Part.svg` reaches exactly `Part`, and
    /// `humanoid-description.svg` reaches every class sharing that tile.
    pub(crate) fn svg(&self, class: &str, slug: Option<&str>) -> Option<Arc<[u8]>> {
        self.files
            .get(&class.to_lowercase())
            .or_else(|| slug.and_then(|s| self.files.get(&s.to_lowercase())))
            .cloned()
    }
}

/// Every installed icon pack's name, alphabetical. A pack is a folder.
pub(crate) fn installed_icon_packs() -> Vec<String> {
    root()
        .map(|dir| folder_names(&dir.join("icon_packs")))
        .unwrap_or_default()
}

/// The plain-named sub-folders of `dir`.
fn folder_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| is_plain_name(name))
        .collect();
    names.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
    names
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    fn scratch() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rbx-native-packs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, relative: &str, bytes: &[u8]) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn plain_names_are_one_segment_only() {
        for good in ["Mine", "my pack", "pack-2", "v1.2"] {
            assert!(is_plain_name(good), "{good}");
        }
        for bad in [
            "", ".", "..", "../x", "a/b", "a\\b", "C:", "C:\\x", ".hidden", "a\0b",
        ] {
            assert!(!is_plain_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_appearance_file_selects_the_built_ins() {
        assert_eq!(
            Appearance::load_from(&scratch().join("nope.json")),
            Appearance::default()
        );
    }

    #[test]
    fn a_chosen_icon_pack_round_trips_through_disk() {
        let path = scratch().join("nested").join("appearance.json");
        let chosen = Appearance {
            icon_pack: Some("Mine".into()),
            ..Appearance::default()
        };
        chosen
            .save_icon_pack_to(&path)
            .expect("writes, creating the folder");
        assert_eq!(Appearance::load_from(&path), chosen);
    }

    /// Changing the pack rewrites `icon_pack` and nothing else: a theme the
    /// loader would refuse, and a key it has never heard of, are still there.
    #[test]
    fn saving_the_icon_pack_leaves_every_other_key_as_it_was() {
        let dir = scratch();
        write(
            &dir,
            "appearance.json",
            br#"{"icon_pack":"Old","theme":"../escape","from_a_newer_version":[1,2]}"#,
        );
        let path = dir.join("appearance.json");

        Appearance {
            icon_pack: Some("New".into()),
            ..Appearance::default()
        }
        .save_icon_pack_to(&path)
        .unwrap();

        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["icon_pack"], "New");
        assert_eq!(saved["theme"], "../escape");
        assert_eq!(saved["from_a_newer_version"], serde_json::json!([1, 2]));
    }

    #[test]
    fn choosing_the_built_in_icons_removes_only_the_icon_pack_key() {
        let dir = scratch();
        write(
            &dir,
            "appearance.json",
            br#"{"icon_pack":"Old","theme":"Dusk"}"#,
        );
        let path = dir.join("appearance.json");

        Appearance::default().save_icon_pack_to(&path).unwrap();

        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.get("icon_pack").is_none());
        assert_eq!(saved["theme"], "Dusk");
    }

    #[test]
    fn a_file_that_is_not_a_json_object_is_replaced_rather_than_failing() {
        let dir = scratch();
        write(&dir, "appearance.json", b"[1, 2, 3]");
        let path = dir.join("appearance.json");

        Appearance {
            icon_pack: Some("Mine".into()),
            ..Appearance::default()
        }
        .save_icon_pack_to(&path)
        .unwrap();

        assert_eq!(
            Appearance::load_from(&path).icon_pack.as_deref(),
            Some("Mine")
        );
    }

    #[test]
    fn colours_round_trip_and_leave_the_other_keys_alone() {
        let dir = scratch();
        write(
            &dir,
            "appearance.json",
            br#"{"theme":"Dusk","icon_pack":"Mine"}"#,
        );
        let path = dir.join("appearance.json");
        let colours = Appearance {
            accent: Some("#4C9BE8".into()),
            tools: [("move".to_owned(), "#8FE0B0".to_owned())].into(),
            ..Appearance::default()
        };
        colours.save_colors_to(&path).unwrap();
        let read = Appearance::load_from(&path);
        assert_eq!(read.accent.as_deref(), Some("#4C9BE8"));
        assert_eq!(read.tools, colours.tools);
        assert_eq!(read.theme.as_deref(), Some("Dusk"));
        assert_eq!(read.icon_pack.as_deref(), Some("Mine"));
        let overrides = read.overrides();
        assert_eq!(
            overrides.accent.map(crate::accent::hex).as_deref(),
            Some("#4C9BE8")
        );
        assert_eq!(overrides.tools[0].0, "tool_move");

        Appearance::default().save_colors_to(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.get("accent").is_none() && saved.get("tools").is_none());
        assert_eq!(saved["theme"], "Dusk");
    }

    #[test]
    fn a_colour_that_is_not_one_is_ignored() {
        let dir = scratch();
        write(
            &dir,
            "appearance.json",
            br##"{"accent":"blue","tools":{"move":"#12"}}"##,
        );
        let read = Appearance::load_from(&dir.join("appearance.json"));
        assert_eq!(read.accent, None);
        assert!(read.tools.is_empty());
    }

    #[test]
    fn appearance_refuses_a_name_that_is_a_path() {
        let path = scratch().join("appearance.json");
        fs::write(&path, br#"{"icon_pack":"../../etc","theme":"a/b"}"#).unwrap();
        assert_eq!(Appearance::load_from(&path), Appearance::default());
    }

    #[test]
    fn appearance_survives_malformed_json_and_wrong_types() {
        let dir = scratch();
        write(&dir, "a.json", b"{not json");
        write(&dir, "b.json", br#"{"icon_pack": 7, "theme": ["x"]}"#);
        assert_eq!(
            Appearance::load_from(&dir.join("a.json")),
            Appearance::default()
        );
        assert_eq!(
            Appearance::load_from(&dir.join("b.json")),
            Appearance::default()
        );
    }

    #[test]
    fn an_overlay_answers_by_class_name_then_by_tile_slug_ignoring_case() {
        let dir = scratch();
        write(&dir, "Part.svg", b"<svg/>");
        write(&dir, "humanoid-description.svg", b"<svg id='h'/>");
        let overlay = IconOverlay::load_from(&dir).unwrap();

        assert!(overlay.svg("part", Some("part")).is_some());
        assert!(overlay.svg("PART", None).is_some());
        // No `AccessoryDescription.svg`, but the tile it shares is there.
        assert_eq!(
            overlay
                .svg("AccessoryDescription", Some("humanoid-description"))
                .as_deref(),
            Some(&b"<svg id='h'/>"[..])
        );
        assert_eq!(overlay.svg("Model", Some("model")), None);
    }

    #[test]
    fn a_class_name_file_beats_the_shared_tile_file() {
        let dir = scratch();
        write(&dir, "AccessoryDescription.svg", b"own");
        write(&dir, "humanoid-description.svg", b"tile");
        let overlay = IconOverlay::load_from(&dir).unwrap();
        assert_eq!(
            overlay
                .svg("AccessoryDescription", Some("humanoid-description"))
                .as_deref(),
            Some(&b"own"[..])
        );
        // A class with no file of its own still reaches the shared tile.
        assert_eq!(
            overlay
                .svg("HumanoidDescription", Some("humanoid-description"))
                .as_deref(),
            Some(&b"tile"[..])
        );
    }

    #[test]
    fn only_svg_files_within_the_size_limit_are_loaded() {
        let dir = scratch();
        write(&dir, "Part.svg", b"<svg/>");
        write(&dir, "Model.png", b"png");
        write(&dir, "Huge.svg", &vec![b' '; MAX_SVG_BYTES as usize + 1]);
        let overlay = IconOverlay::load_from(&dir).unwrap();
        assert_eq!(overlay.files.len(), 1);
        assert!(overlay.svg("Part", None).is_some());
    }

    #[test]
    fn a_pack_folder_that_does_not_exist_is_no_overlay() {
        assert_eq!(IconOverlay::load_from(&scratch().join("gone")), None);
    }

    #[test]
    fn packs_are_listed_alphabetically_ignoring_case_and_skip_files_and_hidden_folders() {
        let dir = scratch();
        write(&dir, "beta/x.svg", b"x");
        write(&dir, "Alpha/x.svg", b"x");
        write(&dir, ".hidden/x.svg", b"x");
        write(&dir, "loose-file.txt", b"x");
        assert_eq!(folder_names(&dir), ["Alpha", "beta"]);
        assert!(folder_names(&dir.join("absent")).is_empty());
    }

    #[test]
    fn a_chosen_pack_draws_over_the_themes_icons_and_both_fall_through() {
        let theme = IconOverlay::with("Part", b"theme");
        let mut chosen = IconOverlay::with("Model", b"chosen");
        chosen
            .files
            .insert("part".into(), Arc::from(&b"chosen"[..]));
        let both = layered(Some(theme.clone()), Some(chosen)).unwrap();
        assert_eq!(both.svg("Part", None).as_deref(), Some(&b"chosen"[..]));

        let only_theme = layered(Some(theme.clone()), None).unwrap();
        assert_eq!(only_theme.svg("Part", None).as_deref(), Some(&b"theme"[..]));
        assert_eq!(layered(None, Some(theme.clone())), Some(theme));
        assert_eq!(layered(None, None), None);
    }
}
