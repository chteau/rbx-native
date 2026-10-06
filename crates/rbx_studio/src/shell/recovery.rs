//! Auto-Recovery's shell side: noticing changes, the timer, and writing the
//! copy off the UI thread (see `crate::recovery` for what and where).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use gpui_kit::*;

use super::Shell;
use crate::recovery;
use crate::save;
use crate::script_editor::source;

/// How often the timer looks. Well under the shortest interval, so a copy
/// lands within a few seconds of falling due.
const CHECK_EVERY: Duration = Duration::from_secs(15);

/// A temp file this old in the recovery folder belongs to a process that
/// died mid-write; a live write takes seconds.
const STALE_TEMP: Duration = Duration::from_secs(60 * 60);

pub(super) struct Recovery {
    enabled: bool,
    minutes: u32,
    /// The place's canonical path: the copy's name hashes it, so a relative
    /// and an absolute launch of one place share a copy.
    place: PathBuf,
    /// Where this session's copies go, claimed as the place opens. `None`
    /// means no copies this session: no config folder, the place is itself
    /// inside the recovery folder, the copy's lock could not be taken, or an
    /// earlier session's copy could not be moved aside.
    copy: Option<PathBuf>,
    /// Held for the session so no other running editor touches `copy`;
    /// the OS releases it when this process exits or dies.
    _lock: Option<std::fs::File>,
    /// The place changed since the last copy (or since it was saved).
    changed: bool,
    /// The place changed since it was opened or last saved; unlike
    /// `changed`, a recovery copy does not clear it. File › Close Place asks
    /// before discarding it.
    unsaved: bool,
    /// When the last copy was taken; the place opening counts as one, so
    /// the first copy waits a full interval.
    last: Instant,
    /// A copy is being written, so the next check does not start another.
    writing: bool,
    /// This session wrote the copy now in the folder, so a save may delete
    /// it. Nothing else is ever deleted.
    wrote: bool,
    /// Bumped by every save, so a copy that finishes writing after one
    /// knows it is stale and deletes itself.
    saves: u64,
}

impl Recovery {
    pub(super) fn new(enabled: bool, minutes: u32, place: &Path) -> Self {
        Recovery {
            enabled,
            minutes: recovery::clamp_minutes(u64::from(minutes)),
            place: std::fs::canonicalize(place).unwrap_or_else(|_| place.to_path_buf()),
            copy: None,
            _lock: None,
            changed: false,
            unsaved: false,
            last: Instant::now(),
            writing: false,
            wrote: false,
            saves: 0,
        }
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn minutes(&self) -> u32 {
        self.minutes
    }

    /// Called from `Shell::push_history_snapshot`, which every recorded
    /// edit (script runs and `Source` writes included) passes through, and
    /// from `Shell::reflect_changes`, which undo, redo and a drag's later
    /// steps do.
    pub(super) fn changed(&mut self) {
        self.changed = true;
        self.unsaved = true;
    }

    pub(super) fn unsaved(&self) -> bool {
        self.unsaved
    }
}

impl Shell {
    pub(super) fn watch_recovery(&mut self, cx: &mut Context<Self>) {
        self.open_recovery();
        cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(CHECK_EVERY).await;
            if shell
                .update(cx, |shell, cx| shell.write_recovery(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    /// Once, as the place opens (the editor holds one place for its whole
    /// life): sweeps temp files dead writers left, refuses to copy a place
    /// that is itself in the recovery folder, claims this session's copy
    /// (its own, if another editor already has this place open), and moves
    /// aside every copy of this place an earlier session left, in this
    /// session's slot or any other no running editor holds, so none is ever
    /// overwritten or deleted, nor left unannounced.
    fn open_recovery(&mut self) {
        let Some(folder) = recovery::folder() else {
            return;
        };
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age > STALE_TEMP);
            if old && recovery::is_temp(&entry.file_name().to_string_lossy()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }

        let state = &mut self.recovery;
        if std::fs::canonicalize(&folder).is_ok_and(|folder| state.place.starts_with(folder)) {
            self.output.push_warning(
                "This place is in the Auto-Recovery folder, so no recovery copies are written \
                 of it, and Ctrl+S saves it there. To keep working on it, copy it out of that \
                 folder and open it from there.",
            );
            return;
        }
        let copy = match recovery::claim(&folder, &state.place) {
            Ok((copy, lock)) => {
                state._lock = Some(lock);
                copy
            }
            Err(err) => {
                self.output.push_warning(&format!(
                    "Auto-Recovery could not lock its copy of this place ({err}), so it \
                     writes no copies this session rather than risk another editor's."
                ));
                return;
            }
        };
        if !copy.exists() {
            state.copy = Some(copy.clone());
        }
        // This session's own slot first: if its old copy cannot be moved,
        // writing would overwrite it, so this session writes none.
        let mut left = vec![(copy.clone(), None)];
        left.extend(
            recovery::orphans(&folder, &state.place, &copy)
                .into_iter()
                .map(|(orphan, lock)| (orphan, Some(lock))),
        );
        for (old, _lock) in left.into_iter().filter(|(old, _)| old.exists()) {
            match keep_aside(&old) {
                Ok(kept) => {
                    if old == copy {
                        self.recovery.copy = Some(copy.clone());
                    }
                    self.output.push_warning(&format!(
                        "Auto-Recovery found a copy of this place from an earlier session and \
                         kept it as {} (Studio Settings › Files & recovery › Open auto-saves).",
                        kept.display()
                    ));
                }
                Err(err) if old == copy => self.output.push_warning(&format!(
                    "Auto-Recovery found a copy of this place from an earlier session at {} \
                     but could not move it aside ({err}), so it writes no copies this \
                     session, leaving that one as it is.",
                    old.display()
                )),
                Err(err) => self.output.push_warning(&format!(
                    "Auto-Recovery found a copy of this place from an earlier session at {} \
                     but could not move it aside ({err}); it is left as it is.",
                    old.display()
                )),
            }
        }
    }

    /// Writes a copy if one is due. The DOM is cloned here, on the UI thread
    /// (a large place's clone is a short hitch), and serialized and written
    /// on a background one. `save::save` writes through a temp file, so a
    /// copy is never left half-written.
    fn write_recovery(&mut self, cx: &mut Context<Self>) {
        let state = &self.recovery;
        let Some(path) = state.copy.clone() else {
            return;
        };
        if state.writing
            || !recovery::due(
                state.enabled,
                state.changed,
                state.last.elapsed(),
                state.minutes,
            )
        {
            return;
        }
        let mut dom = self.dom.clone();
        // Typing still on its debounce goes into the copy, but not through
        // `flush_script_edits`, which would cut an undo step mid-word.
        for (reference, open) in &self.scripts.open {
            if open.pending {
                source::write(&mut dom, *reference, &open.state.read(cx).value());
            }
        }
        let format = self.format;
        let saves = self.recovery.saves;
        self.recovery.changed = false;
        self.recovery.last = Instant::now();
        self.recovery.writing = true;
        self.recovery.wrote = true;

        let written = path.clone();
        let write = cx.background_executor().spawn(async move {
            // The folder was made as the place opened; this only matters if
            // it has been deleted since.
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).map_err(|err| err.to_string())?;
            }
            save::save(&dom, format, &path)
        });
        cx.spawn(async move |shell, cx| {
            let result = write.await;
            let _ = shell.update(cx, |shell, cx| {
                shell.recovery.writing = false;
                if shell.recovery.saves != saves {
                    // A save landed while this was written: nothing is left
                    // to recover, and this copy would outlive it.
                    let _ = std::fs::remove_file(&written);
                } else if let Err(message) = result {
                    // Still unsaved, so the next check tries again.
                    shell.recovery.changed = true;
                    shell
                        .output
                        .push_warning(&format!("Auto-Recovery could not write a copy: {message}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// After a successful Ctrl+S: nothing is left to recover, so the copy
    /// this session wrote goes. One still being written deletes itself when
    /// it lands (see `write_recovery`).
    pub(super) fn saved(&mut self) {
        let state = &mut self.recovery;
        state.changed = false;
        state.unsaved = false;
        state.last = Instant::now();
        state.saves += 1;
        if std::mem::take(&mut state.wrote) {
            if let Some(copy) = &state.copy {
                let _ = std::fs::remove_file(copy);
            }
        }
    }

    pub(super) fn set_auto_recovery(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.recovery.enabled != enabled {
            self.recovery.enabled = enabled;
            self.save_settings();
            cx.notify();
        }
    }

    pub(super) fn set_recovery_minutes(&mut self, minutes: u32, cx: &mut Context<Self>) {
        let minutes = recovery::clamp_minutes(u64::from(minutes));
        if self.recovery.minutes != minutes {
            self.recovery.minutes = minutes;
            self.save_settings();
            cx.notify();
        }
    }
}

/// Moves a copy an earlier session left to its timestamped name (when it
/// was written, numbered if that is taken), returning where it went.
fn keep_aside(copy: &Path) -> std::io::Result<PathBuf> {
    let written = std::fs::metadata(copy)?
        .modified()
        .unwrap_or_else(|_| SystemTime::now());
    let stamp = chrono::DateTime::<chrono::Local>::from(written)
        .format("%Y-%m-%d %H-%M-%S")
        .to_string();
    let mut kept = recovery::kept_path(copy, &stamp);
    let mut n = 1;
    while kept.exists() {
        n += 1;
        kept = recovery::kept_path(copy, &format!("{stamp} ({n})"));
    }
    std::fs::rename(copy, &kept)?;
    Ok(kept)
}
