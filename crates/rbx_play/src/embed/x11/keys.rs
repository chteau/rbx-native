//! Typing into the embedded client: the Device Simulator's Android back
//! button and its keyboard cover's keys.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, InputFocus, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use super::{x11, Embedded};
use crate::embed::EmbedError;

/// A key the client is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Typed as the character, whatever the keyboard layout.
    Char(char),
    Backspace,
    Return,
    Escape,
}

const ESCAPE_KEYSYM: u32 = 0xff1b;
const BACKSPACE_KEYSYM: u32 = 0xff08;
const RETURN_KEYSYM: u32 = 0xff0d;
const SHIFT_KEYSYM: u32 = 0xffe1;

/// How long the pointer stays over the client after the last key: 100 ms
/// was enough in a live test, returning it at once lost the key.
const POINTER_RETURN: Duration = Duration::from_millis(150);
/// How long a newly borrowed keycode is left before it is typed: Sober
/// takes the new keymap a moment after the change (a key sent 100 ms after
/// it typed in a live test; 0.3 s leaves room).
const REMAP_SETTLE: Duration = Duration::from_millis(300);

/// What the typing thread is asked to do.
enum Job {
    /// Type into a window.
    Type { window: u32, keys: Vec<Key> },
    /// Give back every keycode lent to a character.
    Release,
}

/// The typing thread: one, so keys typed in quick succession arrive in
/// order, and a borrowed keycode's wait holds up only the keys after it,
/// never the editor.
static WORKER: Mutex<Option<Sender<Job>>> = Mutex::new(None);

fn send(job: Job) -> Result<(), EmbedError> {
    let mut worker = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    let job = match worker.as_ref() {
        Some(tx) => match tx.send(job) {
            Ok(()) => return Ok(()),
            // The thread died (its connection failed): start another.
            Err(mpsc::SendError(job)) => job,
        },
        None => job,
    };
    start(&mut worker, job)
}

/// Starts the typing thread with `job` as its first.
fn start(worker: &mut Option<Sender<Job>>, job: Job) -> Result<(), EmbedError> {
    let (conn, root) = super::connect()?;
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("embed-typing".into())
        .spawn(move || Typist::new(conn, root).run(rx))
        .map_err(|e| EmbedError::X11(format!("typing thread: {e}")))?;
    tx.send(job)
        .map_err(|_| EmbedError::X11("typing thread exited".to_owned()))?;
    *worker = Some(tx);
    Ok(())
}

/// The typing thread's own connection and what it remembers between jobs.
struct Typist {
    conn: RustConnection,
    root: u32,
    /// Where the pointer was before the current burst of keys.
    home: Option<(i16, i16)>,
    /// Keycodes lent to characters the keymap lacks, oldest first. A lent
    /// keycode keeps its character for the rest of the Play, so only a
    /// character's first press waits for the client's keymap reload.
    borrowed: Vec<u8>,
}

impl Typist {
    fn new(conn: RustConnection, root: u32) -> Self {
        Typist {
            conn,
            root,
            home: None,
            borrowed: Vec::new(),
        }
    }

    fn run(mut self, rx: Receiver<Job>) {
        loop {
            // Waiting with the pointer away from home: a short wait, after
            // which the burst is over and it goes back.
            let job = match self.home {
                Some((x, y)) => match rx.recv_timeout(POINTER_RETURN) {
                    Ok(job) => job,
                    Err(RecvTimeoutError::Timeout) => {
                        let _ = self
                            .conn
                            .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, x, y);
                        let _ = self.conn.flush();
                        self.home = None;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match rx.recv() {
                    Ok(job) => job,
                    Err(_) => return,
                },
            };
            let done = match job {
                Job::Type { window, keys } => self.type_keys(window, &keys),
                Job::Release => self.release(),
            };
            if let Err(e) = done {
                eprintln!("rbx_play: typing into the embedded client: {e}");
            }
        }
    }

    fn release(&mut self) -> Result<(), EmbedError> {
        for code in self.borrowed.drain(..) {
            self.conn
                .change_keyboard_mapping(1, code, 2, &[0, 0])
                .map_err(x11)?;
        }
        self.conn.flush().map_err(x11)
    }

    fn fake(&self, kind: u8, code: u8) -> Result<(), EmbedError> {
        self.conn
            .xtest_fake_input(kind, code, x11rb::CURRENT_TIME, self.root, 0, 0, 0)
            .map_err(x11)?;
        Ok(())
    }

    /// XTest types into whatever has the focus, hence the focus first, and
    /// Sober reads the keyboard only while the pointer is over it
    /// (measured: focus alone, or moving the pointer back at once, loses the
    /// key), so the pointer is put in the middle of the window until the
    /// burst ends. A character the keymap has no key for is typed through a
    /// spare keycode mapped to it.
    fn type_keys(&mut self, window: u32, keys: &[Key]) -> Result<(), EmbedError> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        let per = usize::from(map.keysyms_per_keycode.max(1));
        let mut syms = map.keysyms;
        let shift = find_keysym(&syms, per, min, SHIFT_KEYSYM).map(|(code, _)| code);

        if self.home.is_none() {
            let pointer = self
                .conn
                .query_pointer(self.root)
                .map_err(x11)?
                .reply()
                .map_err(x11)?;
            self.home = Some((pointer.root_x, pointer.root_y));
        }
        let geometry = self
            .conn
            .get_geometry(window)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        let middle = |extent: u16| i16::try_from(extent / 2).unwrap_or(i16::MAX);
        let (x, y) = (middle(geometry.width), middle(geometry.height));

        for &key in keys {
            let keysym = key_keysym(key);
            let (code, shifted) = match find_keysym(&syms, per, min, keysym) {
                Some(found) => found,
                None => {
                    // A free keycode, or else the one lent longest ago.
                    let code = match spare_keycode(&syms, per, min) {
                        Some(code) => code,
                        None if !self.borrowed.is_empty() => self.borrowed.remove(0),
                        None => {
                            return Err(EmbedError::X11("no spare keycode to type with".to_owned()))
                        }
                    };
                    self.borrowed.push(code);
                    self.conn
                        .change_keyboard_mapping(1, code, 2, &[keysym, keysym])
                        .map_err(x11)?;
                    self.conn.sync().map_err(x11)?;
                    let at = usize::from(code - min) * per;
                    syms[at..at + per].fill(0);
                    syms[at] = keysym;
                    syms[at + 1.min(per - 1)] = keysym;
                    std::thread::sleep(REMAP_SETTLE);
                    (code, false)
                }
            };
            // Taken again before every key: the press on the cover that
            // typed it gave the editor the focus back (`FocusFollow`),
            // and a borrowed keycode's wait gives that time to land.
            self.conn
                .warp_pointer(x11rb::NONE, window, 0, 0, 0, 0, x, y)
                .map_err(x11)?;
            self.conn
                .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
                .map_err(x11)?;
            let shift = shift.filter(|_| shifted);
            if let Some(shift) = shift {
                self.fake(KEY_PRESS_EVENT, shift)?;
            }
            self.fake(KEY_PRESS_EVENT, code)?;
            self.fake(KEY_RELEASE_EVENT, code)?;
            if let Some(shift) = shift {
                self.fake(KEY_RELEASE_EVENT, shift)?;
            }
        }
        self.conn.flush().map_err(x11)
    }
}

/// The X keysym for `c`: Latin-1 is its own code point, the rest of
/// Unicode is offset by 0x0100_0000 (X11's keysym convention).
pub(crate) fn char_keysym(c: char) -> u32 {
    match u32::from(c) {
        cp @ (0x20..=0x7e | 0xa0..=0xff) => cp,
        cp => 0x0100_0000 + cp,
    }
}

fn key_keysym(key: Key) -> u32 {
    match key {
        Key::Char(c) => char_keysym(c),
        Key::Backspace => BACKSPACE_KEYSYM,
        Key::Return => RETURN_KEYSYM,
        Key::Escape => ESCAPE_KEYSYM,
    }
}

/// Where `keysym` sits in a `GetKeyboardMapping` reply laid out `per`
/// keysyms to a keycode from `min`: the keycode, and whether it needs
/// Shift (the second level). Only the first two levels count: the rest
/// need modifiers XTest would have to guess.
pub(crate) fn find_keysym(syms: &[u32], per: usize, min: u8, keysym: u32) -> Option<(u8, bool)> {
    syms.chunks(per.max(1))
        .enumerate()
        .find_map(|(at, levels)| {
            let level = levels.iter().take(2).position(|&s| s == keysym)?;
            let code = u8::try_from(usize::from(min) + at).ok()?;
            Some((code, level == 1))
        })
}

/// A keycode with no keysym at all, free to lend.
pub(crate) fn spare_keycode(syms: &[u32], per: usize, min: u8) -> Option<u8> {
    syms.chunks(per.max(1))
        .enumerate()
        .rev()
        .find(|(_, levels)| levels.iter().all(|&s| s == 0))
        .and_then(|(at, _)| u8::try_from(usize::from(min) + at).ok())
}

impl Embedded {
    /// Types Escape into the window.
    pub fn press_escape(&self) -> Result<(), EmbedError> {
        self.type_keys(&[Key::Escape])
    }

    /// Types `keys` into the window with XTest — real key events, which
    /// clients accept where they drop `SendEvent` ones — on the typing
    /// thread (`Typist`), so this returns at once.
    pub fn type_keys(&self, keys: &[Key]) -> Result<(), EmbedError> {
        send(Job::Type {
            window: self.window,
            keys: keys.to_vec(),
        })
    }

    /// Gives back every keycode typing lent to a character.
    pub(super) fn return_borrowed(&self) {
        let _ = send(Job::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::{char_keysym, find_keysym, spare_keycode};

    #[test]
    fn keysyms_follow_x11s_unicode_convention() {
        assert_eq!(char_keysym('a'), 0x61);
        assert_eq!(char_keysym('\u{e9}'), 0xe9);
        assert_eq!(char_keysym('\u{20ac}'), 0x0100_20ac);
        assert_eq!(char_keysym('\u{1f602}'), 0x0101_f602);
    }

    #[test]
    fn finds_levels_and_spare_keycodes() {
        // Keycodes 8..=11, two levels each: a/A, (none), 1/!, (none).
        let syms = [0x61, 0x41, 0, 0, 0x31, 0x21, 0, 0];
        assert_eq!(find_keysym(&syms, 2, 8, 0x61), Some((8, false)));
        assert_eq!(find_keysym(&syms, 2, 8, 0x21), Some((10, true)));
        assert_eq!(find_keysym(&syms, 2, 8, 0x7a), None);
        assert_eq!(spare_keycode(&syms, 2, 8), Some(11));
        assert_eq!(spare_keycode(&[0x61, 0x41], 2, 8), None);
    }
}
