//! The virtual pad end to end: made through `/dev/uinput`, read back from
//! the `/dev/input/event*` node the kernel creates for it. Skips, saying
//! why, where this user can't open either.
#![cfg(target_os = "linux")]

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use rbx_play::gamepad::{Button, Device, PadError, Side, VirtualPad};

const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const ABS_X: u16 = 0x00;

/// The event node, once udev has given it this user's permissions.
fn open_node(pad: &VirtualPad) -> Option<File> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(node) = pad.event_node() {
            match File::open(&node) {
                Ok(file) => return Some(file),
                Err(error) if Instant::now() > deadline => {
                    eprintln!("skipped: can't read {}: {error}", node.display());
                    return None;
                }
                Err(error) if error.kind() != ErrorKind::PermissionDenied => {
                    eprintln!("skipped: can't read {}: {error}", node.display());
                    return None;
                }
                Err(_) => {}
            }
        } else if Instant::now() > deadline {
            eprintln!("skipped: the pad's event node never appeared");
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `(type, code, value)` of every event up to and including the next
/// `SYN_REPORT`.
fn read_frame(file: &mut File) -> Vec<(u16, u16, i32)> {
    let size = std::mem::size_of::<libc::input_event>();
    let mut frame = Vec::new();
    loop {
        let mut poll = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        assert!(
            unsafe { libc::poll(&mut poll, 1, 2000) } > 0,
            "no event in 2 s"
        );
        let mut buf = vec![0u8; size];
        file.read_exact(&mut buf).unwrap();
        // The timestamp comes first; type, code and value are the last 8 bytes.
        let tail = &buf[size - 8..];
        let kind = u16::from_ne_bytes([tail[0], tail[1]]);
        let code = u16::from_ne_bytes([tail[2], tail[3]]);
        let value = i32::from_ne_bytes([tail[4], tail[5], tail[6], tail[7]]);
        if kind == 0 {
            return frame;
        }
        frame.push((kind, code, value));
    }
}

fn sysfs(pad: &VirtualPad, file: &str) -> String {
    let node = pad.event_node().unwrap();
    let event = node.file_name().unwrap();
    std::fs::read_to_string(format!(
        "/sys/class/input/{}/device/{file}",
        event.to_string_lossy()
    ))
    .unwrap()
    .trim()
    .to_owned()
}

#[test]
fn a_virtual_xbox_pad_reads_back_as_one() {
    let mut pad = match VirtualPad::create(Device::Xbox) {
        Ok(pad) => pad,
        Err(PadError::NoAccess(reason)) => {
            eprintln!("skipped: {reason}");
            return;
        }
        Err(error) => panic!("{error}"),
    };
    assert_eq!(sysfs(&pad, "name"), "Microsoft X-Box 360 pad");
    assert_eq!(sysfs(&pad, "id/vendor"), "045e");
    assert_eq!(sysfs(&pad, "id/product"), "028e");
    let Some(mut node) = open_node(&pad) else {
        return;
    };

    pad.press(Button::South).unwrap();
    assert_eq!(read_frame(&mut node), vec![(EV_KEY, 0x130, 1)]);
    pad.stick(Side::Left, 1., 0.).unwrap();
    assert_eq!(read_frame(&mut node), vec![(EV_ABS, ABS_X, 32767)]);
    pad.release(Button::South).unwrap();
    assert_eq!(read_frame(&mut node), vec![(EV_KEY, 0x130, 0)]);

    let node_path = pad.event_node().unwrap();
    drop(pad);
    let deadline = Instant::now() + Duration::from_secs(2);
    while node_path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!node_path.exists(), "dropping the pad unplugs it");
}
