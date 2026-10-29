//! The renderer's half of the pseudo-localization preview (see
//! `scene::gui::pseudo`): the trees it draws in place of the planned ones,
//! and the mark on every text object whose longer text no longer fits,
//! measured by the very typesetter that draws it.

use rbx_dom::Ref;

use super::text::Typesetter;
use crate::scene::{gui_text_overflows, GuiElement, GuiJoin, GuiScreen, GuiStroke, PseudoLocale};

/// The mark's colour, linear like every colour a plan holds: a warm red
/// (sRGB 255, 72, 56) that reads on a light or a dark GUI.
const MARK: [f32; 3] = [1.0, 0.065, 0.04];
/// Drawn inside the box, so a `ClipsDescendants` parent never hides it.
const MARK_BAND: [f32; 2] = [-2.0, 0.0];

/// `planned` as drawn: as it is, or pseudo-localized.
pub(super) fn localized(planned: &[GuiScreen], locale: Option<PseudoLocale>) -> Vec<GuiScreen> {
    match locale {
        Some(locale) => planned
            .iter()
            .map(|screen| screen.pseudo_localized(locale))
            .collect(),
        None => planned.to_vec(),
    }
}

/// Marks every element whose text overflows its box and returns them, in
/// paint order.
pub(super) fn mark_overflows(elements: &mut [GuiElement], fonts: &mut Typesetter) -> Vec<Ref> {
    let mut marked = Vec::new();
    for element in elements {
        let Some(typeset) = &element.text else {
            continue;
        };
        if !gui_text_overflows(typeset, &element.rect, fonts) {
            continue;
        }
        element.strokes.push(GuiStroke {
            color: MARK,
            alpha: 1.0,
            band: MARK_BAND,
            join: GuiJoin::Miter,
            on_text: false,
            thickness: -MARK_BAND[0],
            scaled: false,
        });
        if !marked.contains(&element.referent) {
            marked.push(element.referent);
        }
    }
    marked
}
