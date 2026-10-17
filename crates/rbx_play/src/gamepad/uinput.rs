//! The pad as a Linux `uinput` device, through raw ioctls
//! (`Documentation/input/uinput.rst`).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use libc::{c_char, c_int, Ioctl};

use super::layout::{self, Layout, EV_ABS, EV_KEY, EV_SYN};
use super::{Button, Device, PadError, PadState, Side};

const UINPUT: &str = "/dev/uinput";
const U: u32 = b'U' as u32;
const UI_DEV_CREATE: Ioctl = libc::_IO(U, 1);
const UI_DEV_DESTROY: Ioctl = libc::_IO(U, 2);
const UI_DEV_SETUP: Ioctl = libc::_IOW::<libc::uinput_setup>(U, 3);
const UI_ABS_SETUP: Ioctl = libc::_IOW::<libc::uinput_abs_setup>(U, 4);
const UI_SET_EVBIT: Ioctl = libc::_IOW::<c_int>(U, 100);
const UI_SET_KEYBIT: Ioctl = libc::_IOW::<c_int>(U, 101);
const UI_SET_ABSBIT: Ioctl = libc::_IOW::<c_int>(U, 103);
const SYSNAME_LEN: usize = 64;
const UI_GET_SYSNAME: Ioctl = libc::_IOR::<[c_char; SYSNAME_LEN]>(U, 44);
const BUS_USB: u16 = 0x03;

/// How to let this user open `/dev/uinput` — the editor never runs sudo.
const ALLOW_UINPUT: &str = "Allow it with a udev rule: put \
    KERNEL==\"uinput\", SUBSYSTEM==\"misc\", TAG+=\"uaccess\", OPTIONS+=\"static_node=uinput\" \
    in /etc/udev/rules.d/60-uinput.rules, run `sudo udevadm control --reload && sudo udevadm \
    trigger`, then log out and in. (Steam's controller rules do the same.)";

/// A connected virtual pad; dropping it unplugs it.
pub struct VirtualPad {
    file: File,
    device: Device,
    layout: &'static Layout,
    sent: PadState,
}

fn check(result: c_int) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl VirtualPad {
    /// Plugs in a pad that looks like `device` to every reader of
    /// `/dev/input`.
    pub fn create(device: Device) -> Result<VirtualPad, PadError> {
        let layout = layout::layout(device)
            .ok_or_else(|| PadError::Unavailable(device.unavailable().unwrap_or("").into()))?;
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(UINPUT)
            .map_err(|error| match error.kind() {
                io::ErrorKind::PermissionDenied => PadError::NoAccess(format!(
                    "{UINPUT} isn't writable by this user. {ALLOW_UINPUT}"
                )),
                io::ErrorKind::NotFound => PadError::NoAccess(format!(
                    "{UINPUT} doesn't exist: the uinput module isn't loaded. Load it with \
                     `sudo modprobe uinput` (and list it in /etc/modules-load.d/ to keep it)."
                )),
                _ => PadError::Io(error),
            })?;
        set_up(&file, layout).map_err(PadError::Io)?;
        Ok(VirtualPad {
            file,
            device,
            layout,
            sent: PadState::default(),
        })
    }

    pub fn device(&self) -> Device {
        self.device
    }

    /// What the pad last reported.
    pub fn state(&self) -> PadState {
        self.sent
    }

    /// Reports `next` as one frame: only what changed, then `SYN_REPORT`.
    pub fn send(&mut self, next: PadState) -> io::Result<()> {
        let events: Vec<libc::input_event> = self
            .layout
            .encode(&self.sent, &next)
            .into_iter()
            .map(|event| libc::input_event {
                // SAFETY: an all-zero timeval is valid; the kernel stamps it.
                time: unsafe { std::mem::zeroed() },
                type_: event.kind,
                code: event.code,
                value: event.value,
            })
            .collect();
        if !events.is_empty() {
            // SAFETY: `input_event` is plain old data; the slice covers
            // exactly the vector's initialised elements.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    events.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(events.as_slice()),
                )
            };
            self.file.write_all(bytes)?;
        }
        self.sent = next;
        Ok(())
    }

    pub fn press(&mut self, button: Button) -> io::Result<()> {
        self.with(|state| state.set(button, true))
    }

    pub fn release(&mut self, button: Button) -> io::Result<()> {
        self.with(|state| state.set(button, false))
    }

    pub fn stick(&mut self, side: Side, x: f32, y: f32) -> io::Result<()> {
        self.with(|state| state.set_stick(side, x, y))
    }

    pub fn trigger(&mut self, side: Side, value: f32) -> io::Result<()> {
        self.with(|state| state.set_trigger(side, value))
    }

    fn with(&mut self, change: impl FnOnce(&mut PadState)) -> io::Result<()> {
        let mut next = self.sent;
        change(&mut next);
        self.send(next)
    }

    /// The `/dev/input/eventN` node the kernel made for the pad.
    pub fn event_node(&self) -> Option<PathBuf> {
        let mut name = [0 as c_char; SYSNAME_LEN];
        // SAFETY: the buffer is the size the request encodes.
        check(unsafe { libc::ioctl(self.file.as_raw_fd(), UI_GET_SYSNAME, name.as_mut_ptr()) })
            .ok()?;
        // SAFETY: the kernel wrote a NUL-terminated name into the buffer.
        let sysname = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) };
        let dir = PathBuf::from("/sys/devices/virtual/input").join(sysname.to_str().ok()?);
        std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.starts_with("event")
                .then(|| PathBuf::from("/dev/input").join(name))
        })
    }
}

impl Drop for VirtualPad {
    fn drop(&mut self) {
        // Closing the descriptor would destroy it too; explicit is clearer
        // and releases everything held at once.
        // SAFETY: a plain request on a descriptor this value owns.
        unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY) };
    }
}

/// Declares the layout's events and axes, then creates the device.
fn set_up(file: &File, layout: &Layout) -> io::Result<()> {
    let fd = file.as_raw_fd();
    // SAFETY (every ioctl below): requests from `uinput.h` with arguments of
    // the type each encodes, on an open uinput descriptor.
    for kind in [EV_SYN, EV_KEY, EV_ABS] {
        check(unsafe { libc::ioctl(fd, UI_SET_EVBIT, c_int::from(kind)) })?;
    }
    for code in layout.keys() {
        check(unsafe { libc::ioctl(fd, UI_SET_KEYBIT, c_int::from(code)) })?;
    }
    for (code, range) in layout.axes() {
        check(unsafe { libc::ioctl(fd, UI_SET_ABSBIT, c_int::from(code)) })?;
        let setup = libc::uinput_abs_setup {
            code,
            absinfo: libc::input_absinfo {
                value: (range.min + range.max + 1).div_euclid(2),
                minimum: range.min,
                maximum: range.max,
                fuzz: range.fuzz,
                flat: range.flat,
                resolution: 0,
            },
        };
        check(unsafe { libc::ioctl(fd, UI_ABS_SETUP, &setup) })?;
    }
    let identity = layout.identity;
    // SAFETY: `uinput_setup` is plain old data; all-zero is a valid value.
    let mut setup: libc::uinput_setup = unsafe { std::mem::zeroed() };
    setup.id = libc::input_id {
        bustype: BUS_USB,
        vendor: identity.vendor,
        product: identity.product,
        version: identity.version,
    };
    // The last byte stays NUL.
    for (slot, byte) in setup.name.iter_mut().zip(identity.name.bytes().take(79)) {
        *slot = byte as c_char;
    }
    check(unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup) })?;
    check(unsafe { libc::ioctl(fd, UI_DEV_CREATE) })
}
