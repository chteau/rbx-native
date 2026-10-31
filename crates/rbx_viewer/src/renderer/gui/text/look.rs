//! What a host drawing its own copy of a text object's text over a canvas
//! (an editor's field over a `TextBox` being typed in) needs to set it as
//! the canvas does, so taking focus moves no glyph.

use std::sync::Arc;

use super::{em, Typesetter};
use crate::fonts::Face;
use crate::scene::{GuiAlign, GuiRect, GuiTypeset};

/// A text object as the last canvas laid it out, short of its text.
#[derive(Debug, Clone)]
pub(in crate::renderer::gui) struct Spot {
    area: GuiRect,
    face: Face,
    size: f32,
    line_height: f32,
    align: [GuiAlign; 2],
    wrapped: bool,
}

impl Spot {
    pub(in crate::renderer::gui) fn of(typeset: &GuiTypeset, rect: &GuiRect) -> Spot {
        let text = &typeset.text;
        Spot {
            area: typeset.area(rect),
            face: text.face.clone(),
            size: typeset.size,
            line_height: text.line_height,
            align: [text.x_align, text.y_align],
            wrapped: text.wrapped || text.scaled,
        }
    }
}

/// How a text object sets its text.
#[derive(Debug, Clone, PartialEq)]
pub struct GuiTextLook {
    /// `x, y, width, height`: the element's box less its `UIPadding`, where
    /// the lines lie.
    pub area: [f32; 4],
    /// The face's family as its own `name` table gives it, which is what a
    /// text system matches a registered face on. Empty while the face has
    /// not landed (the canvas then shapes in a system sans-serif).
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    /// The face file, to register with that text system.
    pub face: Option<Arc<Vec<u8>>>,
    /// The em in pixels, what a text system calls the font size.
    pub em: f32,
    /// One line's height in pixels.
    pub line: f32,
    /// `TextXAlignment` and `TextYAlignment` as how far across the area a
    /// line sits: 0 left or top, 0.5 centred, 1 right or bottom.
    pub align: [f32; 2],
    /// Whether lines wrap at the area's width.
    pub wrapped: bool,
}

impl Typesetter {
    pub(in crate::renderer::gui) fn look(&self, spot: &Spot) -> GuiTextLook {
        let landed = self
            .families
            .get(&spot.face.family)
            .and_then(|family| Some((family, family.closest(spot.face.weight, spot.face.italic)?)));
        let fraction = |align: GuiAlign| match align {
            GuiAlign::Start => 0.0,
            GuiAlign::Center => 0.5,
            GuiAlign::End => 1.0,
        };
        let line = em::line_box(&spot.face, spot.size);
        GuiTextLook {
            area: [spot.area.x, spot.area.y, spot.area.width, spot.area.height],
            family: landed
                .map(|(family, _)| family.name.clone())
                .unwrap_or_default(),
            weight: landed.map_or(spot.face.weight, |(_, entry)| entry.weight),
            italic: landed.map_or(spot.face.italic, |(_, entry)| entry.italic),
            face: landed.and_then(|(_, entry)| self.bytes.get(&entry.asset).cloned()),
            em: em::em_px(line, em::line_em(&self.families, &self.ems, &spot.face)),
            line: line * spot.line_height,
            align: spot.align.map(fraction),
            wrapped: spot.wrapped,
        }
    }
}
