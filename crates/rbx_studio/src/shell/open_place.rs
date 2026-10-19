//! File › New and File › Open…: the editor is one place per window, so both
//! leave this one (asking about unsaved changes first, see `close_place`)
//! for a new window on the other place, as Home's New and Open do.
//!
//! `RBX_STUDIO_OPEN=<path>` skips the file chooser and opens `<path>` — a
//! debugging aid for driving Open where no chooser portal may answer.

use std::path::{Path, PathBuf};

use gpui_kit::{Context, PathPromptOptions};

use crate::home::{self, Template};

use super::Shell;

pub(in crate::shell) const OPEN_VARIABLE: &str = "RBX_STUDIO_OPEN";

/// Whether `path` names a file the editor reads: a place or a model, binary
/// or XML. GPUI's file chooser has no extension filter, so the pick is
/// checked here.
pub(super) fn is_place_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["rbxl", "rbxlx", "rbxm", "rbxmx"]
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// A fresh Baseplate on disk, in the places folder.
pub(super) fn create_baseplate() -> Result<PathBuf, String> {
    home::new_place_path(Template::Baseplate)
        .ok_or_else(|| "no config directory to create the place in".to_string())
        .and_then(|path| Template::Baseplate.create(&path).map(|()| path))
}

impl Shell {
    /// File › Open…: asks for a file, then opens it as the place.
    pub(crate) fn open_place_dialog(&mut self, cx: &mut Context<Self>) {
        if let Ok(path) = std::env::var(OPEN_VARIABLE) {
            self.open_picked(PathBuf::from(path), cx);
            return;
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(picked) = picked.await else {
                return;
            };
            let _ = this.update(cx, |shell, cx| match picked {
                Ok(Some(paths)) => {
                    if let Some(path) = paths.into_iter().next() {
                        shell.open_picked(path, cx);
                    }
                }
                Ok(None) => {}
                // No file chooser portal to ask: say so rather than a menu
                // row that silently does nothing.
                Err(err) => shell.warn(format!("Open: no file dialog: {err:#}"), cx),
            });
        })
        .detach();
    }

    fn open_picked(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if is_place_file(&path) {
            self.request_open(path, cx);
        } else {
            self.warn(
                format!(
                    "Open: {} is not a .rbxl, .rbxlx, .rbxm or .rbxmx file",
                    path.display()
                ),
                cx,
            );
        }
    }

    pub(super) fn warn(&mut self, message: String, cx: &mut Context<Self>) {
        self.output.push_warning(&message);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::is_place_file;

    #[test]
    fn places_and_models_open_in_either_case() {
        for name in ["a.rbxl", "a.RBXLX", "dir/b.rbxm", "c.RbxMx"] {
            assert!(is_place_file(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn other_files_do_not() {
        for name in ["a.txt", "a.rbxl.bak", "rbxl", "a.lua", "a."] {
            assert!(!is_place_file(Path::new(name)), "{name}");
        }
    }
}
