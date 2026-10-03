//! Writing templates: every change is a file operation in the layout the
//! loader reads, checked against the name rules first. Nothing here updates
//! the in-memory list — the caller reloads the directory afterwards, so what
//! the editor shows is always what is on disk.
//!
//! Deleting is a plain `remove_file`: no Trash crate is in the tree, and the
//! window says the delete can't be undone.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::names::NameError;
use super::{read_source, ScriptTemplates, SkipReason, EXTENSION, MAX_BYTES};

#[derive(Debug)]
pub(crate) enum StoreError {
    Name(NameError),
    /// Over [`MAX_BYTES`]; nothing was written.
    TooLarge,
    /// No config directory to write into.
    NoDir,
    Io(io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Name(err) => err.fmt(f),
            StoreError::TooLarge => f.write_str("Templates can't be larger than 256 KiB."),
            StoreError::NoDir => f.write_str("There is no config folder to keep templates in."),
            StoreError::Io(err) => err.fmt(f),
        }
    }
}

impl From<NameError> for StoreError {
    fn from(err: NameError) -> Self {
        StoreError::Name(err)
    }
}

impl From<io::Error> for StoreError {
    fn from(err: io::Error) -> Self {
        StoreError::Io(err)
    }
}

/// What an import did with each picked file.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Imported {
    /// The new templates' names, in the order they were picked.
    pub(crate) added: Vec<String>,
    /// Picked files that aren't `.luau` (the picker can't filter by type).
    pub(crate) ignored: usize,
    /// `.luau` files the loader would refuse, by file name.
    pub(crate) refused: Vec<(String, SkipReason)>,
}

impl ScriptTemplates {
    /// Where `class`'s template `stem` lives (`Default` for the starter).
    pub(crate) fn path(&self, class: &str, stem: &str) -> Option<PathBuf> {
        Some(
            self.dir
                .as_ref()?
                .join(class)
                .join(format!("{stem}.{EXTENSION}")),
        )
    }

    fn path_or_err(&self, class: &str, stem: &str) -> Result<PathBuf, StoreError> {
        self.path(class, stem).ok_or(StoreError::NoDir)
    }

    /// A new template; returns its name as stored (trimmed).
    pub(crate) fn create(
        &self,
        class: &'static str,
        name: &str,
        source: &str,
    ) -> Result<String, StoreError> {
        let name = self.check_name(class, name, None)?;
        self.write(class, &name, source)?;
        Ok(name)
    }

    /// Replaces the text of `class`'s template `stem` — [`super::DEFAULT_STEM`]
    /// writes the starter's replacement. Atomic: a temp file beside it, then
    /// a rename, so a crash or a full disk leaves the last good file.
    pub(crate) fn write(&self, class: &str, stem: &str, source: &str) -> Result<(), StoreError> {
        if source.len() as u64 > MAX_BYTES {
            return Err(StoreError::TooLarge);
        }
        let path = self.path_or_err(class, stem)?;
        let folder = path.parent().expect("a class folder");
        fs::create_dir_all(folder)?;
        // A dot-name without the `.luau` extension: never loaded, even if a
        // crash leaves it behind.
        let temp = folder.join(format!(".{stem}.{EXTENSION}.tmp"));
        let result = (|| {
            let mut file = fs::File::create(&temp)?;
            file.write_all(source.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        Ok(result?)
    }

    /// Renames `class`'s template `old`; returns the new name as stored.
    pub(crate) fn rename(
        &self,
        class: &'static str,
        old: &str,
        new: &str,
    ) -> Result<String, StoreError> {
        let new = self.check_name(class, new, Some(old))?;
        self.move_file(class, old, class, &new)?;
        Ok(new)
    }

    /// Moves `from`'s template `name` into the `to` class folder.
    pub(crate) fn move_to(
        &self,
        from: &str,
        name: &str,
        to: &'static str,
    ) -> Result<(), StoreError> {
        let name = self.check_name(to, name, None)?;
        self.move_file(from, &name, to, &name)
    }

    fn move_file(
        &self,
        from: &str,
        old: &str,
        to: &'static str,
        new: &str,
    ) -> Result<(), StoreError> {
        let source = self.path_or_err(from, old)?;
        let target = self.path_or_err(to, new)?;
        // The list can lag the disk by a poll; never write over a file. A
        // rename that only changes casing finds itself here, and is fine.
        let same_file = from == to && old.eq_ignore_ascii_case(new);
        if target.exists() && !same_file {
            return Err(NameError::Taken {
                class: to,
                name: new.to_owned(),
            }
            .into());
        }
        fs::create_dir_all(target.parent().expect("a class folder"))?;
        Ok(fs::rename(source, target)?)
    }

    /// A copy of `class`'s template `name` called "<name> copy" (or
    /// "copy 2"…); returns the copy's name.
    pub(crate) fn duplicate(&self, class: &'static str, name: &str) -> Result<String, StoreError> {
        let source = fs::read_to_string(self.path_or_err(class, name)?)?;
        let copy = self
            .free_name(class, name, " copy")
            .ok_or(StoreError::Name(NameError::Forbidden))?;
        self.write(class, &copy, &source)?;
        Ok(copy)
    }

    /// Removes `class`'s template `stem`; [`super::DEFAULT_STEM`] puts the built-in
    /// starter back.
    pub(crate) fn delete(&self, class: &str, stem: &str) -> Result<(), StoreError> {
        Ok(fs::remove_file(self.path_or_err(class, stem)?)?)
    }

    /// Removes a file the loader refused, by its file name.
    pub(crate) fn delete_skipped(&self, class: &str, file_name: &str) -> Result<(), StoreError> {
        let dir = self.dir.as_ref().ok_or(StoreError::NoDir)?;
        Ok(fs::remove_file(dir.join(class).join(file_name))?)
    }

    /// Copies each picked `.luau` file into `class`, under its own stem or
    /// "<stem> 2"… when that is taken. Stops at the first write that fails.
    pub(crate) fn import(
        &self,
        class: &'static str,
        picked: &[PathBuf],
    ) -> Result<Imported, StoreError> {
        let mut imported = Imported::default();
        // Names taken by this same import count too, before any reload.
        let mut so_far = self.clone();
        for path in picked {
            if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
                imported.ignored += 1;
                continue;
            }
            let file_name = file_name(path);
            let source = match read_source(path) {
                Ok(Some(source)) => source,
                Ok(None) => {
                    imported.ignored += 1;
                    continue;
                }
                Err(reason) => {
                    imported.refused.push((file_name, reason));
                    continue;
                }
            };
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let Some(name) = so_far.free_name(class, stem, "") else {
                imported.ignored += 1;
                continue;
            };
            self.write(class, &name, &source)?;
            so_far.extras.push(super::Template {
                class,
                name: name.clone(),
                source: String::new(),
            });
            imported.added.push(name);
        }
        Ok(imported)
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
