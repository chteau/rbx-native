//! Crash logs: a panic (or, on Windows, a native crash) writes what happened
//! to `<config>/logs/crash-<time>-<pid>.log`, and the next launch offers it
//! in a notice. On Windows, where a double-clicked editor has no console,
//! stderr is also copied into `session-<time>-<pid>.log` as it is written
//! (started from a terminal or a script, stderr is left as it is).
//! Only the newest few of each are kept, and anything that looks like a key,
//! cookie or token is masked before it reaches a file.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gpui_kit::component::h_flex;
use gpui_kit::*;

use crate::launcher::ui::icon;
use crate::tokens;

/// Logs of each kind kept.
const KEPT: usize = 10;

/// Lines of stderr a crash log ends with.
#[cfg(windows)]
const TAIL: usize = 200;

/// Where a crash gets reported: GitHub issues, with the log attached.
pub(crate) const ISSUES_URL: &str = "https://github.com/chteau/rbx-native/issues/new";
/// The community Discord, for help and feedback.
pub(crate) const DISCORD_URL: &str = "https://discord.gg/5KSmhcqHt5";

/// The newest stderr lines (Windows only), for a crash log to end with.
static TAIL_LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Crash logs this session wrote. A panic something caught (a renderer that
/// failed to build, say) still writes one; quitting normally renames them to
/// `panic-…`, so only a session that died leaves a `crash-…` behind.
static WRITTEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The crash log the previous session left, until the notice takes it.
static PREVIOUS: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Records the last session's crash for [`notice`], prunes old logs and
/// installs the hooks. Before any other thread starts.
pub(crate) fn init() {
    let Some(folder) = folder() else {
        return;
    };
    let _ = fs::create_dir_all(&folder);
    *lock(&PREVIOUS) = unseen_crash(&folder);
    for prefix in ["crash-", "panic-", "session-"] {
        prune(&folder, prefix);
    }
    #[cfg(windows)]
    tee_stderr(&folder);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(no message)");
        let location = info
            .location()
            .map_or_else(String::new, |at| format!("\nat {at}"));
        let thread = std::thread::current();
        let what = format!(
            "panic on thread '{}': {message}{location}",
            thread.name().unwrap_or("unnamed")
        );
        write_crash(&what, true);
    }));
    #[cfg(windows)]
    native::install();
}

/// Called once the app quits normally: see [`WRITTEN`].
pub(crate) fn quit_cleanly() {
    for path in lock(&WRITTEN).drain(..) {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            let _ = fs::rename(
                &path,
                path.with_file_name(name.replacen("crash-", "panic-", 1)),
            );
        }
    }
}

fn folder() -> Option<PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("logs"))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn file_name(prefix: &str) -> String {
    format!("{prefix}{}-{}.log", stamp(), std::process::id())
}

/// The logs in `folder` named `prefix…`, oldest first (names start with
/// their time).
fn logs(folder: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(".log"))
        })
        .collect();
    found.sort();
    found
}

fn prune(folder: &Path, prefix: &str) {
    let found = logs(folder, prefix);
    for old in &found[..found.len().saturating_sub(KEPT)] {
        let _ = fs::remove_file(old);
    }
}

/// The newest crash log no launch has offered yet, marked offered.
fn unseen_crash(folder: &Path) -> Option<PathBuf> {
    let newest = logs(folder, "crash-").pop()?;
    let name = newest.file_name()?.to_str()?.to_owned();
    let seen = folder.join("last-seen-crash");
    if fs::read_to_string(&seen).is_ok_and(|last| last.trim() >= name.as_str()) {
        return None;
    }
    let _ = fs::write(&seen, &name);
    Some(newest)
}

/// Writes a crash log about `what`, with a backtrace when `backtrace`.
fn write_crash(what: &str, backtrace: bool) {
    let Some(folder) = folder() else {
        return;
    };
    let backtrace = if backtrace {
        std::backtrace::Backtrace::force_capture().to_string()
    } else {
        "(not taken)".to_owned()
    };
    // `try_lock`: the thread that crashed may be the one holding it.
    let tail = TAIL_LINES
        .try_lock()
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    let report = format!(
        "RbxNative {} crashed at {}\n{}\nGPU: {}\n\n{what}\n\nBacktrace:\n{backtrace}\n\
         Last output:\n{tail}\n",
        env!("CARGO_PKG_VERSION"),
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z"),
        platform(),
        rbx_viewer::chosen_adapter().unwrap_or_else(|| "none opened yet".to_owned()),
    );
    let path = folder.join(file_name("crash-"));
    if fs::write(&path, redact(&report)).is_ok() {
        if let Ok(mut written) = WRITTEN.try_lock() {
            written.push(path);
        }
    }
}

/// The OS and the build's architecture, and on Windows the machine's own
/// when the build runs emulated (an x64 build on Windows on ARM).
fn platform() -> String {
    let build = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    #[cfg(windows)]
    if let Some(machine) = native::machine() {
        return format!("{build} build on a {machine} machine");
    }
    build
}

/// Masks the value after anything that names a secret: a `.ROBLOSECURITY`
/// cookie, an API key header or field, a bearer token.
fn redact(text: &str) -> String {
    const MARKERS: [&str; 9] = [
        "_|WARNING:",
        ".ROBLOSECURITY=",
        "x-api-key:",
        "x-api-key=",
        "Bearer ",
        "api_key=",
        "apikey=",
        "token=",
        "\"token\":",
    ];
    let mut out = text.to_owned();
    for marker in MARKERS {
        let mut from = 0;
        while let Some(found) = out[from..]
            .to_ascii_lowercase()
            .find(&marker.to_ascii_lowercase())
        {
            let start = from + found + marker.len();
            let value_start =
                start + out[start..].len() - out[start..].trim_start_matches([' ', '"']).len();
            let value_end = out[value_start..]
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ';' | ',' | '&'))
                .map_or(out.len(), |end| value_start + end);
            if value_end > value_start {
                out.replace_range(value_start..value_end, "[redacted]");
            }
            from = value_start + "[redacted]".len().min(out.len() - value_start);
        }
    }
    out
}

/// When stderr goes nowhere (a double-clicked editor has no console), points
/// it, and stdout if it goes nowhere either, at a pipe whose reader copies
/// every line, masked, into the session log and [`TAIL_LINES`]. Rust's stderr
/// looks its handle up on every write, so `eprintln!` follows. A stderr that
/// already goes somewhere (a terminal, a caller's pipe) is left alone: a
/// copying thread could still be behind when the process exits, and what
/// the caller reads would lose its last lines.
#[cfg(windows)]
fn tee_stderr(folder: &Path) {
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    let usable = |handle: HANDLE| !handle.is_null() && handle != INVALID_HANDLE_VALUE;
    // SAFETY: a plain Win32 query.
    if usable(unsafe { GetStdHandle(STD_ERROR_HANDLE) }) {
        return;
    }
    let Ok(mut file) = fs::File::create(folder.join(file_name("session-"))) else {
        return;
    };
    let Ok((reader, writer)) = std::io::pipe() else {
        return;
    };
    // SAFETY: plain Win32 calls; the pipe's writer is leaked to stay valid
    // for the life of the process.
    unsafe {
        let writer = writer.into_raw_handle();
        SetStdHandle(STD_ERROR_HANDLE, writer);
        if !usable(GetStdHandle(STD_OUTPUT_HANDLE)) {
            SetStdHandle(STD_OUTPUT_HANDLE, writer);
        }
    }
    let _ = std::thread::Builder::new()
        .name("rbxstudio-stderr".to_owned())
        .spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut line = Vec::new();
            while reader
                .read_until(b'\n', &mut line)
                .is_ok_and(|read| read > 0)
            {
                let text = redact(String::from_utf8_lossy(&line).trim_end());
                line.clear();
                let _ = writeln!(file, "{text}");
                let mut tail = lock(&TAIL_LINES);
                if tail.len() == TAIL {
                    tail.pop_front();
                }
                tail.push_back(text);
            }
        });
}

#[cfg(windows)]
mod native {
    use windows_sys::Win32::Foundation::{EXCEPTION_STACK_OVERFLOW, HANDLE};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SetUnhandledExceptionFilter, EXCEPTION_POINTERS,
    };
    use windows_sys::Win32::System::SystemInformation::{
        IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};

    /// A note for an access violation or the like that no handler took
    /// (a driver's, say), before Windows ends the process. A `__fastfail`
    /// (what a Rust abort is) bypasses this; the panic hook has already
    /// written that one.
    pub(super) fn install() {
        // SAFETY: installs a filter with the signature Windows expects.
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
        }
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows hands the filter a valid record, checked for null.
        let (code, address) = unsafe {
            let record = info.as_ref().map(|info| info.ExceptionRecord);
            match record.and_then(|record| record.as_ref()) {
                Some(record) => (record.ExceptionCode, record.ExceptionAddress),
                None => (0, std::ptr::null_mut()),
            }
        };
        // A blown stack has no room to walk itself.
        super::write_crash(
            &format!("native exception 0x{code:08x} at {address:?}"),
            code != EXCEPTION_STACK_OVERFLOW,
        );
        0 // EXCEPTION_CONTINUE_SEARCH: Windows ends the process as before.
    }

    /// The machine's own architecture when it differs from the build's.
    pub(super) fn machine() -> Option<&'static str> {
        let mut process: IMAGE_FILE_MACHINE = 0;
        let mut native: IMAGE_FILE_MACHINE = 0;
        // SAFETY: the pseudo-handle needs no closing and both out-pointers
        // are valid for the call.
        let current: HANDLE = unsafe { GetCurrentProcess() };
        if unsafe { IsWow64Process2(current, &mut process, &mut native) } == 0 {
            return None;
        }
        match native {
            IMAGE_FILE_MACHINE_ARM64 if cfg!(not(target_arch = "aarch64")) => Some("ARM64"),
            IMAGE_FILE_MACHINE_AMD64 if cfg!(not(target_arch = "x86_64")) => Some("x64"),
            _ => None,
        }
    }
}

/// "RbxNative crashed last time": offered once, in the bottom-right corner of
/// whichever window opens first, until dismissed.
pub(crate) struct CrashNotice {
    log: Option<PathBuf>,
}

struct Shared(Entity<CrashNotice>);

impl Global for Shared {}

/// The notice, for a window's root to add; `None` when the last session
/// didn't crash or it was dismissed.
pub(crate) fn notice(cx: &mut App) -> Option<AnyView> {
    if !cx.has_global::<Shared>() {
        let log = lock(&PREVIOUS).take()?;
        let notice = cx.new(|_| CrashNotice { log: Some(log) });
        cx.set_global(Shared(notice));
    }
    let notice = cx.global::<Shared>().0.clone();
    notice.read(cx).log.is_some().then(|| notice.into())
}

impl Render for CrashNotice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(log) = self.log.clone() else {
            return div().into_any_element();
        };
        let copied = log.clone();
        let action = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px(px(6.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_color(tokens::check_on())
                .hover(|this| this.bg(tokens::hover()))
                .child(label)
        };
        div()
            .absolute()
            // Above the update toast, which sits clear of the Command Bar.
            .bottom(px(100.))
            .right(px(16.))
            .child(
                h_flex()
                    .id("crash-notice")
                    .debug_selector(|| "crash-notice".into())
                    .gap(px(10.))
                    .py(px(10.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .items_center()
                    .rounded(tokens::radius_container())
                    .bg(tokens::field_select())
                    .shadow(tokens::elevation())
                    .font_family(tokens::font_ui())
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .text_color(tokens::text())
                    .child(
                        div()
                            .text_color(tokens::warning())
                            .child(icon("triangle-alert", 16.)),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("RbxNative crashed last time"),
                    )
                    .child(
                        action("crash-notice-open", "Open log")
                            .on_click(move |_, _, cx| cx.open_with_system(&log)),
                    )
                    .child(
                        action("crash-notice-copy", "Copy").on_click(move |_, _, cx| {
                            if let Ok(text) = fs::read_to_string(&copied) {
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                            }
                        }),
                    )
                    .child(
                        action("crash-notice-report", "Report on GitHub")
                            .on_click(|_, _, cx| cx.open_url(ISSUES_URL)),
                    )
                    .child(
                        action("crash-notice-discord", "Discord")
                            .on_click(|_, _, cx| cx.open_url(DISCORD_URL)),
                    )
                    .child(
                        div()
                            .id("crash-notice-close")
                            .p(px(4.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| this.bg(tokens::hover()).text_color(tokens::text()))
                            .child(icon("x", 13.))
                            .tooltip(|window, cx| {
                                crate::shell::tooltip::text("Dismiss", window, cx)
                            })
                            .on_click(cx.listener(|notice, _, _, cx| {
                                notice.log = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{fs, logs, prune, redact, unseen_crash, KEPT};

    #[test]
    fn secrets_are_masked() {
        let line = "GET x-api-key: abc123 cookie .ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE|_xyz; \
                    Authorization: Bearer eyJhb.c url?token=q1w2&x=1";
        let masked = redact(line);
        for secret in ["abc123", "DO-NOT-SHARE", "_xyz", "eyJhb", "q1w2"] {
            assert!(!masked.contains(secret), "{secret} left in {masked}");
        }
        assert!(masked.contains("&x=1"));
        assert_eq!(redact("nothing secret here"), "nothing secret here");
    }

    #[test]
    fn only_a_crash_no_launch_offered_is_offered() {
        let folder = std::env::temp_dir().join(format!("rbx-crash-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        assert_eq!(unseen_crash(&folder), None);
        fs::write(folder.join("crash-20261101-120000-1.log"), "old").unwrap();
        fs::write(folder.join("crash-20261102-120000-1.log"), "new").unwrap();
        fs::write(folder.join("panic-20261103-120000-1.log"), "caught").unwrap();
        assert_eq!(
            unseen_crash(&folder),
            Some(folder.join("crash-20261102-120000-1.log"))
        );
        assert_eq!(unseen_crash(&folder), None);
        for n in 0..KEPT + 3 {
            fs::write(folder.join(format!("session-2026110{n:02}.log")), "").unwrap();
        }
        prune(&folder, "session-");
        assert_eq!(logs(&folder, "session-").len(), KEPT);
        fs::remove_dir_all(&folder).unwrap();
    }
}
