//! Reading what is behind one of our windows: every viewable window below
//! it, composited by the X server through Render (windows are redirected
//! under a compositing WM, so a picture on one reads its own contents even
//! where something covers it), shrunk on the way, then laid over the
//! wallpaper and blurred here.

use std::time::SystemTime;

use image::RgbaImage;
use x11rb::connection::Connection;
use x11rb::protocol::damage::{self, ConnectionExt as _};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::render::{
    self, ConnectionExt as _, CreatePictureAux, PictOp, PictType, Pictformat,
};
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConnectionExt as _, EventMask, ImageFormat, ImageOrder, MapState,
    Rectangle, SubwindowMode, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::wallpaper::{self, Rect};

/// Each side of the backdrop is this many times smaller than the window:
/// the blur would throw the detail away anyway.
pub(super) const SCALE: u32 = 4;

/// The blur's sigma in screen pixels, a frosted pane's worth.
const SIGMA: f32 = 24.0;

pub(super) struct Frame {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Where one of our windows is and what lies below it, from its last capture.
#[derive(Default)]
pub(super) struct Placement {
    pub top: Window,
    pub rect: Option<Rect>,
    pub below: Vec<Window>,
    target: Option<(u32, u32, u32, u32)>,
    /// The last capture before the wallpaper and the blur, hashed: damage
    /// on a window that is itself covered changes nothing worth redrawing.
    last: u64,
}

struct Background {
    stamp: Option<SystemTime>,
    key: Option<(wallpaper::Settings, (u32, u32), Vec<Rect>)>,
    canvas: RgbaImage,
}

pub(super) struct Capture {
    pub conn: RustConnection,
    pub root: Window,
    root_size: (u32, u32),
    formats: std::collections::HashMap<u32, Pictformat>,
    argb: Pictformat,
    background: Background,
}

/// A root child as the capture sees it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Candidate {
    pub window: Window,
    pub area: Rect,
    /// Mapped, viewable and drawable (not input-only).
    pub shown: bool,
    pub format: Pictformat,
}

/// The root's children under `top`, bottom first (the order `QueryTree`
/// gives them in), or `None` if `top` isn't one of them.
pub(super) fn stack_below(children: &[Window], top: Window) -> Option<&[Window]> {
    let ours = children.iter().position(|&child| child == top)?;
    Some(&children[..ours])
}

/// The windows that show through `rect`, bottom first.
pub(super) fn visible_below(stack: Vec<Candidate>, rect: Rect) -> Vec<Candidate> {
    stack
        .into_iter()
        .filter(|candidate| candidate.shown && candidate.area.intersects(&rect))
        .collect()
}

pub(super) fn extensions_present(conn: &impl Connection) -> bool {
    ["Composite", "RENDER", "DAMAGE"]
        .iter()
        .all(|name| conn.extension_information(name).ok().flatten().is_some())
}

type Res<T> = Result<T, Box<dyn std::error::Error>>;

impl Capture {
    pub(super) fn connect() -> Res<Capture> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let root_size = {
            let screen = &conn.setup().roots[screen];
            (
                screen.width_in_pixels as u32,
                screen.height_in_pixels as u32,
            )
        };
        if conn.setup().image_byte_order != ImageOrder::LSB_FIRST {
            return Err("the X server sends big-endian images".into());
        }
        conn.render_query_version(0, 11)?.reply()?;
        conn.damage_query_version(1, 1)?.reply()?;
        let pict = conn.render_query_pict_formats()?.reply()?;
        let argb = pict
            .formats
            .iter()
            .find(|f| {
                f.type_ == PictType::DIRECT
                    && f.depth == 32
                    && f.direct.alpha_mask == 0xff
                    && f.direct.alpha_shift == 24
                    && f.direct.red_shift == 16
                    && f.direct.blue_shift == 0
            })
            .map(|f| f.id)
            .ok_or("the X server has no 32-bit ARGB format")?;
        let formats = pict
            .screens
            .iter()
            .flat_map(|screen| &screen.depths)
            .flat_map(|depth| &depth.visuals)
            .map(|visual| (visual.visual, visual.format))
            .collect();
        // Every top-level's moves, restacks, maps and unmaps, plus the
        // stacking list GNOME rewrites when focus changes.
        conn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new()
                .event_mask(EventMask::SUBSTRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE),
        )?;
        conn.flush()?;
        Ok(Capture {
            conn,
            root,
            root_size,
            formats,
            argb,
            background: Background {
                stamp: None,
                key: None,
                canvas: RgbaImage::new(0, 0),
            },
        })
    }

    /// The top-level (child of the root) that holds `window`.
    fn top_level(&self, mut window: Window) -> Res<Window> {
        loop {
            let parent = self.conn.query_tree(window)?.reply()?.parent;
            if parent == self.root || parent == x11rb::NONE {
                return Ok(window);
            }
            window = parent;
        }
    }

    /// What is behind `window`, blurred, or `None` while it isn't on screen
    /// or hasn't changed since the last capture.
    pub(super) fn capture(&mut self, window: Window, at: &mut Placement) -> Res<Option<Frame>> {
        let conn = &self.conn;
        at.top = self.top_level(window)?;
        let viewable = conn.get_window_attributes(at.top)?.reply()?.map_state == MapState::VIEWABLE;
        let size = conn.get_geometry(window)?.reply()?;
        let origin = conn
            .translate_coordinates(window, self.root, 0, 0)?
            .reply()?;
        let rect = Rect {
            x: origin.dst_x as i32,
            y: origin.dst_y as i32,
            w: size.width as u32,
            h: size.height as u32,
        };
        at.rect = Some(rect);
        if !viewable || rect.w == 0 || rect.h == 0 {
            at.below.clear();
            return Ok(None);
        }

        let children = conn.query_tree(self.root)?.reply()?.children;
        let Some(stack) = stack_below(&children, at.top) else {
            return Ok(None);
        };
        let cookies = stack
            .iter()
            .map(|&child| {
                Ok((
                    child,
                    conn.get_window_attributes(child)?,
                    conn.get_geometry(child)?,
                ))
            })
            .collect::<Res<Vec<_>>>()?;
        let mut candidates = Vec::new();
        for (window, attributes, geometry) in cookies {
            let (Ok(attributes), Ok(geometry)) = (attributes.reply(), geometry.reply()) else {
                continue; // gone since the tree was read
            };
            let Some(&format) = self.formats.get(&attributes.visual) else {
                continue;
            };
            let border = geometry.border_width as i32;
            candidates.push(Candidate {
                window,
                area: Rect {
                    x: geometry.x as i32 + border,
                    y: geometry.y as i32 + border,
                    w: geometry.width as u32,
                    h: geometry.height as u32,
                },
                shown: attributes.map_state == MapState::VIEWABLE
                    && attributes.class == WindowClass::INPUT_OUTPUT,
                format,
            });
        }
        let below = visible_below(candidates, rect);
        at.below = below.iter().map(|candidate| candidate.window).collect();

        let (w, h) = (rect.w.div_ceil(SCALE), rect.h.div_ceil(SCALE));
        let target = self.target(at, w, h)?;
        let conn = &self.conn;
        let clear = render::Color {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0,
        };
        let whole = Rectangle {
            x: 0,
            y: 0,
            width: w as u16,
            height: h as u16,
        };
        conn.render_fill_rectangles(PictOp::SRC, target, clear, &[whole])?;
        let scale = render::Transform {
            matrix11: (SCALE as i32) << 16,
            matrix22: (SCALE as i32) << 16,
            matrix33: 1 << 16,
            matrix12: 0,
            matrix13: 0,
            matrix21: 0,
            matrix23: 0,
            matrix31: 0,
            matrix32: 0,
        };
        for Candidate {
            window: child,
            area,
            format,
            ..
        } in below
        {
            let picture = conn.generate_id()?;
            conn.render_create_picture(
                picture,
                child,
                format,
                &CreatePictureAux::new().subwindowmode(SubwindowMode::INCLUDE_INFERIORS),
            )?;
            conn.render_set_picture_transform(picture, scale)?;
            conn.render_set_picture_filter(picture, b"bilinear", &[])?;
            conn.render_composite(
                PictOp::OVER,
                picture,
                x11rb::NONE,
                target,
                0,
                0,
                0,
                0,
                (area.x - rect.x).div_euclid(SCALE as i32) as i16,
                (area.y - rect.y).div_euclid(SCALE as i32) as i16,
                area.w.div_ceil(SCALE) as u16,
                area.h.div_ceil(SCALE) as u16,
            )?;
            conn.render_free_picture(picture)?;
        }
        let pixmap = at.target.map(|(pixmap, ..)| pixmap).unwrap_or_default();
        let image = conn
            .get_image(ImageFormat::Z_PIXMAP, pixmap, 0, 0, w as u16, h as u16, !0)?
            .reply()?;
        let digest = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (rect.x, rect.y, rect.w, rect.h, &image.data).hash(&mut hasher);
            self.background.stamp.hash(&mut hasher);
            hasher.finish()
        };
        if std::mem::replace(&mut at.last, digest) == digest {
            return Ok(None);
        }

        let mut out = self.ground(rect, w, h);
        over(out.as_mut(), &image.data);
        let blurred = image::imageops::fast_blur(&out, SIGMA / SCALE as f32);
        Ok(Some(Frame {
            bgra: blurred.into_raw(),
            width: w,
            height: h,
        }))
    }

    /// The window's scratch pixmap and picture, remade when its size changes.
    fn target(&self, at: &mut Placement, w: u32, h: u32) -> Res<u32> {
        if let Some((_, picture, tw, th)) = at.target {
            if (tw, th) == (w, h) {
                return Ok(picture);
            }
        }
        self.release(at);
        let pixmap = self.conn.generate_id()?;
        self.conn
            .create_pixmap(32, pixmap, self.root, w as u16, h as u16)?;
        let picture = self.conn.generate_id()?;
        self.conn
            .render_create_picture(picture, pixmap, self.argb, &CreatePictureAux::new())?;
        at.target = Some((pixmap, picture, w, h));
        Ok(picture)
    }

    pub(super) fn release(&self, at: &mut Placement) {
        if let Some((pixmap, picture, ..)) = at.target.take() {
            let _ = self.conn.render_free_picture(picture);
            let _ = self.conn.free_pixmap(pixmap);
        }
    }

    /// The wallpaper under `rect`, at the backdrop's scale.
    fn ground(&mut self, rect: Rect, w: u32, h: u32) -> RgbaImage {
        let stamp = wallpaper::settings_stamp();
        if self.background.key.is_none() || stamp != self.background.stamp {
            self.background.stamp = stamp;
            let settings = wallpaper::read_settings();
            let monitors = self.monitors();
            let key = (settings, self.root_size, monitors);
            if self.background.key.as_ref() != Some(&key) {
                let image = wallpaper::load(&key.0);
                self.background.canvas =
                    wallpaper::canvas(&key.0, image.as_ref(), key.1, &key.2, SCALE);
                self.background.key = Some(key);
            }
        }
        let canvas = &self.background.canvas;
        let (x0, y0) = (
            rect.x.div_euclid(SCALE as i32),
            rect.y.div_euclid(SCALE as i32),
        );
        RgbaImage::from_fn(w, h, |x, y| {
            let cx = (x0 + x as i32).clamp(0, canvas.width() as i32 - 1);
            let cy = (y0 + y as i32).clamp(0, canvas.height() as i32 - 1);
            *canvas.get_pixel(cx as u32, cy as u32)
        })
    }

    fn monitors(&self) -> Vec<Rect> {
        let Ok(Ok(reply)) = self
            .conn
            .randr_get_monitors(self.root, true)
            .map(|cookie| cookie.reply())
        else {
            return Vec::new();
        };
        reply
            .monitors
            .iter()
            .map(|m| Rect {
                x: m.x as i32,
                y: m.y as i32,
                w: m.width as u32,
                h: m.height as u32,
            })
            .collect()
    }

    /// Damage reports for every window in `windows`, so a change below one
    /// of ours wakes a refresh, and none for the rest.
    pub(super) fn track(
        &self,
        tracked: &mut std::collections::HashMap<Window, damage::Damage>,
        windows: &std::collections::HashSet<Window>,
    ) -> Res<()> {
        tracked.retain(|window, damage| {
            let keep = windows.contains(window);
            if !keep {
                let _ = self.conn.damage_destroy(*damage);
            }
            keep
        });
        for &window in windows {
            if let std::collections::hash_map::Entry::Vacant(slot) = tracked.entry(window) {
                let damage = self.conn.generate_id()?;
                self.conn
                    .damage_create(damage, window, damage::ReportLevel::NON_EMPTY)?;
                slot.insert(damage);
            }
        }
        self.conn.flush()?;
        Ok(())
    }
}

/// Premultiplied BGRA `top` over the opaque `under`, in place.
pub(super) fn over(under: &mut [u8], top: &[u8]) {
    for (dst, src) in under
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(top.as_chunks::<4>().0)
    {
        let keep = 255 - src[3] as u32;
        for channel in 0..3 {
            dst[channel] =
                (src[channel] as u32 + (dst[channel] as u32 * keep + 127) / 255).min(255) as u8;
        }
        dst[3] = 255;
    }
}
