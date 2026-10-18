//! Unelevated side of the Windows shaper: launches the elevated helper
//! (this same exe with `HELPER_FLAG`) and talks to it over a private named
//! pipe, strictly request -> reply.
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_CANCELLED, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use super::{error_text, HELPER_FLAG, PIPE_PREFIX};
use crate::netsim::{lines, NetConditions, ShaperError};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

/// Handle to the elevated WinDivert helper.
pub struct Shaper {
    pipe: File,
    reader: BufReader<File>,
    // Stored as an integer so the struct stays `Send`.
    process: usize,
}

fn process_running(process: usize) -> bool {
    // SAFETY: `process` is a live process handle owned by the caller.
    unsafe { WaitForSingleObject(process as HANDLE, 0) == WAIT_TIMEOUT }
}

fn err(text: impl Into<String>) -> ShaperError {
    ShaperError(text.into())
}

impl Shaper {
    /// Launches the elevated helper and waits until it reports `ready`.
    pub fn start() -> Result<Shaper, ShaperError> {
        let exe = std::env::current_exe().map_err(|e| err(format!("cannot find own exe: {e}")))?;
        let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        if !dir.join("WinDivert.dll").is_file() || !dir.join("WinDivert64.sys").is_file() {
            return Err(err(error_text(2)));
        }

        let mut rnd = [0u8; 8];
        getrandom::fill(&mut rnd).map_err(|e| err(format!("no random data: {e}")))?;
        let hex: String = rnd.iter().map(|b| format!("{b:02x}")).collect();
        // SAFETY: plain query with no arguments.
        let our_pid = unsafe { GetCurrentProcessId() };
        let name = format!("{PIPE_PREFIX}{our_pid}-{hex}");
        let wname = wide(OsStr::new(&name));

        // SAFETY: `wname` is NUL-terminated and outlives the call.
        let pipe = unsafe {
            CreateNamedPipeW(
                wname.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                0,
                std::ptr::null(),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return Err(err("could not create the helper pipe"));
        }

        let verb = wide(OsStr::new("runas"));
        let file = wide(exe.as_os_str());
        let params = wide(OsStr::new(&format!("{HELPER_FLAG} {name} {our_pid}")));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: params.as_ptr(),
            nShow: SW_HIDE,
            ..Default::default()
        };
        // SAFETY: `info` and the wide strings it points to are alive for the call.
        let launched = unsafe { ShellExecuteExW(&mut info) };
        if launched == 0 || info.hProcess.is_null() {
            // SAFETY: reads the calling thread's last error; `pipe` is ours.
            let code = unsafe { GetLastError() };
            unsafe { CloseHandle(pipe) };
            return Err(err(if code == ERROR_CANCELLED {
                "the administrator prompt was declined".to_string()
            } else {
                format!("could not start the elevated helper (error {code})")
            }));
        }
        let process = info.hProcess as usize;

        let pipe_addr = pipe as usize;
        let (tx, rx) = mpsc::channel::<Result<(), u32>>();
        let waiter = std::thread::spawn(move || {
            let h = pipe_addr as HANDLE;
            // SAFETY: the pipe handle stays open until this thread has finished.
            let ok = unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
            let result = if ok != 0 {
                Ok(())
            } else {
                let code = unsafe { GetLastError() };
                if code == ERROR_PIPE_CONNECTED {
                    Ok(())
                } else {
                    Err(code)
                }
            };
            let _ = tx.send(result);
        });

        let begin = Instant::now();
        let outcome: Result<(), String> = loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(())) => break Ok(()),
                Ok(Err(code)) => break Err(format!("helper pipe failed (error {code})")),
                Err(RecvTimeoutError::Disconnected) => break Err("helper pipe failed".into()),
                Err(RecvTimeoutError::Timeout) => {
                    if !process_running(process) {
                        break Err("the helper exited before connecting".into());
                    }
                    if begin.elapsed() >= CONNECT_TIMEOUT {
                        break Err("the helper did not connect in time".into());
                    }
                }
            }
        };

        if let Err(text) = outcome {
            // A client open releases a thread still blocked in ConnectNamedPipe.
            let _ = OpenOptions::new().read(true).write(true).open(&name);
            let _ = waiter.join();
            // SAFETY: both handles are ours and no thread uses them any more.
            unsafe {
                CloseHandle(pipe);
                CloseHandle(process as HANDLE);
            }
            return Err(err(text));
        }
        let _ = waiter.join();

        // SAFETY: ownership of the pipe handle moves into the File.
        let pipe_file = unsafe { File::from_raw_handle(pipe as _) };
        let mut shaper = match pipe_file.try_clone() {
            Ok(clone) => Shaper {
                pipe: pipe_file,
                reader: BufReader::new(clone),
                process,
            },
            Err(e) => {
                unsafe { CloseHandle(process as HANDLE) };
                return Err(err(format!("helper pipe failed: {e}")));
            }
        };

        let first = shaper.read_line()?;
        if first == "ready" {
            return Ok(shaper);
        }
        let text = match first.strip_prefix("error ") {
            Some(rest) => rest.split_once(' ').map_or(rest, |(_, t)| t).to_string(),
            None => format!("unexpected helper reply: {first}"),
        };
        Err(err(text))
    }

    fn read_line(&mut self) -> Result<String, ShaperError> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(err("the helper closed the connection")),
            Ok(_) => Ok(line.trim_end_matches(['\r', '\n']).to_string()),
            Err(e) => Err(err(format!("helper pipe failed: {e}"))),
        }
    }

    /// Sends both directions and waits for the helper to confirm each.
    pub fn apply(&mut self, conditions: &NetConditions) -> Result<(), ShaperError> {
        self.pipe
            .write_all(lines::conditions_lines(conditions).as_bytes())
            .and_then(|()| self.pipe.flush())
            .map_err(|e| err(format!("helper pipe failed: {e}")))?;
        for expected in ["ok out", "ok in"] {
            let reply = self.read_line()?;
            if reply != expected {
                return Err(err(reply));
            }
        }
        Ok(())
    }

    /// Whether the elevated helper process is still running.
    pub fn alive(&mut self) -> bool {
        process_running(self.process)
    }

    /// No standing caveat on Windows.
    pub fn notice(&self) -> Option<&'static str> {
        None
    }
}

impl Drop for Shaper {
    fn drop(&mut self) {
        // The elevated helper cannot be killed from here; it exits on `stop` or EOF.
        let _ = self.pipe.write_all(b"stop\n");
        // SAFETY: the process handle is ours and closed exactly once.
        unsafe { CloseHandle(self.process as HANDLE) };
    }
}
