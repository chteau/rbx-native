//! The drawn backdrop: one worker thread captures for every window that
//! paints one, the UI thread only swaps in the picture it hands back.
//!
//! Refreshes are driven by XDamage on the windows below and by the root's
//! structure events (a top-level moved, restacked, mapped or unmapped, ours
//! included), never by a timer: an idle desktop costs nothing. A busy one is
//! held to [`MIN_INTERVAL`] per window.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures::channel::mpsc::{unbounded, UnboundedSender};
use futures::StreamExt as _;
use gpui_kit::*;
use image::RgbaImage;
use x11rb::connection::Connection as _;
use x11rb::protocol::damage::ConnectionExt as _;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::Event;

use super::capture::{Capture, Frame, Placement};
use super::wallpaper::Rect;
use super::BlurSupport;

/// The fastest one window's backdrop is refreshed.
pub(super) const MIN_INTERVAL: Duration = Duration::from_millis(250);

/// How often the worker looks for X events while it has windows to serve.
const POLL: Duration = Duration::from_millis(50);

/// Lets a refresh through when something changed and the last one is at
/// least [`MIN_INTERVAL`] old.
#[derive(Default)]
pub(super) struct Throttle {
    last: Option<Instant>,
}

impl Throttle {
    pub(super) fn due(&mut self, now: Instant, dirty: bool) -> bool {
        let due = dirty && self.last.is_none_or(|last| now - last >= MIN_INTERVAL);
        if due {
            self.last = Some(now);
        }
        due
    }
}

#[derive(Default)]
struct Slot(Mutex<Option<Option<Frame>>>);

enum Msg {
    Watch(u32, Arc<Slot>, UnboundedSender<()>),
    Unwatch(u32),
}

fn send(msg: Msg) {
    static WORKER: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
    let sender = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("window-blur".into())
            .spawn(move || run(rx));
        Mutex::new(tx)
    });
    if let Ok(sender) = sender.lock() {
        let _ = sender.send(msg);
    }
}

/// Logged the first time only: a broken capture would otherwise repeat
/// itself four times a second.
fn log_once(err: &dyn std::fmt::Display) {
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!("rbxstudio: drawn window blur off, showing see-through: {err}");
    }
}

struct Watched {
    slot: Arc<Slot>,
    notify: UnboundedSender<()>,
    at: Placement,
    dirty: bool,
    throttle: Throttle,
}

impl Watched {
    fn publish(&self, frame: Option<Frame>) {
        if let Ok(mut slot) = self.slot.0.lock() {
            *slot = Some(frame);
        }
        let _ = self.notify.unbounded_send(());
    }

    /// Whether a top-level at `area` (if known) changing touches this window.
    fn touched_by(&self, window: u32, area: Option<Rect>) -> bool {
        window == self.at.top
            || self.at.below.contains(&window)
            || match (area, self.at.rect) {
                (Some(area), Some(rect)) => area.intersects(&rect),
                (None, _) => true,
                (_, None) => true,
            }
    }
}

fn run(rx: Receiver<Msg>) {
    let mut x = match Capture::connect() {
        Ok(x) => x,
        Err(err) => {
            log_once(&err);
            // Still answer, so every window falls back to see-through.
            while let Ok(msg) = rx.recv() {
                if let Msg::Watch(_, slot, notify) = msg {
                    *slot.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(None);
                    let _ = notify.unbounded_send(());
                }
            }
            return;
        }
    };
    let mut watched: HashMap<u32, Watched> = HashMap::new();
    let mut damages = HashMap::new();
    let stacking = x
        .conn
        .intern_atom(false, b"_NET_CLIENT_LIST_STACKING")
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.atom);
    loop {
        let msg = match watched.is_empty() {
            true => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            false => rx.recv_timeout(POLL),
        };
        let mut msgs = Vec::new();
        match msg {
            Ok(msg) => msgs.push(msg),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        msgs.extend(rx.try_iter());
        for msg in msgs {
            match msg {
                Msg::Watch(window, slot, notify) => {
                    let entry = Watched {
                        slot,
                        notify,
                        at: Placement::default(),
                        dirty: true,
                        throttle: Throttle::default(),
                    };
                    if let Some(mut old) = watched.insert(window, entry) {
                        x.release(&mut old.at);
                    }
                }
                Msg::Unwatch(window) => {
                    if let Some(mut old) = watched.remove(&window) {
                        x.release(&mut old.at);
                    }
                }
            }
        }

        while let Ok(Some(event)) = x.conn.poll_for_event() {
            let (window, area) = match event {
                Event::DamageNotify(event) => {
                    let _ = x
                        .conn
                        .damage_subtract(event.damage, x11rb::NONE, x11rb::NONE);
                    let area = Rect {
                        x: (event.geometry.x + event.area.x) as i32,
                        y: (event.geometry.y + event.area.y) as i32,
                        w: event.area.width as u32,
                        h: event.area.height as u32,
                    };
                    // Damage outside our rectangle is no reason to refresh.
                    for entry in watched.values_mut() {
                        if entry.at.rect.is_some_and(|rect| rect.intersects(&area)) {
                            entry.dirty = true;
                        }
                    }
                    continue;
                }
                Event::ConfigureNotify(event) => (
                    event.window,
                    Some(Rect {
                        x: event.x as i32,
                        y: event.y as i32,
                        w: event.width as u32,
                        h: event.height as u32,
                    }),
                ),
                Event::MapNotify(event) => (event.window, None),
                Event::UnmapNotify(event) => (event.window, None),
                Event::DestroyNotify(event) => {
                    damages.remove(&event.window);
                    (event.window, None)
                }
                Event::PropertyNotify(event) if Some(event.atom) == stacking => (x.root, None),
                _ => continue,
            };
            for entry in watched.values_mut() {
                if entry.touched_by(window, area) {
                    entry.dirty = true;
                }
            }
        }

        let now = Instant::now();
        let mut gone = Vec::new();
        for (&window, entry) in &mut watched {
            if entry.notify.is_closed() {
                gone.push(window);
                continue;
            }
            if !entry.throttle.due(now, entry.dirty) {
                continue;
            }
            entry.dirty = false;
            match x.capture(window, &mut entry.at) {
                Ok(Some(frame)) => entry.publish(Some(frame)),
                Ok(None) => {}
                Err(err) => {
                    // A window that closed is no failure: just wake its task
                    // so it notices.
                    match x
                        .conn
                        .get_geometry(window)
                        .ok()
                        .map(|cookie| cookie.reply())
                    {
                        Some(Ok(_)) => {
                            log_once(&err);
                            entry.publish(None);
                        }
                        _ => drop(entry.notify.unbounded_send(())),
                    }
                    gone.push(window);
                }
            }
        }
        for window in gone {
            if let Some(mut old) = watched.remove(&window) {
                x.release(&mut old.at);
            }
        }
        let below: HashSet<u32> = watched
            .values()
            .flat_map(|entry| entry.at.below.iter().copied())
            .collect();
        if let Err(err) = x.track(&mut damages, &below) {
            log_once(&err);
        }
    }
}

/// The UI thread's side, per X window.
struct Entry {
    slot: Arc<Slot>,
    image: Option<Arc<RenderImage>>,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<u32, Entry>> = RefCell::new(HashMap::new());
}

/// The window's bottom layer: what is behind it, blurred, with the theme's
/// ground over it. `None` unless the editor draws the blur itself, and until
/// the first picture of what's behind has arrived.
fn backdrop(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let id = crate::pointer_lock::window_id(window)?;
    let wanted = super::support() == BlurSupport::Drawn
        && crate::theme::active().effects.window == WindowBackgroundAppearance::Blurred;
    if !wanted {
        let old = WINDOWS.with_borrow_mut(|windows| windows.remove(&id))?;
        send(Msg::Unwatch(id));
        if let Some(image) = old.image {
            cx.drop_image(image, Some(window));
        }
        return None;
    }

    let (old, image) = WINDOWS.with_borrow_mut(|windows| {
        let entry = windows.entry(id).or_insert_with(|| watch(id, window, cx));
        let frame = entry.slot.0.lock().ok().and_then(|mut slot| slot.take());
        let old = match frame {
            Some(frame) => {
                let fresh = frame.and_then(|frame| {
                    let buffer = RgbaImage::from_raw(frame.width, frame.height, frame.bgra)?;
                    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
                });
                std::mem::replace(&mut entry.image, fresh)
            }
            None => None,
        };
        (old, entry.image.clone())
    });
    if let Some(old) = old {
        cx.drop_image(old, Some(window));
    }
    let image = image?;
    let ground = crate::tokens::black();
    Some(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                // The picture covers the whole X window, which can be larger
                // than this element (the toolkit's shadow inset): each pixel
                // lands back over what it shows.
                let whole = Bounds::new(point(px(0.), px(0.)), window.viewport_size());
                let _ = window.paint_image(bounds, whole, Corners::default(), image, 0, false);
                window.paint_quad(fill(bounds, ground));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element(),
    )
}

/// Starts serving `id`: the worker captures for it, and every picture it
/// hands back redraws the window.
fn watch(id: u32, window: &mut Window, cx: &mut App) -> Entry {
    let slot = Arc::new(Slot::default());
    let (notify, mut arrived) = unbounded();
    send(Msg::Watch(id, slot.clone(), notify));
    window
        .spawn(cx, async move |cx| {
            while arrived.next().await.is_some() {
                if cx.update(|window, _| window.refresh()).is_err() {
                    break;
                }
            }
            WINDOWS.with_borrow_mut(|windows| windows.remove(&id));
        })
        .detach();
    Entry { slot, image: None }
}

/// The view [`super::wrap`] puts between `Root` and a window's own view.
pub(crate) struct Backdropped(pub(super) AnyView);

impl Render for Backdropped {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .children(backdrop(window, cx))
            .child(self.0.clone())
    }
}
