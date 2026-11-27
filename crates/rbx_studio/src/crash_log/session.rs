//! The session log: stderr goes through a pipe whose reader copies every
//! line, masked, into `<config>/logs/session-<time>-<pid>.log` and the crash
//! log's tail, and passes it on unchanged to wherever stderr went before (a
//! terminal, a caller's pipe), if anywhere. Rust's stderr writes to fd 2 (or
//! looks its handle up on every write, on Windows), so `eprintln!` follows,
//! as does anything a child process inherits it with.
//!
//! A file stops at [`ROLL_AT`] and the log goes on in a new one; the crash
//! log module keeps the newest few. [`finish`] puts stderr back and waits a
//! moment for the reader to catch up, so a caller reading stderr doesn't
//! lose the last lines when the process exits.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::Duration;

use super::{file_name, lock, prune, redact, TAIL, TAIL_LINES};

/// A session log grows to this before the log goes on in a new file.
const ROLL_AT: u64 = 8 << 20;

/// How long [`finish`] waits for the reader to drain the pipe.
const DRAIN_WAIT: Duration = Duration::from_millis(500);

/// What [`finish`] needs to undo [`start`].
struct Running {
    restore: platform::Restore,
    drained: Receiver<()>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

/// Starts copying stderr into the session log. Once, before any thread that
/// writes to stderr; a no-op when there's no logs folder.
pub(crate) fn start() {
    let Some(folder) = super::folder() else {
        return;
    };
    let mut running = lock(&RUNNING);
    if running.is_some() {
        return;
    }
    let Ok(mut log) = Log::open(folder) else {
        return;
    };
    let Ok((reader, writer)) = io::pipe() else {
        return;
    };
    let Some((restore, mut forward)) = platform::redirect(writer) else {
        return;
    };
    let (done, drained) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("rbxstudio-stderr".to_owned())
        .spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut line = Vec::new();
            while reader
                .read_until(b'\n', &mut line)
                .is_ok_and(|read| read > 0)
            {
                // Passed on first: whoever reads stderr shouldn't wait on
                // the file. A reader that went away stops getting it.
                if forward
                    .as_mut()
                    .is_some_and(|out| out.write_all(&line).is_err())
                {
                    forward = None;
                }
                let text = redact(String::from_utf8_lossy(&line).trim_end());
                line.clear();
                log.line(&text);
                let mut tail = lock(&TAIL_LINES);
                if tail.len() == TAIL {
                    tail.pop_front();
                }
                tail.push_back(text);
            }
            let _ = done.send(());
        });
    if spawned.is_ok() {
        *running = Some(Running { restore, drained });
    } else {
        platform::undo(restore);
    }
}

/// Puts stderr back and gives the reader a moment to write what's left.
/// Once the app is done; a child process still holding the pipe keeps the
/// reader going, so this only waits [`DRAIN_WAIT`] for it.
pub(crate) fn finish() {
    let Some(running) = lock(&RUNNING).take() else {
        return;
    };
    platform::undo(running.restore);
    let _ = running.drained.recv_timeout(DRAIN_WAIT);
}

/// The file side: the current session log, rolled over at [`ROLL_AT`].
struct Log {
    folder: PathBuf,
    file: File,
    written: u64,
    /// Files this session started, for a name that's still unique when a
    /// roll lands in the same second as the last one.
    files: u32,
}

impl Log {
    fn open(folder: PathBuf) -> io::Result<Log> {
        fs::create_dir_all(&folder)?;
        let file = File::create(folder.join(file_name("session-")))?;
        Ok(Log {
            folder,
            file,
            written: 0,
            files: 1,
        })
    }

    fn line(&mut self, text: &str) {
        if self.written >= ROLL_AT {
            self.roll();
        }
        if writeln!(self.file, "{text}").is_ok() {
            self.written += text.len() as u64 + 1;
        }
    }

    fn roll(&mut self) {
        let name = file_name("session-").replace(".log", &format!("-{}.log", self.files));
        if let Ok(file) = File::create(self.folder.join(name)) {
            self.file = file;
            self.written = 0;
            self.files += 1;
            prune(&self.folder, "session-");
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::File;
    use std::io::PipeWriter;
    use std::os::fd::{AsRawFd, FromRawFd, RawFd};

    /// fd 2 as it was, or `None` when it was closed.
    pub(super) struct Restore(Option<RawFd>);

    /// Points fd 2 at `writer`; returns how to undo it and where stderr
    /// went before, to pass lines on to.
    pub(super) fn redirect(writer: PipeWriter) -> Option<(Restore, Option<File>)> {
        // SAFETY: plain fd calls on fds this process owns. The saved copy is
        // close-on-exec, so no child inherits it.
        unsafe {
            let saved = libc::fcntl(2, libc::F_DUPFD_CLOEXEC, 3);
            if libc::dup2(writer.as_raw_fd(), 2) < 0 {
                if saved >= 0 {
                    libc::close(saved);
                }
                return None;
            }
            let saved = (saved >= 0).then_some(saved);
            let forward = saved
                .map(|fd| libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3))
                .filter(|fd| *fd >= 0)
                .map(|fd| File::from_raw_fd(fd));
            Some((Restore(saved), forward))
        }
    }

    /// Puts fd 2 back (or onto /dev/null if it was closed), which closes
    /// this process's end of the pipe.
    pub(super) fn undo(restore: Restore) {
        // SAFETY: as above.
        unsafe {
            match restore.0 {
                Some(fd) => {
                    libc::dup2(fd, 2);
                    libc::close(fd);
                }
                None => {
                    let null = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
                    if null >= 0 {
                        libc::dup2(null, 2);
                        libc::close(null);
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::File;
    use std::io::PipeWriter;
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE,
    };

    /// The handles to put back, and the pipe's end to close.
    pub(super) struct Restore {
        stderr: HANDLE,
        /// Stdout went nowhere and was pointed at the pipe too.
        stdout: Option<HANDLE>,
        writer: HANDLE,
    }

    // SAFETY: plain handle values, used on whichever thread calls `finish`.
    unsafe impl Send for Restore {}

    fn usable(handle: HANDLE) -> bool {
        !handle.is_null() && handle != INVALID_HANDLE_VALUE
    }

    /// Points stderr (and stdout, when it goes nowhere: a double-clicked
    /// editor has neither) at `writer`; returns how to undo it and where
    /// stderr went before, to pass lines on to.
    pub(super) fn redirect(writer: PipeWriter) -> Option<(Restore, Option<File>)> {
        // SAFETY: plain Win32 calls. The forward copy is a duplicate, so the
        // old stderr stays open for `undo` to put back.
        unsafe {
            let stderr = GetStdHandle(STD_ERROR_HANDLE);
            let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
            let writer = writer.into_raw_handle() as HANDLE;
            SetStdHandle(STD_ERROR_HANDLE, writer);
            let stdout = (!usable(stdout)).then(|| {
                SetStdHandle(STD_OUTPUT_HANDLE, writer);
                stdout
            });
            let forward = if usable(stderr) {
                duplicate(stderr).map(|copy| File::from_raw_handle(copy))
            } else {
                None
            };
            Some((
                Restore {
                    stderr,
                    stdout,
                    writer,
                },
                forward,
            ))
        }
    }

    /// A handle of our own to the old stderr, so dropping the forward copy
    /// never closes the one `undo` puts back.
    unsafe fn duplicate(handle: HANDLE) -> Option<*mut std::ffi::c_void> {
        use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut copy: HANDLE = std::ptr::null_mut();
        // SAFETY: duplicates within this process into a valid out-pointer.
        let ok = unsafe {
            let me = GetCurrentProcess();
            DuplicateHandle(me, handle, me, &mut copy, 0, 0, DUPLICATE_SAME_ACCESS)
        };
        (ok != 0).then_some(copy)
    }

    pub(super) fn undo(restore: Restore) {
        let put = |which: STD_HANDLE, handle: HANDLE| {
            // SAFETY: a plain Win32 call.
            unsafe { SetStdHandle(which, handle) };
        };
        put(STD_ERROR_HANDLE, restore.stderr);
        if let Some(stdout) = restore.stdout {
            put(STD_OUTPUT_HANDLE, stdout);
        }
        // SAFETY: the pipe end `redirect` took ownership of, closed once.
        unsafe { CloseHandle(restore.writer) };
    }
}

#[cfg(test)]
mod tests {
    use super::{fs, Log, ROLL_AT};

    #[test]
    fn a_full_log_goes_on_in_a_new_file() {
        let folder = std::env::temp_dir().join(format!("rbxn-session-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let mut log = Log::open(folder.clone()).unwrap();
        log.line("first");
        log.written = ROLL_AT;
        log.line("second");
        let mut names: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        names.sort();
        assert_eq!(names.len(), 2, "{names:?}");
        let texts: Vec<_> = names
            .iter()
            .map(|path| fs::read_to_string(path).unwrap())
            .collect();
        assert!(texts.contains(&"first\n".to_owned()), "{texts:?}");
        assert!(texts.contains(&"second\n".to_owned()), "{texts:?}");
        fs::remove_dir_all(&folder).unwrap();
    }
}
