//! Drawing an element onto the canvas, Figma's way: arm a tool from the
//! insert bar or its key, then drag its box out — or click, for the class's
//! own size centred on the pointer. It lands in the topmost container under
//! where the drag began, placed in the unit the bar's switch names, and is
//! one undo step together with its first values.

use gpui_kit::*;
use rbx_dom::Ref;

use super::Shell;
use crate::explorer::insert::{gui_defaults, incremented_name};
use crate::ui_canvas::arrange::{place, Member};
use crate::ui_canvas::{covers, rotate, udim2_text, Rect, Unit};

/// What a drawn element nests in: the classes a designer builds with.
const CONTAINERS: [&str; 3] = ["Frame", "ScrollingFrame", "CanvasGroup"];

/// The tools' keys — the set Roblox's Figma-style editors settled on, F and
/// T being Figma's own.
pub(super) const TOOL_KEYS: [(&str, &str); 6] = [
    ("f", "Frame"),
    ("t", "TextLabel"),
    ("b", "TextButton"),
    ("x", "TextBox"),
    ("l", "ImageLabel"),
    ("g", "ImageButton"),
];

impl Shell {
    /// Arms `class` for the next press on the canvas, or disarms it when
    /// it already is.
    pub(super) fn arm_tool(&mut self, class: &'static str, cx: &mut Context<Self>) {
        self.ui.tool = match self.ui.tool == Some(class) {
            true => None,
            false => Some(class),
        };
        cx.notify();
    }

    /// Puts down the element a draw from `from` to `to` (canvas pixels)
    /// made, or — with no `to`, a click — one of its class's own size.
    pub(super) fn finish_draw(
        &mut self,
        class: &'static str,
        from: [f32; 2],
        to: Option<[f32; 2]>,
        cx: &mut Context<Self>,
    ) {
        self.ui.tool = None;
        let Some((root, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let container = boxes
            .iter()
            .rev()
            .filter(|placed| covers(placed, from))
            .find(|placed| {
                self.dom.get(placed.referent).is_some_and(|instance| {
                    CONTAINERS
                        .iter()
                        .any(|base| self.database.is_subclass_of(instance.class(), base))
                })
            })
            .copied()
            .unwrap_or(root);
        let Some(content) = container.content.map(Rect::from_array) else {
            return;
        };
        let rect = match to {
            Some(to) => Rect::spanning(from, to),
            None => {
                let [w, h] = default_size(&self.database, class);
                Rect {
                    x: from[0] - w * 0.5,
                    y: from[1] - h * 0.5,
                    w,
                    h,
                }
            }
        };
        // Into the container's own frame, where its content box is square.
        let pivot = Rect::of(&container).centre();
        let centre = rect.centre();
        let [x, y] = rotate(
            [centre[0] - pivot[0], centre[1] - pivot[1]],
            -container.rotation,
        );
        let member = Member {
            rect: Rect {
                x: pivot[0] + x - rect.w * 0.5,
                y: pivot[1] + y - rect.h * 0.5,
                ..rect
            },
            anchor: [0.0; 2],
            position: [(0.0, 0); 2],
            size: [(0.0, 0); 2],
            size_scale: 1.0,
            size_axes: [0, 1],
        };
        let scaled = [[self.ui.unit == Unit::Scale; 2]; 2];
        let (position, size) = place(
            &member,
            [content.x, content.y],
            [content.w, content.h],
            scaled,
        );
        self.insert_gui_element(
            class,
            container.referent,
            vec![
                ("Position", udim2_text(position)),
                ("Size", udim2_text(size)),
            ],
            cx,
        );
    }

    /// Inserts `class` under `parent` with the Explorer insert's own seed
    /// (`explorer::insert::gui_defaults`), then `values` over it, selected.
    pub(super) fn insert_gui_element(
        &mut self,
        class: &str,
        parent: Ref,
        values: Vec<(&'static str, String)>,
        cx: &mut Context<Self>,
    ) {
        let name = match self.increment_names() {
            true => incremented_name(&self.dom, Some(parent), class),
            false => class.to_owned(),
        };
        self.edit_gui_tree(
            "insert",
            |dom, database| {
                let new = dom.new_instance(class, &name, Some(parent));
                let seed = gui_defaults(database, class)
                    .into_iter()
                    .map(|(property, text)| (property, text.to_owned()));
                let writes = seed
                    .chain(values)
                    .map(|(property, text)| (new, property, text))
                    .collect();
                (writes, Some(vec![new]))
            },
            cx,
        );
    }
}

/// The size a click puts a `class` down at: the offsets of its seeded
/// `Size`.
fn default_size(database: &rbx_reflection::ReflectionDatabase, class: &str) -> [f32; 2] {
    let offsets: Vec<f32> = gui_defaults(database, class)
        .into_iter()
        .find(|(property, _)| *property == "Size")
        .map(|(_, text)| {
            text.split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default();
    match offsets.as_slice() {
        [_, w, _, h] => [*w, *h],
        _ => [100.0, 100.0],
    }
}
