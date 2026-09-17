//! A `ScrollingFrame`'s window, canvas and scroll bars, as a still frame
//! shows them: the children resolve against the canvas, shifted back by
//! `CanvasPosition` and clipped to the window, and a bar is drawn on each axis
//! the canvas overflows along.
//!
//! The docs describe the pieces — which box each is, when a bar appears,
//! what the three bar images are — but no geometry beyond that. What is
//! decided here, and nowhere in the docs: `CanvasSize`'s scale is a fraction
//! of the *window* (the docs' inset diagram has a 100% canvas meeting the bar
//! "edge-to-edge", which only the window makes true); a thumb is the window's
//! share of the canvas along the track, at the canvas position's share of the
//! slack; a bar's track is the window's span along its axis; the two end caps
//! are `ScrollBarThickness` square, shrinking to share a thumb shorter than
//! two of them; `UIPadding` insets the canvas, as it does any container.

use super::super::plan::{Node, Scrolling};
use super::{children, sizing, Context, Element, ImageScale, Painted, Rect, Scope, TextMeasure};

/// The window the canvas shows through, what the canvas came to, and the
/// bars that came with it — `AbsoluteWindowSize` and `AbsoluteCanvasSize`,
/// in the docs' terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Window {
    pub(super) rect: Rect,
    pub(super) canvas: [f32; 2],
    /// `CanvasPosition` clamped to what the canvas can actually scroll by,
    /// zero along an axis with no bar: "this property doesn't do anything if
    /// scroll bars aren't visible".
    pub(super) position: [f32; 2],
    pub(super) shown: [bool; 2],
}

/// One scroll bar's thumb as three axis-aligned boxes along the track — top
/// (or left) cap, middle, bottom (or right) cap — plus the turn the images
/// take: none along a vertical bar, a quarter counterclockwise along a
/// horizontal one, per the docs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Thumb {
    pub(super) segments: [Rect; 3],
    pub(super) rotation: f32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn scroll(
    node: &Node,
    scrolling: &Scrolling,
    frame: &Rect,
    cells: Option<&[Rect]>,
    context: Context,
    measure: &mut dyn TextMeasure,
    into: &mut Vec<Element>,
) {
    let window = window(node, scrolling, frame, measure);
    let canvas = Rect {
        x: window.rect.x - window.position[0],
        y: window.rect.y - window.position[1],
        width: window.canvas[0],
        height: window.canvas[1],
    };
    children(
        Scope {
            nodes: &node.children,
            groups: &node.groups,
            layout: node.list.as_ref(),
        },
        &sizing::padded(node, &canvas),
        cells,
        Context {
            // Whatever `ClipsDescendants` says, nothing scrolled out of the
            // window can be seen: the docs call the window "the visible
            // content area". A rotated frame keeps the same exemption a
            // rotated clipping frame gets (see `emit`).
            clip: match context.rotated {
                true => context.clip,
                false => Some(
                    context
                        .clip
                        .map_or(window.rect, |outer| outer.intersect(&window.rect)),
                ),
            },
            ..context
        },
        measure,
        into,
    );

    // Over the content, at the frame's own `ZIndex`: a bar is part of the
    // frame, not a child of it.
    for axis in 0..2 {
        let Some(thumb) = thumb(axis, scrolling, frame, &window) else {
            continue;
        };
        let images = &scrolling.images;
        for (segment, asset) in
            thumb
                .segments
                .iter()
                .zip([&images.top, &images.mid, &images.bottom])
        {
            if segment.width <= 0.0 || segment.height <= 0.0 {
                continue;
            }
            // The renderer turns a box about its own centre, so a horizontal
            // segment is handed over standing up, for the quarter turn to
            // lay flat where the segment is.
            let rect = match thumb.rotation == 0.0 {
                true => *segment,
                false => Rect {
                    x: segment.x + (segment.width - segment.height) * 0.5,
                    y: segment.y + (segment.height - segment.width) * 0.5,
                    width: segment.height,
                    height: segment.width,
                },
            };
            into.push(Element {
                viewport: None,
                rect: context.carried(rect),
                clip: context.clip,
                rotation: context.angle + thumb.rotation,
                background: [0.0; 3],
                background_alpha: 0.0,
                border: None,
                border_inset: 0.0,
                z_index: node.z_index,
                image: Some(Painted {
                    asset: asset.clone(),
                    tint: scrolling.bar_color,
                    alpha: scrolling.bar_alpha,
                    repeat: [1.0, 1.0],
                    scale: ImageScale::Stretch,
                    rect_offset: [0.0, 0.0],
                    rect_size: [0.0, 0.0],
                    pixelated: false,
                }),
                corner_radii: [0.0; 4],
                stroke: None,
                gradient: None,
                text: None,
                group: None,
            });
        }
    }
}

/// Settles the window against the frame.
///
/// Whether a bar shows depends on the window, and `ScrollBarInset.ScrollBar`
/// makes the window depend on whether a bar shows; the docs describe no
/// settling, so this runs the dependency twice from the whole frame, which
/// is enough for one bar's inset to reveal the other.
pub(super) fn window(
    node: &Node,
    scrolling: &Scrolling,
    frame: &Rect,
    measure: &mut dyn TextMeasure,
) -> Window {
    let overflow = |rect: Rect, measure: &mut dyn TextMeasure| {
        let canvas = canvas_extent(node, scrolling, rect.size(), measure);
        let shown = [0, 1].map(|axis| {
            scrolling.thickness > 0.0
                && scrolling.direction[axis]
                && canvas[axis] > rect.size()[axis]
        });
        (canvas, shown)
    };

    let mut rect = *frame;
    let (mut canvas, mut shown) = overflow(rect, measure);
    for _ in 0..2 {
        // The vertical bar takes width, the horizontal one height.
        let take_x = match scrolling.vertical_inset.applies(shown[1]) {
            true => scrolling.thickness,
            false => 0.0,
        };
        let take_y = match scrolling.horizontal_inset.applies(shown[0]) {
            true => scrolling.thickness,
            false => 0.0,
        };
        rect = Rect {
            x: frame.x + if scrolling.bar_left { take_x } else { 0.0 },
            y: frame.y,
            width: (frame.width - take_x).max(0.0),
            height: (frame.height - take_y).max(0.0),
        };
        (canvas, shown) = overflow(rect, measure);
    }

    let position = [0, 1].map(|axis| match shown[axis] {
        true => scrolling.canvas_position[axis].clamp(0.0, canvas[axis] - rect.size()[axis]),
        false => 0.0,
    });
    Window {
        rect,
        canvas,
        position,
        shown,
    }
}

/// `AbsoluteCanvasSize`: "the maximum of the `CanvasSize` property and the
/// size of the children if `AutomaticCanvasSize` is set to something other
/// than `None`" — per axis, since the enum names them separately.
fn canvas_extent(
    node: &Node,
    scrolling: &Scrolling,
    window: [f32; 2],
    measure: &mut dyn TextMeasure,
) -> [f32; 2] {
    let mut canvas = scrolling.canvas_size.against(window);
    if scrolling.automatic_canvas != [false, false] {
        let content = sizing::content_extent(node, canvas, scrolling.automatic_canvas, measure);
        for axis in 0..2 {
            if scrolling.automatic_canvas[axis] {
                canvas[axis] = canvas[axis].max(content[axis]);
            }
        }
    }
    canvas.map(|extent| extent.max(0.0))
}

/// The bar along `axis` (0 horizontal, 1 vertical), `None` where none shows.
pub(super) fn thumb(
    axis: usize,
    scrolling: &Scrolling,
    frame: &Rect,
    window: &Window,
) -> Option<Thumb> {
    if !window.shown[axis] {
        return None;
    }
    let thickness = scrolling.thickness;
    let track = window.rect.size()[axis];
    let visible = window.rect.size()[axis] / window.canvas[axis];
    let length = track * visible;
    let slack = window.canvas[axis] - window.rect.size()[axis];
    let offset = (track - length) * window.position[axis] / slack;
    let cap = thickness.min(length * 0.5);
    let (start, mid, end) = (
        [0.0, cap],
        [cap, (length - 2.0 * cap).max(0.0)],
        [length - cap, cap],
    );

    Some(match axis {
        // Horizontal: along the bottom edge, left to right; the images are
        // "rotated 90° counterclockwise for a horizontal scroll bar", which
        // is what a negative turn is in this clockwise-positive space.
        0 => {
            let y = frame.y + frame.height - thickness;
            let x = window.rect.x + offset;
            Thumb {
                segments: [start, mid, end].map(|[along, extent]| Rect {
                    x: x + along,
                    y,
                    width: extent,
                    height: thickness,
                }),
                rotation: -90.0,
            }
        }
        _ => {
            let x = match scrolling.bar_left {
                true => frame.x,
                false => frame.x + frame.width - thickness,
            };
            let y = window.rect.y + offset;
            Thumb {
                segments: [start, mid, end].map(|[along, extent]| Rect {
                    x,
                    y: y + along,
                    width: thickness,
                    height: extent,
                }),
                rotation: 0.0,
            }
        }
    })
}
