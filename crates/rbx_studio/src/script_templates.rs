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
//! The files are the only copy: `store` writes straight into this layout
//! (so a template somebody shared, or edited by hand, behaves exactly like
//! one made in the editor), and `Shell` reloads the whole directory whenever
//! its fingerprint changes (see `Shell::watch_script_templates`). A template
//! file is data a stranger may have authored, so the loader refuses what it
//! cannot use rather than guessing — an unreadable or non-UTF-8 file, one
//! over [`MAX_BYTES`] — and keeps a [`Skipped`] entry saying so. A file that
//! is not `.luau`, or a folder that is not a script class, is simply not a
//! template and is ignored. None of those stops the editor starting.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::settings::default_config_dir;

// The write path lands before the window that calls it.
#[cfg_attr(not(test), allow(dead_code))]
mod names;
#[cfg_attr(not(test), allow(dead_code))]
mod store;

/// The classes a template can be for. `LuaSourceContainer` subclasses with a
/// `Source` a user would author by hand; anything else in the directory is
/// ignored.
pub(crate) const CLASSES: [&str; 3] = ["Script", "LocalScript", "ModuleScript"];

/// A starter is source text a person types by hand; 256 KiB is far past any
/// real one and keeps a stray binary or generated file from being read into
/// memory and pasted into a `Source` property.
pub(crate) const MAX_BYTES: u64 = 256 * 1024;

/// The stem that replaces a class's built-in starter instead of being listed.
pub(crate) const DEFAULT_STEM: &str = "Default";

const EXTENSION: &str = "luau";

/// One extra template: `class` is what gets inserted, `name` what the menu
/// shows, `source` what the new script starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Template {
    pub(crate) class: &'static str,
    pub(crate) name: String,
    pub(crate) source: String,
}

/// Why the loader left a `.luau` file out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkipReason {
    NotUtf8,
    TooLarge,
    Unreadable,
}

impl Skipped {
    /// The one-line reason the templates list shows under the file name.
    pub(crate) fn summary(&self) -> String {
        match self.reason {
            SkipReason::NotUtf8 => "Not UTF-8 text".to_owned(),
            SkipReason::TooLarge => format!(
                "{} KiB, over the {} KiB limit",
                self.len.div_ceil(1024),
                MAX_BYTES / 1024
            ),
            SkipReason::Unreadable => "Couldn\u{2019}t be read".to_owned(),
        }
    }
}

/// A `.luau` file in a class folder that the loader refused. `file_name`
/// keeps the extension: it is shown as the file, never as a template name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Skipped {
    pub(crate) class: &'static str,
    pub(crate) file_name: String,
    pub(crate) reason: SkipReason,
    /// Its size in bytes, 0 when even that couldn't be read.
    pub(crate) len: u64,
}

/// Everything loaded from a templates directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScriptTemplates {
    /// `None` only when there is no config directory at all, which also
    /// makes every `store` write fail rather than land somewhere surprising.
    dir: Option<PathBuf>,
    defaults: HashMap<&'static str, String>,
    extras: Vec<Template>,
    skipped: Vec<Skipped>,
}

impl ScriptTemplates {
    /// The user's templates, or none at all when there is no config
    /// directory or nothing in it.
    pub(crate) fn load() -> Self {
        match dir() {
            Some(dir) => Self::load_from(&dir),
            None => Self::default(),
        }
    }

    pub(crate) fn load_from(dir: &Path) -> Self {
        let mut templates = Self {
            dir: Some(dir.to_owned()),
            ..Self::default()
        };
        for class in CLASSES {
            let Ok(entries) = fs::read_dir(dir.join(class)) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
                    continue;
                }
                let (Some(name), Some(file_name)) = (
                    path.file_stem().and_then(|s| s.to_str()),
                    path.file_name().and_then(|s| s.to_str()),
                ) else {
                    continue;
                };
                let source = match read_source(&path) {
                    Ok(Some(source)) => source,
                    Ok(None) => continue,
                    Err(reason) => {
                        templates.skipped.push(Skipped {
                            class,
                            file_name: file_name.to_owned(),
                            reason,
                            len: fs::metadata(&path).map_or(0, |meta| meta.len()),
                        });
                        continue;
                    }
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
        // reshuffles between machines is not one anybody can learn. Classes
        // follow `CLASSES` — the order of the built-in rows these are listed
        // after — rather than alphabetically.
        templates
            .extras
            .sort_by_cached_key(|t| (class_rank(t.class), t.name.to_lowercase(), t.name.clone()));
        templates
            .skipped
            .sort_by_cached_key(|s| (class_rank(s.class), s.file_name.to_lowercase()));
        templates
    }

    /// The directory this was loaded from.
    pub(crate) fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
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

    /// The files the loader refused, grouped like [`Self::extras`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }
}

/// `<config>/script_templates`, whether or not it exists yet.
pub(crate) fn dir() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("script_templates"))
}

/// A class's place in [`CLASSES`]; anything else sorts last.
fn class_rank(class: &str) -> usize {
    CLASSES
        .iter()
        .position(|c| *c == class)
        .unwrap_or(CLASSES.len())
}

/// The file's text, `Ok(None)` for something that is not a file at all (a
/// folder that happens to end in `.luau`), or why it was refused.
fn read_source(path: &Path) -> Result<Option<String>, SkipReason> {
    let meta = fs::metadata(path).map_err(|_| SkipReason::Unreadable)?;
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_BYTES {
        return Err(SkipReason::TooLarge);
    }
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(err) if err.kind() == io::ErrorKind::InvalidData => Err(SkipReason::NotUtf8),
        Err(_) => Err(SkipReason::Unreadable),
    }
}

#[cfg(test)]
mod tests;
