//! User-defined starter scripts, read from disk so people can add and share
//! them without a rebuild — the same idea as the editor's icon and theme
//! packs, for what the Model menu and the ribbon's Script tile seed a new
//! script with.
//!
//! Layout, under the config directory `settings.rs` already owns:
//!
//! ```text
//! <config>/script_templates/
//!   Script/Default.luau          replaces the built-in `Script` starter
//!   Script/Enemy AI.luau         an extra entry named "Enemy AI"
//!   LocalScript/…
//!   ModuleScript/…
//! ```
//!
//! One folder per class because a template only makes sense for the class it
//! is inserted as, and a folder says so without a naming convention to parse
//! or a header to strip out of the source. A file called `Default.luau` is
//! not listed: it takes the place of that class's built-in starter, so
//! "every new `Script` looks like this" needs no second setting. Anything
//! else with a `.luau` extension is an extra template named by its file
//! stem.
//!
//! Read once at startup. A template file is data a stranger may have
//! authored, so the loader refuses what it cannot use rather than guessing:
//! an unreadable or non-UTF-8 file, one over [`MAX_BYTES`], a folder that is
//! not a script class. None of those stops the editor starting.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::settings::default_config_dir;

/// The classes a template can be for. `LuaSourceContainer` subclasses with a
/// `Source` a user would author by hand; anything else in the directory is
/// ignored.
pub(crate) const CLASSES: [&str; 3] = ["Script", "LocalScript", "ModuleScript"];

/// A starter is source text a person types by hand; 256 KiB is far past any
/// real one and keeps a stray binary or generated file from being read into
/// memory and pasted into a `Source` property.
const MAX_BYTES: u64 = 256 * 1024;

/// The stem that replaces a class's built-in starter instead of being listed.
const DEFAULT_STEM: &str = "Default";

/// One extra template: `class` is what gets inserted, `name` what the menu
/// shows, `source` what the new script starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Template {
    pub(crate) class: &'static str,
    pub(crate) name: String,
    pub(crate) source: String,
}

/// Everything loaded from a templates directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScriptTemplates {
    defaults: HashMap<&'static str, String>,
    extras: Vec<Template>,
}

impl ScriptTemplates {
    /// The user's templates, or none at all when there is no config
    /// directory or nothing in it.
    pub(crate) fn load() -> Self {
        default_config_dir()
            .map(|dir| Self::load_from(&dir.join("script_templates")))
            .unwrap_or_default()
    }

    pub(crate) fn load_from(dir: &Path) -> Self {
        let mut templates = Self::default();
        for class in CLASSES {
            let Ok(entries) = fs::read_dir(dir.join(class)) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("luau") {
                    continue;
                }
                let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Some(source) = read_source(&path) else {
                    continue;
                };
                if name == DEFAULT_STEM {
                    templates.defaults.insert(class, source);
                } else {
                    templates.extras.push(Template {
                        class,
                        name: name.to_owned(),
                        source,
                    });
                }
            }
        }
        // Directory order is whatever the filesystem says; a menu that
        // reshuffles between machines is not one anybody can learn.
        templates
            .extras
            .sort_by_cached_key(|t| (t.class, t.name.to_lowercase(), t.name.clone()));
        templates
    }

    /// The user's replacement for `class`'s built-in starter, if they wrote
    /// one.
    pub(crate) fn default_for(&self, class: &str) -> Option<&str> {
        self.defaults.get(class).map(String::as_str)
    }

    /// Every extra template, grouped by class and alphabetical within it.
    pub(crate) fn extras(&self) -> &[Template] {
        &self.extras
    }
}

fn read_source(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// A fresh, empty directory per call: tests run in parallel and must not
    /// see each other's files.
    fn scratch() -> std::path::PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rbx-native-script-templates-{}-{}",
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
    fn a_missing_directory_loads_as_nothing() {
        let templates = ScriptTemplates::load_from(&scratch().join("absent"));
        assert_eq!(templates, ScriptTemplates::default());
        assert!(templates.extras().is_empty());
        assert_eq!(templates.default_for("Script"), None);
    }

    #[test]
    fn a_file_is_an_extra_template_named_by_its_stem() {
        let dir = scratch();
        write(&dir, "ModuleScript/Enemy AI.luau", b"return {}\n");
        let templates = ScriptTemplates::load_from(&dir);
        assert_eq!(
            templates.extras(),
            [Template {
                class: "ModuleScript",
                name: "Enemy AI".into(),
                source: "return {}\n".into(),
            }]
        );
    }

    #[test]
    fn default_luau_replaces_the_built_in_starter_and_is_not_listed() {
        let dir = scratch();
        write(&dir, "Script/Default.luau", b"print('mine')\n");
        let templates = ScriptTemplates::load_from(&dir);
        assert_eq!(templates.default_for("Script"), Some("print('mine')\n"));
        assert_eq!(templates.default_for("LocalScript"), None);
        assert!(templates.extras().is_empty());
    }

    #[test]
    fn extras_are_grouped_by_class_and_sorted_by_name_ignoring_case() {
        let dir = scratch();
        write(&dir, "ModuleScript/beta.luau", b"b");
        write(&dir, "Script/Zed.luau", b"z");
        write(&dir, "ModuleScript/Alpha.luau", b"a");
        let listed: Vec<_> = ScriptTemplates::load_from(&dir)
            .extras()
            .iter()
            .map(|t| (t.class, t.name.clone()))
            .collect();
        assert_eq!(
            listed,
            [
                ("ModuleScript", "Alpha".to_owned()),
                ("ModuleScript", "beta".to_owned()),
                ("Script", "Zed".to_owned()),
            ]
        );
    }

    #[test]
    fn only_luau_files_in_a_script_class_folder_count() {
        let dir = scratch();
        write(&dir, "Script/notes.txt", b"x");
        write(&dir, "Script/readme", b"x");
        write(&dir, "Part/Thing.luau", b"x");
        write(&dir, "Thing.luau", b"x");
        assert!(ScriptTemplates::load_from(&dir).extras().is_empty());
    }

    #[test]
    fn a_file_that_is_not_utf8_is_skipped_rather_than_failing_the_load() {
        let dir = scratch();
        write(&dir, "Script/Bad.luau", &[0xff, 0xfe, 0x00]);
        write(&dir, "Script/Good.luau", b"ok");
        let templates = ScriptTemplates::load_from(&dir);
        assert_eq!(templates.extras().len(), 1);
        assert_eq!(templates.extras()[0].name, "Good");
    }

    #[test]
    fn a_file_over_the_size_limit_is_skipped() {
        let dir = scratch();
        write(
            &dir,
            "Script/Huge.luau",
            &vec![b'a'; MAX_BYTES as usize + 1],
        );
        assert!(ScriptTemplates::load_from(&dir).extras().is_empty());
    }
}
