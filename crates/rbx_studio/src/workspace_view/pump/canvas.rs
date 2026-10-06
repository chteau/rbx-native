//! The UI editor's 2D canvas, drawn by the same render thread and viewer as
//! the 3D view: one `ScreenGui` alone, at a simulated screen size, with no
//! scene pass (see `rbx_viewer::Headless::render_gui`).
//!
//! Unlike the 3D view, which draws every tick so animated content keeps
//! moving, a canvas is only redrawn when something it could show has
//! changed — a new request, an edit, an asset landing — since a still GUI
//! tree is a still picture.

use rbx_dom::Ref;
use rbx_viewer::{GuiBox, Headless};

/// The flat ground a canvas is drawn on, in encoded sRGB: a mid grey, light
/// enough that a black frame reads against it and dark enough that a white
/// one does, which is the one thing a GUI backdrop has to get right.
const BACKDROP: [f32; 3] = [0.24, 0.24, 0.25];

/// Which screen to draw, and at what simulated resolution — and, while the
/// UI editor's frame sheet is open, the one `ViewportFrame` it shows larger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) screen: Ref,
    pub(crate) size: (u32, u32),
    pub(crate) frame: Option<FrameRequest>,
}

/// One `ViewportFrame` drawn alone (see
/// `rbx_viewer::Headless::render_viewport_frame`): at `size`, over its own
/// background or not, and over `backdrop` (encoded sRGB, 0–255).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameRequest {
    pub(crate) frame: Ref,
    pub(crate) size: (u32, u32),
    pub(crate) background: bool,
    pub(crate) backdrop: [u8; 3],
}

/// A drawn canvas, as the UI thread receives it.
pub(crate) struct Drawn {
    pub(crate) request: Request,
    /// What was drawn: `request.size`, or a `BillboardGui`/`SurfaceGui`'s
    /// own canvas size.
    pub(crate) size: (u32, u32),
    pub(crate) pixels: Vec<u8>,
    pub(crate) boxes: Vec<GuiBox>,
    /// The frame drawn alone, when one was asked for and could be.
    pub(crate) frame: Option<(FrameRequest, Vec<u8>)>,
}

/// What the render loop keeps between ticks: the standing request, and
/// whether anything has happened since it was last drawn.
#[derive(Default)]
pub(super) struct Canvas {
    request: Option<Request>,
    dirty: bool,
}

impl Canvas {
    /// A new request, or `None` once the canvas is off screen. Redraws only
    /// when it differs: the UI thread re-sends the same one every frame.
    pub(super) fn set(&mut self, request: Option<Request>) {
        if request != self.request {
            self.request = request;
            self.dirty = true;
        }
    }

    /// Something that may show on the canvas changed: an edit, an asset.
    pub(super) fn touch(&mut self) {
        self.dirty = true;
    }

    /// Draws the canvas if it is owed a frame.
    pub(super) fn draw(&mut self, viewer: &mut Headless) -> Option<Drawn> {
        let request = self.request.filter(|_| self.dirty)?;
        self.dirty = false;
        match viewer.render_gui(request.screen, request.size, BACKDROP) {
            // After the canvas: its layout is what plans the frame's tree.
            Ok(canvas) => Some(Drawn {
                request,
                size: canvas.size,
                pixels: canvas.pixels,
                boxes: canvas.boxes,
                frame: request.frame.and_then(|frame| {
                    let backdrop = frame.backdrop.map(|channel| f32::from(channel) / 255.0);
                    viewer
                        .render_viewport_frame(frame.frame, frame.size, backdrop, frame.background)
                        .map_err(|err| eprintln!("rbxstudio: viewport frame: {err}"))
                        .ok()
                        .map(|pixels| (frame, pixels))
                }),
            }),
            Err(err) => {
                eprintln!("rbxstudio: canvas: {err}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(screen: u32) -> Option<Request> {
        Some(Request {
            screen: Ref::new(screen),
            size: (1920, 1080),
            frame: None,
        })
    }

    #[test]
    fn only_a_changed_request_or_a_touch_owes_a_frame() {
        let mut canvas = Canvas::default();
        assert!(!canvas.dirty, "nothing asked for yet");

        canvas.set(request(1));
        assert!(canvas.dirty);
        canvas.dirty = false;
        canvas.set(request(1));
        assert!(!canvas.dirty, "the same request every frame is not news");

        canvas.set(request(2));
        assert!(canvas.dirty);
        canvas.dirty = false;
        canvas.touch();
        assert!(canvas.dirty);
    }
}
