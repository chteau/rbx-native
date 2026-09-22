//! The UI editor canvas's geometry, apart from any GPUI state: which element
//! a point lands on, what a marquee takes, and how a drag on a handle turns
//! into the `Position`/`Size`/`Rotation` writes Roblox's own `UDim2` model
//! needs. Everything is in *canvas pixels* — the simulated screen the
//! `ScreenGui` is laid out against — and every element box is the one the
//! renderer laid out (see `rbx_viewer::GuiBox`), so the canvas never
//! re-derives a layout of its own.
//!
//! [`guides`] is the snapping and measuring, and [`arrange`] the align,
//! distribute and group geometry.

pub(crate) mod arrange;
pub(crate) mod carry;
pub(crate) mod guides;

use rbx_dom::Ref;
use rbx_viewer::GuiBox;

/// The simulated screens the canvas offers — the conventional widths a
/// Figma frame is stress-tested against, in Roblox's terms: a whole
/// resolution per preset, since a `UDim2` resolves against both axes.
pub(crate) const PRESETS: [(&str, u32, u32); 6] = [
    ("Desktop 1920×1080", 1920, 1080),
    ("Laptop 1366×768", 1366, 768),
    ("Tablet 768×1024", 768, 1024),
    ("Phone landscape 844×390", 844, 390),
    ("Phone portrait 390×844", 390, 844),
    ("Small phone 320×568", 320, 568),
];

/// How far the canvas zooms either way.
const ZOOM_RANGE: (f32, f32) = (0.05, 8.0);

/// Where the canvas sits in its panel: canvas pixel `p` shows at
/// `pan + p * zoom`, in the panel's own logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct View {
    pub(crate) zoom: f32,
    pub(crate) pan: [f32; 2],
}

impl View {
    /// The whole `screen` centred in `panel` with `margin` round it — shrunk
    /// to fit, but never blown up past 1:1, where the picture (drawn at the
    /// screen's own resolution) would only go soft.
    pub(crate) fn fit(panel: [f32; 2], screen: [f32; 2], margin: f32) -> View {
        let room = [0, 1].map(|axis| (panel[axis] - margin * 2.0).max(1.0) / screen[axis].max(1.0));
        let zoom = room[0].min(room[1]).clamp(ZOOM_RANGE.0, 1.0);
        View {
            zoom,
            pan: [0, 1].map(|axis| (panel[axis] - screen[axis] * zoom) * 0.5),
        }
    }

    /// `rect` centred in `panel` with `margin` round it, zoomed as far in
    /// as that takes — zoom to selection, which unlike [`View::fit`] goes
    /// past 1:1: a small button is what it is asked to show.
    pub(crate) fn framing(panel: [f32; 2], rect: &Rect, margin: f32) -> View {
        let size = [rect.w, rect.h];
        let room = [0, 1].map(|axis| (panel[axis] - margin * 2.0).max(1.0) / size[axis].max(1.0));
        let zoom = room[0].min(room[1]).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let centre = rect.centre();
        View {
            zoom,
            pan: [0, 1].map(|axis| panel[axis] * 0.5 - centre[axis] * zoom),
        }
    }

    pub(crate) fn to_view(self, p: [f32; 2]) -> [f32; 2] {
        [0, 1].map(|axis| self.pan[axis] + p[axis] * self.zoom)
    }

    pub(crate) fn to_canvas(self, p: [f32; 2]) -> [f32; 2] {
        [0, 1].map(|axis| (p[axis] - self.pan[axis]) / self.zoom)
    }

    /// Zoomed by `factor` about `at` (panel pixels), which stays put under
    /// the pointer the way every canvas editor zooms.
    pub(crate) fn zoomed(self, factor: f32, at: [f32; 2]) -> View {
        let zoom = (self.zoom * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let kept = zoom / self.zoom;
        View {
            zoom,
            pan: [0, 1].map(|axis| at[axis] - (at[axis] - self.pan[axis]) * kept),
        }
    }
}

/// An axis-aligned rectangle in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Rect {
    pub(crate) fn of(placed: &GuiBox) -> Rect {
        Rect::from_array(placed.rect)
    }

    /// `x, y, width, height`, the way `rbx_viewer::GuiBox` spells a box.
    pub(crate) fn from_array([x, y, w, h]: [f32; 4]) -> Rect {
        Rect { x, y, w, h }
    }

    /// The box through two corners, either way round.
    pub(crate) fn spanning(a: [f32; 2], b: [f32; 2]) -> Rect {
        Rect {
            x: a[0].min(b[0]),
            y: a[1].min(b[1]),
            w: (a[0] - b[0]).abs(),
            h: (a[1] - b[1]).abs(),
        }
    }

    pub(crate) fn centre(&self) -> [f32; 2] {
        [self.x + self.w * 0.5, self.y + self.h * 0.5]
    }

    /// Min, centre and max along `axis` (0 across, 1 down).
    pub(crate) fn lines(&self, axis: usize) -> [f32; 3] {
        let (start, length) = self.along(axis);
        [start, start + length * 0.5, start + length]
    }

    pub(crate) fn along(&self, axis: usize) -> (f32, f32) {
        match axis {
            0 => (self.x, self.w),
            _ => (self.y, self.h),
        }
    }

    pub(crate) fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect {
            x,
            y,
            w: (self.x + self.w).max(other.x + other.w) - x,
            h: (self.y + self.h).max(other.y + other.h) - y,
        }
    }

    pub(crate) fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.w <= self.x + self.w
            && other.y + other.h <= self.y + self.h
    }

    pub(crate) fn shifted(&self, by: [f32; 2]) -> Rect {
        Rect {
            x: self.x + by[0],
            y: self.y + by[1],
            ..*self
        }
    }

    /// The four corners after turning `degrees` clockwise about the centre,
    /// top-left first — what the renderer draws for a rotated element.
    pub(crate) fn corners(&self, degrees: f32) -> [[f32; 2]; 4] {
        let c = self.centre();
        let (hw, hh) = (self.w * 0.5, self.h * 0.5);
        [[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]].map(|p| {
            let [x, y] = rotate(p, degrees);
            [c[0] + x, c[1] + y]
        })
    }

    /// The axis-aligned box around [`Rect::corners`].
    pub(crate) fn turned_bounds(&self, degrees: f32) -> Rect {
        let corners = self.corners(degrees);
        let min = corners
            .iter()
            .fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
        let max = corners
            .iter()
            .fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
        Rect::spanning(min, max)
    }
}

/// `point` turned `degrees` clockwise about the origin — clockwise on a
/// screen whose y runs down, the way `GuiObject.Rotation` turns.
pub(crate) fn rotate([x, y]: [f32; 2], degrees: f32) -> [f32; 2] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [x * cos - y * sin, x * sin + y * cos]
}

/// Whether `point` falls inside `placed` as drawn, rotation included.
pub(crate) fn covers(placed: &GuiBox, point: [f32; 2]) -> bool {
    covers_turned(&Rect::of(placed), placed.rotation, point)
}

/// Whether `point` falls inside `rect` turned `degrees` about its centre.
pub(crate) fn covers_turned(rect: &Rect, degrees: f32, point: [f32; 2]) -> bool {
    let c = rect.centre();
    let [x, y] = rotate([point[0] - c[0], point[1] - c[1]], -degrees);
    x.abs() <= rect.w * 0.5 && y.abs() <= rect.h * 0.5
}

/// The topmost element under `point`: the last in paint order that covers
/// it, which is what a click on the screen itself would land on.
pub(crate) fn hit(boxes: &[GuiBox], point: [f32; 2]) -> Option<Ref> {
    boxes
        .iter()
        .rev()
        .find(|placed| covers(placed, point))
        .map(|placed| placed.referent)
}

/// The element's own box: the first one laid out for it (a `ScrollingFrame`'s
/// bar segments come after its own).
pub(crate) fn box_of(boxes: &[GuiBox], referent: Ref) -> Option<&GuiBox> {
    boxes.iter().find(|placed| placed.referent == referent)
}

/// What a marquee over `area` selects: every element drawn wholly inside it
/// whose parent is not also taken — the outermost of a nested run, so a
/// marquee round a card takes the card rather than every label on it.
pub(crate) fn marquee(
    boxes: &[GuiBox],
    area: Rect,
    parent_of: impl Fn(Ref) -> Option<Ref>,
) -> Vec<Ref> {
    let mut inside: Vec<Ref> = Vec::new();
    for placed in boxes {
        let bounds = Rect::of(placed).turned_bounds(placed.rotation);
        if area.contains(&bounds) && !inside.contains(&placed.referent) {
            inside.push(placed.referent);
        }
    }
    inside
        .iter()
        .copied()
        .filter(|&referent| {
            let mut ancestor = parent_of(referent);
            while let Some(up) = ancestor {
                if inside.contains(&up) {
                    return false;
                }
                ancestor = parent_of(up);
            }
            true
        })
        .collect()
}

/// A `UDim2`'s two `(scale, offset)` pairs, as the DOM holds one.
pub(crate) type Udim2 = [(f32, i32); 2];

/// Which of the eight resize handles: -1, 0 or 1 across and down, the
/// side of the box it sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Handle(pub(crate) i8, pub(crate) i8);

impl Handle {
    pub(crate) const ALL: [Handle; 8] = [
        Handle(-1, -1),
        Handle(0, -1),
        Handle(1, -1),
        Handle(1, 0),
        Handle(1, 1),
        Handle(0, 1),
        Handle(-1, 1),
        Handle(-1, 0),
    ];

    /// Where it sits on `rect` turned `degrees`, in canvas pixels.
    pub(crate) fn at(self, rect: &Rect, degrees: f32) -> [f32; 2] {
        let c = rect.centre();
        let [x, y] = rotate(
            [
                f32::from(self.0) * rect.w * 0.5,
                f32::from(self.1) * rect.h * 0.5,
            ],
            degrees,
        );
        [c[0] + x, c[1] + y]
    }

    fn is_corner(self) -> bool {
        self.0 != 0 && self.1 != 0
    }
}

/// What one resize step changes, in the element's own (unturned) frame:
/// how much bigger it gets, and how far its centre moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Resize {
    pub(crate) grow: [f32; 2],
    pub(crate) centre: [f32; 2],
}

/// Dragging `handle` of a `size` box by `local` (the pointer's travel, turned
/// into the element's own frame): the grabbed sides follow, the opposite
/// ones hold still — never past each other, a box is never turned inside
/// out. `keep_aspect` on a corner scales both sides by one factor, the
/// larger of the two the pointer asks for.
pub(crate) fn resize(handle: Handle, size: [f32; 2], local: [f32; 2], keep_aspect: bool) -> Resize {
    let sides = [f32::from(handle.0), f32::from(handle.1)];
    let mut grow = [0, 1].map(|axis| (sides[axis] * local[axis]).max(-size[axis]));
    if keep_aspect && handle.is_corner() && size[0] > 0.0 && size[1] > 0.0 {
        let factors = [0, 1].map(|axis| (size[axis] + grow[axis]) / size[axis]);
        let factor = if (factors[0] - 1.0).abs() >= (factors[1] - 1.0).abs() {
            factors[0]
        } else {
            factors[1]
        };
        grow = [0, 1].map(|axis| size[axis] * factor - size[axis]);
    }
    Resize {
        grow,
        centre: [0, 1].map(|axis| sides[axis] * grow[axis] * 0.5),
    }
}

/// How far an element's `Position` offset moves for its centre to travel
/// `centre` canvas pixels while it grows by `grow` — `Position` is where
/// `AnchorPoint` lands, and it lands in the *parent's* frame, turned by the
/// parent's own absolute rotation.
pub(crate) fn position_shift(
    centre: [f32; 2],
    parent_rotation: f32,
    anchor: [f32; 2],
    grow: [f32; 2],
) -> [f32; 2] {
    let local = rotate(centre, -parent_rotation);
    [0, 1].map(|axis| local[axis] + (anchor[axis] - 0.5) * grow[axis])
}

/// `udim` with `shift` added to each offset, rounded: a `UDim` offset is a
/// whole number of pixels.
pub(crate) fn shifted(udim: Udim2, shift: [f32; 2]) -> Udim2 {
    [0, 1].map(|axis| {
        let (scale, offset) = udim[axis];
        (scale, (offset as f32 + shift[axis]).round() as i32)
    })
}

/// Which half of a `UDim` the canvas writes an edit into: whole pixels, or
/// a share of the parent — the insert bar's Offset/Scale switch. Only the
/// half named moves; the other keeps whatever it held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Unit {
    #[default]
    Offset,
    Scale,
}

/// [`shifted`] in `unit`: the pixels onto the offsets, or onto the scales
/// as the share of `span` — what a scale of 1 comes to, in pixels, on each
/// axis — they are. An axis with no span to divide by takes pixels.
pub(crate) fn shifted_in(udim: Udim2, shift: [f32; 2], unit: Unit, span: [f32; 2]) -> Udim2 {
    let pixels = shifted(udim, shift);
    [0, 1].map(|axis| match unit == Unit::Scale && span[axis] > 0.0 {
        true => (
            round_scale(udim[axis].0 + shift[axis] / span[axis]),
            udim[axis].1,
        ),
        false => pixels[axis],
    })
}

/// A scale to four places: a ten-thousandth of a 1920-pixel screen is a
/// fifth of a pixel, and any more places print noise.
pub(crate) fn round_scale(scale: f32) -> f32 {
    (scale * 10_000.0).round() / 10_000.0
}

/// [`resize`] about the box's centre, Alt's convention in Figma and Sketch:
/// the far sides move as far as the grabbed ones, the other way.
pub(crate) fn centred(step: Resize, size: [f32; 2]) -> Resize {
    Resize {
        grow: [0, 1].map(|axis| (step.grow[axis] * 2.0).max(-size[axis])),
        centre: [0.0; 2],
    }
}

/// The text `properties::edit::commit` reads a `UDim2` back out of.
pub(crate) fn udim2_text(udim: Udim2) -> String {
    let [(sx, ox), (sy, oy)] = udim;
    format!("{sx}, {ox}, {sy}, {oy}")
}

/// The angle of `point` about `centre`, clockwise from the right, in degrees.
pub(crate) fn angle_of(centre: [f32; 2], point: [f32; 2]) -> f32 {
    (point[1] - centre[1])
        .atan2(point[0] - centre[0])
        .to_degrees()
}

#[cfg(test)]
#[path = "ui_canvas/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ui_canvas/unit_tests.rs"]
mod unit_tests;
