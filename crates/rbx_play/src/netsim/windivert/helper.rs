//! The elevated Windows helper: diverts the Roblox client's UDP packets with
//! WinDivert, holds them in a user-space delay line and reinjects them.
//!
//! It talks to the editor over a named pipe with the integer line protocol
//! from `netsim::lines`, and exits as soon as the editor process goes away.
//!
//! It outlives a Play on purpose: starting it again would mean a second
//! administrator prompt on every shaped Play. What it lets go of at Stop
//! (`idle`) is the driver: every WinDivert handle closes and the service
//! is stopped, then the next `out`/`in` loads it again.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Media::timeBeginPeriod;
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
};

use super::ffi::{Address, Api, Handle};
use super::udp;
use crate::netsim::delay::{DelayLine, Rng};
use crate::netsim::lines::{self, Line};
use crate::netsim::NetConditions;

/// 40 header bytes plus the largest IP payload WinDivert can hand back.
const PACKET_BUF: usize = 40 + 0xFFFF;
const PORT_POLL: Duration = Duration::from_millis(500);

/// Each packet keeps the handle that received it: WinDivert only reinjects
/// correctly through the same handle with the packet's original address.
type Item = (Vec<u8>, Address, Arc<Handle>);

struct Lines {
    out: DelayLine<Item>,
    inbound: DelayLine<Item>,
    rng: Rng,
}

struct Shared {
    /// False between a Play's Stop (`idle`) and the next conditions.
    active: AtomicBool,
    conditions: Mutex<NetConditions>,
    lines: Mutex<Lines>,
    wake: Condvar,
}

pub fn run_helper(args: &[String]) -> i32 {
    let code = serve(args);
    driver::release();
    code
}

fn serve(args: &[String]) -> i32 {
    let [pipe, pid] = args else { return 2 };
    if !pipe.starts_with(super::PIPE_PREFIX)
        || pid.is_empty()
        || !pid.bytes().all(|b| b.is_ascii_digit())
    {
        return 2;
    }
    let Ok(parent) = pid.parse::<u32>() else {
        return 2;
    };
    let Ok(mut writer) = OpenOptions::new().read(true).write(true).open(pipe) else {
        return 1;
    };
    let Ok(reader) = writer.try_clone() else {
        return 1;
    };
    let mut reader = BufReader::new(reader);

    watch_parent(parent);
    // SAFETY: no preconditions; raises sleep granularity so 1 ms delays hold.
    unsafe { timeBeginPeriod(1) };

    let api = match load_api() {
        Ok(api) => Arc::new(api),
        Err(code) => return report_error(&mut writer, code),
    };
    driver::note_state();
    // A filter that matches nothing still loads the driver, so admin and
    // driver problems surface now rather than on the first port change.
    match Handle::open(&api, "false", 0) {
        Ok(probe) => drop(probe),
        Err(code) => return report_error(&mut writer, code),
    }
    if writer.write_all(b"ready\n").is_err() {
        return 1;
    }

    let shared = Arc::new(Shared {
        active: AtomicBool::new(true),
        conditions: Mutex::new(NetConditions::default()),
        lines: Mutex::new(Lines {
            out: DelayLine::new(),
            inbound: DelayLine::new(),
            rng: Rng::from_entropy(),
        }),
        wake: Condvar::new(),
    });
    {
        let s = Arc::clone(&shared);
        std::thread::spawn(move || sender(&s));
    }
    {
        let s = Arc::clone(&shared);
        std::thread::spawn(move || port_watcher(&api, &s));
    }

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return 0,
            Ok(_) => {}
        }
        let reply: &[u8] = match lines::parse(&line) {
            Some(Line::Out(w)) => {
                lock(&shared.conditions).outbound = w.to_way();
                shared.active.store(true, Ordering::Relaxed);
                b"ok out\n"
            }
            Some(Line::In(w)) => {
                lock(&shared.conditions).inbound = w.to_way();
                shared.active.store(true, Ordering::Relaxed);
                b"ok in\n"
            }
            Some(Line::Idle) => {
                shared.active.store(false, Ordering::Relaxed);
                b"ok idle\n"
            }
            Some(Line::Stop) => return 0,
            _ => b"error out malformed\n",
        };
        if writer.write_all(reply).is_err() {
            return 0;
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn report_error(writer: &mut std::fs::File, code: u32) -> i32 {
    let msg = format!("error {code} {}\n", super::error_text(code));
    let _ = writer.write_all(msg.as_bytes());
    1
}

/// Loaded by absolute path next to our own executable, never via the search
/// path, because this process runs elevated.
fn load_api() -> Result<Api, u32> {
    let dir: PathBuf = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .ok_or(2u32)?;
    Api::load(&dir.join("WinDivert.dll"))
}

/// Only the editor is opened, with SYNCHRONIZE alone: opening the Roblox
/// client would trip its anti-tamper, and the editor's exit is what should
/// end shaping anyway.
fn watch_parent(pid: u32) {
    // SAFETY: plain call; a null result is handled.
    let h = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if h.is_null() {
        std::process::exit(0);
    }
    let raw = h as usize;
    std::thread::spawn(move || {
        // SAFETY: `raw` is a live process handle owned by this thread.
        unsafe { WaitForSingleObject(raw as _, INFINITE) };
        driver::release();
        std::process::exit(0);
    });
}

/// The filter is fixed when a handle opens, so a port change opens a new
/// handle first and only then shuts the old one's receive side: packets are
/// never left undiverted in between, and the old handle stays alive (via
/// queued items) until its delayed packets are reinjected.
///
/// While idle no port is diverted, so the last handle closes and the driver
/// is stopped; going active again notes its state before the next open
/// loads it.
fn port_watcher(api: &Arc<Api>, shared: &Arc<Shared>) {
    let mut ports: Vec<u16> = Vec::new();
    let mut current: Option<Arc<Handle>> = None;
    let mut priority: i16 = 1;
    // The start-up probe loaded it.
    let mut loaded = true;
    loop {
        let active = shared.active.load(Ordering::Relaxed);
        if active && !loaded {
            driver::note_state();
            loaded = true;
        }
        let now = if active {
            udp::ports_of(&udp::client_pids())
        } else {
            Vec::new()
        };
        if now != ports {
            let next_priority = if priority == 1 { 2 } else { 1 };
            let next = if now.is_empty() {
                Ok(None)
            } else {
                Handle::open(api, &super::ports_filter(&now), next_priority)
                    .map(|h| Some(Arc::new(h)))
            };
            if let Ok(next) = next {
                if let Some(h) = &next {
                    let h = Arc::clone(h);
                    let s = Arc::clone(shared);
                    std::thread::spawn(move || receiver(&h, &s));
                }
                if let Some(old) = current.take() {
                    old.shutdown_recv();
                }
                current = next;
                priority = next_priority;
                ports = now;
            }
        }
        if !active && loaded && current.is_none() {
            driver::release();
            loaded = false;
        }
        std::thread::sleep(PORT_POLL);
    }
}

fn receiver(handle: &Arc<Handle>, shared: &Shared) {
    let mut buf = vec![0u8; PACKET_BUF];
    while let Some((len, addr)) = handle.recv(&mut buf) {
        let outbound = addr.outbound();
        let way = {
            let c = lock(&shared.conditions);
            if outbound {
                c.outbound
            } else {
                c.inbound
            }
        }
        .clamped();
        let item = (buf[..len.min(buf.len())].to_vec(), addr, Arc::clone(handle));
        let mut l = lock(&shared.lines);
        let Lines { out, inbound, rng } = &mut *l;
        let line = if outbound { out } else { inbound };
        if line.admit(Instant::now(), way, rng, item).is_some() {
            drop(l);
            shared.wake.notify_one();
        }
    }
}

fn sender(shared: &Shared) {
    let mut due: Vec<Item> = Vec::new();
    loop {
        {
            let mut l = lock(&shared.lines);
            loop {
                let now = Instant::now();
                while let Some(i) = l.out.pop_due(now) {
                    due.push(i);
                }
                while let Some(i) = l.inbound.pop_due(now) {
                    due.push(i);
                }
                if !due.is_empty() {
                    break;
                }
                let next = match (l.out.next_release(), l.inbound.next_release()) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                l = match next {
                    None => shared
                        .wake
                        .wait(l)
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                    Some(t) => {
                        shared
                            .wake
                            .wait_timeout(l, t.saturating_duration_since(now))
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .0
                    }
                };
            }
        }
        // Sent outside the lock so receivers never wait on driver calls.
        for (packet, addr, handle) in due.drain(..) {
            handle.send(&packet, &addr);
        }
    }
}

/// WinDivert loads its driver on the first open and leaves the service
/// running until reboot or `sc stop WinDivert`, so quitting the editor used
/// to leave it RUNNING. The helper stops it on the way out, unless it was
/// already running before (another program of the user's may be using it).
/// The stop is asked while this process still holds handles; Windows
/// finishes it once they close, a moment later when the process exits.
mod driver {
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
        SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
        SERVICE_STATUS, SERVICE_STOP, SERVICE_STOPPED,
    };

    /// Set when the service was not running before this helper loaded it.
    static OURS: AtomicBool = AtomicBool::new(false);

    /// Runs `body` on the WinDivert service, if it exists and opens.
    fn with_service<T>(body: impl FnOnce(SC_HANDLE) -> T) -> Option<T> {
        let name: Vec<u16> = "WinDivert\0".encode_utf16().collect();
        // SAFETY: handles are checked for null and closed once; `name` is a
        // NUL-terminated UTF-16 string alive for the call.
        unsafe {
            let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
            if manager.is_null() {
                return None;
            }
            let service = OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS | SERVICE_STOP);
            let result = (!service.is_null()).then(|| body(service));
            if !service.is_null() {
                CloseServiceHandle(service);
            }
            CloseServiceHandle(manager);
            result
        }
    }

    fn state(service: SC_HANDLE) -> Option<u32> {
        // SAFETY: `status` is a valid out pointer for a live handle.
        unsafe {
            let mut status: SERVICE_STATUS = std::mem::zeroed();
            (QueryServiceStatus(service, &mut status) != 0).then_some(status.dwCurrentState)
        }
    }

    /// Remembers whether the driver is this helper's to stop. Called before
    /// the first open; a service that doesn't exist yet is ours too.
    pub(super) fn note_state() {
        let running = with_service(state).flatten() == Some(SERVICE_RUNNING);
        OURS.store(!running, Ordering::Relaxed);
    }

    /// Stops the driver if this helper started it. Asked while handles are
    /// still open (packets waiting in the delay lines hold the old one),
    /// Windows finishes the stop once the last of them closes.
    pub(super) fn release() {
        if !OURS.swap(false, Ordering::Relaxed) {
            return;
        }
        with_service(|service| {
            if state(service) == Some(SERVICE_STOPPED) {
                return;
            }
            // SAFETY: `status` is a valid out pointer for a live handle.
            let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
            unsafe { ControlService(service, SERVICE_CONTROL_STOP, &mut status) };
        });
    }
}
