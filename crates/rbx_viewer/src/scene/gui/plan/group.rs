//! Reads a `CanvasGroup`'s tint: `GroupColor3` and `GroupTransparency`,
//! which the docs apply "to the rendered result" of the whole subtree rather
//! than to each child — a distinction only the renderer can honour, by
//! flattening the subtree first (see `renderer::gui::group`).

use std::collections::BTreeMap;

use rbx_dom::Variant;

use super::props::{alpha, color};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Group {
    pub(crate) color: [f32; 3],
    /// `1 - GroupTransparency`.
    pub(crate) alpha: f32,
}

impl Group {
    /// Whether the tint changes anything: a group at the defaults draws
    /// exactly like a `Frame`, so it is never worth a flattening pass.
    pub(crate) fn is_default(&self) -> bool {
        self.alpha >= 1.0 && self.color == [1.0, 1.0, 1.0]
    }
}

pub(in crate::scene::gui) fn group(properties: &BTreeMap<String, Variant>) -> Group {
    Group {
        color: color(properties, "GroupColor3", [1.0, 1.0, 1.0]),
        alpha: alpha(properties, "GroupTransparency"),
    }
}
