//! The pixel box every resolved element is measured in.

/// A screen-space box in pixels, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl Rect {
    pub(crate) fn size(&self) -> [f32; 2] {
        [self.width, self.height]
    }

    /// This box carried `degrees` clockwise around `pivot`: its centre turns,
    /// its extent does not, which is all an axis-aligned box can say about
    /// a rotation — the element's own turn about that centre is
    /// [`Element::rotation`]'s.
    pub(crate) fn turned(&self, degrees: f32, pivot: [f32; 2]) -> Rect {
        if degrees == 0.0 {
            return *self;
        }
        // Same clockwise convention as the renderer's own rotation, y running
        // down the screen.
        let (sin, cos) = degrees.to_radians().sin_cos();
        let dx = self.x + self.width * 0.5 - pivot[0];
        let dy = self.y + self.height * 0.5 - pivot[1];
        Rect {
            x: pivot[0] + dx * cos - dy * sin - self.width * 0.5,
            y: pivot[1] + dx * sin + dy * cos - self.height * 0.5,
            ..*self
        }
    }

    /// The overlap of two boxes, empty (zero-sized) where they do not meet —
    /// which is what a scissor rect has to become for a child clipped away
    /// entirely.
    pub(crate) fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);

        Rect {
            x,
            y,
            width: (right - x).max(0.0),
            height: (bottom - y).max(0.0),
        }
    }
}
