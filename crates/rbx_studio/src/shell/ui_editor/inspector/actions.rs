//! The inspector's one-click edits — everything that is not a number: show
//! and hide, clip, a quarter turn, the anchor grid, the auto layout flow
//! and its alignment, the aspect lock, and a fill, a stroke, a gradient or
//! a constraint put on or taken off.

use gpui_kit::*;
use rbx_dom::{Ref, Variant};

use super::super::tree::Writes;
use super::Shell;
use crate::ui_canvas::{box_of, shifted_in, udim2_text};

/// How an element lays its children out: by hand, or by a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Flow {
    Free,
    Column,
    Row,
    Grid,
}

const LIST: &str = "UIListLayout";
const GRID: &str = "UIGridLayout";
pub(super) const ASPECT: &str = "UIAspectRatioConstraint";

/// `HorizontalAlignment` and `VerticalAlignment` item names, left to right
/// and top to bottom — the auto layout grid's columns and rows.
const ACROSS: [&str; 3] = ["Left", "Center", "Right"];
const DOWN: [&str; 3] = ["Top", "Center", "Bottom"];

/// What a modifier the inspector makes starts as, over its class defaults:
/// a layout sorts by `LayoutOrder`, the order a designer drags into, and
/// a gradient starts as a visible one.
pub(super) fn seed(class: &str) -> impl Iterator<Item = (&'static str, String)> {
    let seeds: &[(&'static str, &str)] = match class {
        LIST => &[("SortOrder", "LayoutOrder"), ("Padding", "0, 8")],
        GRID => &[("SortOrder", "LayoutOrder")],
        "UIGradient" => &[("Color", "0, 1, 1, 1; 1, 0, 0, 0")],
        "UIStroke" => &[("Color", "0, 0, 0"), ("Thickness", "1")],
        _ => &[],
    };
    seeds
        .iter()
        .map(|&(property, text)| (property, text.to_owned()))
}

impl Shell {
    fn write_all(&mut self, writes: Writes, cx: &mut Context<Self>) {
        if !writes.is_empty() {
            self.write_drag(true, &writes, cx);
        }
    }

    fn enum_name(&self, referent: Ref, property: &str, enum_name: &str) -> Option<String> {
        let instance = self.dom.get(referent)?;
        match self.database.stored_or_default(instance, property)? {
            (_, Variant::Enum(value)) => self
                .database
                .enum_name(enum_name, *value)
                .map(str::to_owned),
            _ => None,
        }
    }

    /// Whether `property` is on for every inspected element — `None` where
    /// they disagree.
    pub(super) fn flag(&self, property: &str) -> Option<bool> {
        let mut flags = self.inspected().into_iter().map(|r| {
            self.dom.get(r).is_some_and(|instance| {
                matches!(
                    self.database.stored_or_default(instance, property),
                    Some((_, Variant::Bool(true)))
                )
            })
        });
        let first = flags.next()?;
        flags.all(|flag| flag == first).then_some(first)
    }

    /// Turns `property` on for all, or off where it already is on for all.
    pub(super) fn toggle_flag(&mut self, property: &'static str, cx: &mut Context<Self>) {
        let on = self.flag(property) != Some(true);
        let writes = self
            .inspected()
            .into_iter()
            .map(|r| (r, property, on.to_string()))
            .collect();
        self.write_all(writes, cx);
    }

    /// A quarter turn clockwise for each, kept within one turn.
    pub(super) fn quarter_turn(&mut self, cx: &mut Context<Self>) {
        let writes = self
            .inspected()
            .into_iter()
            .filter_map(|r| {
                let degrees = self.numbers(r, "Rotation")?.first().copied()?;
                Some((
                    r,
                    "Rotation",
                    format!("{}", (degrees + 90.0).rem_euclid(360.0)),
                ))
            })
            .collect();
        self.write_all(writes, cx);
    }

    /// The first inspected element's `AnchorPoint`.
    pub(super) fn anchor(&self) -> Option<[f32; 2]> {
        let numbers = self.numbers(*self.inspected().first()?, "AnchorPoint")?;
        Some([*numbers.first()?, *numbers.get(1)?])
    }

    /// Moves each element's `AnchorPoint` to `anchor` without moving it on
    /// screen: its `Position` takes up the difference, in the bar's unit.
    pub(super) fn set_anchor(&mut self, anchor: [f32; 2], cx: &mut Context<Self>) {
        let Some((root, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let mut writes = Writes::new();
        for h in self.held_selection(&root, &boxes) {
            let size = [h.rect.w, h.rect.h];
            let travel = [0, 1].map(|axis| (anchor[axis] - h.anchor[axis]) * size[axis]);
            let position = shifted_in(h.position, travel, self.ui.unit, h.position_span());
            writes.push((
                h.referent,
                "AnchorPoint",
                format!("{}, {}", anchor[0], anchor[1]),
            ));
            writes.push((h.referent, "Position", udim2_text(position)));
        }
        self.write_all(writes, cx);
    }

    /// How the first inspected element lays its children out.
    pub(super) fn flow(&self) -> Flow {
        if self.anchor_child(GRID).is_some() {
            return Flow::Grid;
        }
        match self.anchor_child(LIST) {
            Some(list) => match self
                .enum_name(list, "FillDirection", "FillDirection")
                .as_deref()
            {
                Some("Horizontal") => Flow::Row,
                _ => Flow::Column,
            },
            None => Flow::Free,
        }
    }

    /// Lays every inspected element's children out as `flow` says: the
    /// layout it needs made or re-aimed, any other taken off.
    pub(super) fn set_flow(&mut self, flow: Flow, cx: &mut Context<Self>) {
        let wanted = match flow {
            Flow::Free => None,
            Flow::Column | Flow::Row => Some(LIST),
            Flow::Grid => Some(GRID),
        };
        let direction = match flow {
            Flow::Row => "Horizontal",
            _ => "Vertical",
        };
        let plan: Vec<(Ref, Vec<Ref>, Option<Ref>)> = self
            .inspected()
            .into_iter()
            .map(|element| {
                let doomed = [LIST, GRID]
                    .into_iter()
                    .filter(|&class| Some(class) != wanted)
                    .filter_map(|class| self.child_of(element, class))
                    .collect();
                (
                    element,
                    doomed,
                    wanted.and_then(|class| self.child_of(element, class)),
                )
            })
            .collect();
        self.edit_gui_tree(
            "auto layout",
            |dom, _| {
                let mut writes = Writes::new();
                for (element, doomed, kept) in plan {
                    for layout in doomed {
                        dom.remove(layout);
                    }
                    let Some(class) = wanted else {
                        continue;
                    };
                    let layout = kept.unwrap_or_else(|| {
                        let made = dom.new_instance(class, class, Some(element));
                        writes.extend(seed(class).map(|(p, t)| (made, p, t)));
                        made
                    });
                    writes.push((layout, "FillDirection", direction.to_owned()));
                }
                (writes, None)
            },
            cx,
        );
    }

    /// Where the first inspected element's layout puts its children, as a
    /// cell of the three-by-three alignment grid.
    pub(super) fn layout_align(&self) -> Option<[usize; 2]> {
        let layout = self
            .anchor_child(LIST)
            .or_else(|| self.anchor_child(GRID))?;
        let across = self.enum_name(layout, "HorizontalAlignment", "HorizontalAlignment")?;
        let down = self.enum_name(layout, "VerticalAlignment", "VerticalAlignment")?;
        Some([
            ACROSS.iter().position(|name| *name == across)?,
            DOWN.iter().position(|name| *name == down)?,
        ])
    }

    pub(super) fn set_layout_align(&mut self, cell: [usize; 2], cx: &mut Context<Self>) {
        let mut writes = Writes::new();
        for element in self.inspected() {
            if let Some(layout) = self
                .child_of(element, LIST)
                .or_else(|| self.child_of(element, GRID))
            {
                writes.push((layout, "HorizontalAlignment", ACROSS[cell[0]].to_owned()));
                writes.push((layout, "VerticalAlignment", DOWN[cell[1]].to_owned()));
            }
        }
        self.write_all(writes, cx);
    }

    /// Locks each element to the shape it has, with a
    /// `UIAspectRatioConstraint`; or, when the first already is, frees all.
    pub(super) fn toggle_aspect(&mut self, cx: &mut Context<Self>) {
        if self.anchor_child(ASPECT).is_some() {
            self.remove_modifier(ASPECT, cx);
            return;
        }
        let boxes = self
            .canvas_boxes(cx)
            .map(|(_, boxes)| boxes)
            .unwrap_or_default();
        let ratios: Vec<(Ref, f32)> = self
            .inspected()
            .into_iter()
            .filter(|&element| self.child_of(element, ASPECT).is_none())
            .filter_map(|element| {
                let placed = box_of(&boxes, element)?;
                (placed.rect[3] > 0.0).then(|| (element, placed.rect[2] / placed.rect[3]))
            })
            .collect();
        self.edit_gui_tree(
            ASPECT,
            |dom, _| {
                let writes = ratios
                    .into_iter()
                    .map(|(element, ratio)| {
                        let made = dom.new_instance(ASPECT, ASPECT, Some(element));
                        (made, "AspectRatio", ratio.to_string())
                    })
                    .collect();
                (writes, None)
            },
            cx,
        );
    }

    /// Puts a seeded `class` under every inspected element that has none.
    pub(super) fn add_modifier(&mut self, class: &'static str, cx: &mut Context<Self>) {
        let bare: Vec<Ref> = self
            .inspected()
            .into_iter()
            .filter(|&element| self.child_of(element, class).is_none())
            .collect();
        if bare.is_empty() {
            return;
        }
        self.edit_gui_tree(
            class,
            |dom, _| {
                let mut writes = Writes::new();
                for element in bare {
                    let made = dom.new_instance(class, class, Some(element));
                    writes.extend(seed(class).map(|(p, t)| (made, p, t)));
                }
                (writes, None)
            },
            cx,
        );
    }

    /// Takes every inspected element's `class` off.
    pub(super) fn remove_modifier(&mut self, class: &'static str, cx: &mut Context<Self>) {
        let doomed: Vec<Ref> = self
            .inspected()
            .into_iter()
            .filter_map(|element| self.child_of(element, class))
            .collect();
        if doomed.is_empty() {
            return;
        }
        self.edit_gui_tree(
            class,
            |dom, _| {
                for modifier in doomed {
                    dom.remove(modifier);
                }
                (Writes::new(), None)
            },
            cx,
        );
    }

    /// A fill on (opaque) or off (clear) for all.
    pub(super) fn set_fill(&mut self, on: bool, cx: &mut Context<Self>) {
        let text = if on { "0" } else { "1" };
        let writes = self
            .inspected()
            .into_iter()
            .map(|r| (r, "BackgroundTransparency", text.to_owned()))
            .collect();
        self.write_all(writes, cx);
    }

    /// Selects the first inspected element's `class`, for the rows the
    /// inspector does not break out.
    pub(super) fn select_modifier(&mut self, class: &str, cx: &mut Context<Self>) {
        if let Some(modifier) = self.anchor_child(class) {
            self.select(modifier, cx);
        }
    }
}
