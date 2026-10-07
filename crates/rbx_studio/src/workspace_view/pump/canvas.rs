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

/// The canvas comes back transparent where the GUI is, so the editor's dot
/// grid shows through it: drawn once over black and once over white (see
/// [`unblend`]).
const BLACK: [f32; 3] = [0.0; 3];
const WHITE: [f32; 3] = [1.0; 3];

/// Recovers a straight-alpha picture from the same canvas drawn over black
/// (`black`, rewritten in place) and over white. Every GUI blend is
/// `c·a + under·(1−a)` in encoded sRGB (see `rbx_viewer`'s
/// `renderer::gui::pipeline::encoded`), so a whole stack of them is affine
/// in what lies under it: `white − black = 255·(1−a)`, `black = c·a`.
fn unblend(black: &mut [u8], white: &[u8]) {
    for (b, w) in black
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(white.as_chunks::<4>().0)
    {
        let gap: u32 = (0..3).map(|i| u32::from(w[i].saturating_sub(b[i]))).sum();
        let alpha = 255 - (gap / 3).min(255);
        for channel in &mut b[..3] {
            *channel = match alpha {
                0 => 0,
                _ => (u32::from(*channel) * 255 / alpha).min(255) as u8,
            };
        }
        b[3] = alpha as u8;
    }
}

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
        // ponytail: two full draws per change; clearing to transparent with a
        // fixed alpha blend in the GUI pipelines would make it one.
        let drawn = viewer
            .render_gui(request.screen, request.size, WHITE)
            .and_then(|white| {
                let mut black = viewer.render_gui(request.screen, request.size, BLACK)?;
                unblend(&mut black.pixels, &white.pixels);
                // `RBX_STUDIO_CANVAS_DUMP=<path.png>`: the canvas exactly as
                // drawn, straight alpha, for pixel comparisons.
                if let Some(path) = std::env::var_os("RBX_STUDIO_CANVAS_DUMP") {
                    let (width, height) = black.size;
                    let _ = image::save_buffer(
                        path,
                        &black.pixels,
                        width,
                        height,
                        image::ExtendedColorType::Rgba8,
                    );
                }
                Ok(black)
            });
        match drawn {
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
    fn unblending_recovers_coverage_and_straight_colour() {
        // Clear, opaque red, and red at half coverage.
        let mut black = vec![0, 0, 0, 255, 200, 0, 0, 255, 100, 0, 0, 255];
        let white = [255, 255, 255, 255, 200, 0, 0, 255, 228, 128, 128, 255];
        unblend(&mut black, &white);
        assert_eq!(black, [0, 0, 0, 0, 200, 0, 0, 255, 200, 0, 0, 127]);
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
