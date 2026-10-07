//! Where everything on a node sits, in canvas units (pixels at 100%).
//! Computed here rather than read back from the drawn elements, so a wire
//! can be painted to a pin and a click found on one in the same frame the
//! node is laid out — and so both are testable.
//!
//! A node is a header over rows; row `n` holds input `n` on the left and
//! output `n` on the right. Text widths are estimated from character
//! counts, generously, so a label never runs into the edge.

use super::catalog::{self, Kind, Pin, PinType};
use super::{End, Graph, Group, Node, NodeId};

pub(crate) const HEADER: f32 = 26.0;
pub(crate) const ROW: f32 = 22.0;
const TOP: f32 = 4.0;
const BOTTOM: f32 = 8.0;
const MIN_WIDTH: f32 = 140.0;
/// Average advance of the UI font at the node's 12px.
const CHAR: f32 = 6.6;
/// The monospace literal chips, at 11px.
const MONO: f32 = 6.8;
/// How many characters of a literal a chip shows before an ellipsis.
pub(crate) const CHIP_CHARS: usize = 18;
/// How far from a pin's centre a press still lands on it, at 100%.
pub(crate) const PIN_REACH: f32 = 9.0;

/// Which side of a node a pin is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Rect {
    pub(crate) fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[0] <= self.x + self.w && p[1] >= self.y && p[1] <= self.y + self.h
    }

    pub(crate) fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
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

    pub(crate) fn spanning(a: [f32; 2], b: [f32; 2]) -> Rect {
        Rect {
            x: a[0].min(b[0]),
            y: a[1].min(b[1]),
            w: (a[0] - b[0]).abs(),
            h: (a[1] - b[1]).abs(),
        }
    }
}

/// What a pin's row shows as its name. Get Property's output is named after
/// the property it reads, as the code would.
pub(crate) fn label(graph: &Graph, node: &Node, pin: &Pin, side: Side) -> String {
    if side == Side::Output && node.kind == "get_property" {
        if let Some(name) = graph.value(&End::new(node.id, "Property")) {
            if !name.is_empty() {
                return name;
            }
        }
    }
    pin.name.to_owned()
}

/// The literal an unwired value input shows in its chip, if it has one.
pub(crate) fn chip(graph: &Graph, node: &Node, pin: &Pin) -> Option<String> {
    if pin.ty == PinType::Exec {
        return None;
    }
    let end = End::new(node.id, pin.name);
    if graph.wire_into(&end).is_some() {
        return None;
    }
    let value = graph.value(&end)?;
    Some(match pin.ty {
        PinType::String => super::codegen::quote(&value),
        _ => value,
    })
}

/// A chip's text cut to what it shows.
pub(crate) fn chip_text(text: &str) -> String {
    match text.chars().count() > CHIP_CHARS {
        true => text.chars().take(CHIP_CHARS - 1).chain(['…']).collect(),
        false => text.to_owned(),
    }
}

pub(crate) fn chip_width(text: &str) -> f32 {
    chip_text(text).chars().count() as f32 * MONO + 12.0
}

pub(crate) fn label_width(text: &str) -> f32 {
    text.chars().count() as f32 * CHAR
}

/// Where an input's chip sits on the canvas: right after its label, which
/// is drawn at this same estimate so a click lands where the chip is.
pub(crate) fn chip_rect(graph: &Graph, node: &Node, name: &str) -> Option<Rect> {
    let kind = catalog::kind(&node.kind)?;
    let row = kind.inputs.iter().position(|pin| pin.name == name)?;
    let pin = &kind.inputs[row];
    let text = chip(graph, node, pin)?;
    let label = label(graph, node, pin, Side::Input);
    let gap = if label.is_empty() { 0.0 } else { 6.0 };
    Some(Rect {
        x: node.x + 14.0 + label_width(&label) + gap,
        y: node.y + row_centre(row) - 8.0,
        w: chip_width(&text),
        h: 16.0,
    })
}

/// The input chip under a canvas point, if any.
pub(crate) fn chip_at(graph: &Graph, p: [f32; 2]) -> Option<End> {
    graph.nodes.iter().rev().find_map(|node| {
        let kind = catalog::kind(&node.kind)?;
        kind.inputs.iter().find_map(|pin| {
            chip_rect(graph, node, pin.name)
                .filter(|rect| rect.contains(p))
                .map(|_| End::new(node.id, pin.name))
        })
    })
}

pub(crate) fn rows(kind: &Kind) -> usize {
    kind.inputs.len().max(kind.outputs.len()).max(1)
}

pub(crate) fn rect(graph: &Graph, node: &Node) -> Rect {
    let Some(kind) = catalog::kind(&node.kind) else {
        return Rect {
            x: node.x,
            y: node.y,
            w: MIN_WIDTH,
            h: HEADER + ROW + TOP + BOTTOM,
        };
    };
    let title = 34.0 + kind.title.chars().count() as f32 * CHAR + 16.0;
    let widest = (0..rows(kind))
        .map(|row| {
            let left = kind.inputs.get(row).map_or(0.0, |pin| {
                let text = label_width(&label(graph, node, pin, Side::Input));
                text + chip(graph, node, pin).map_or(0.0, |chip| 6.0 + chip_width(&chip))
            });
            let right = kind.outputs.get(row).map_or(0.0, |pin| {
                label_width(&label(graph, node, pin, Side::Output))
            });
            14.0 + left + 18.0 + right + 14.0
        })
        .fold(title, f32::max);
    Rect {
        x: node.x,
        y: node.y,
        w: (widest.max(MIN_WIDTH) / 10.0).ceil() * 10.0,
        h: HEADER + TOP + ROW * rows(kind) as f32 + BOTTOM,
    }
}

/// The vertical centre of row `row`, from the node's top.
pub(crate) fn row_centre(row: usize) -> f32 {
    HEADER + TOP + ROW * row as f32 + ROW * 0.5
}

/// A pin's centre on the canvas.
pub(crate) fn pin(graph: &Graph, end: &End, side: Side) -> Option<[f32; 2]> {
    let node = graph.node(end.node)?;
    let kind = catalog::kind(&node.kind)?;
    let pins = match side {
        Side::Input => kind.inputs,
        Side::Output => kind.outputs,
    };
    let row = pins.iter().position(|pin| pin.name == end.pin)?;
    let rect = rect(graph, node);
    let x = match side {
        Side::Input => rect.x,
        Side::Output => rect.x + rect.w,
    };
    Some([x, rect.y + row_centre(row)])
}

/// The topmost node under a canvas point: later nodes draw over earlier.
pub(crate) fn node_at(graph: &Graph, p: [f32; 2]) -> Option<NodeId> {
    graph
        .nodes
        .iter()
        .rev()
        .find(|node| rect(graph, node).contains(p))
        .map(|node| node.id)
}

/// The pin nearest a canvas point within `reach`, with its side.
pub(crate) fn pin_at(graph: &Graph, p: [f32; 2], reach: f32) -> Option<(End, Side)> {
    let mut best: Option<(f32, End, Side)> = None;
    for node in graph.nodes.iter().rev() {
        let Some(kind) = catalog::kind(&node.kind) else {
            continue;
        };
        let rect = rect(graph, node);
        for (side, pins, x) in [
            (Side::Input, kind.inputs, rect.x),
            (Side::Output, kind.outputs, rect.x + rect.w),
        ] {
            for (row, pin) in pins.iter().enumerate() {
                let at = [x, rect.y + row_centre(row)];
                let distance = ((at[0] - p[0]).powi(2) + (at[1] - p[1]).powi(2)).sqrt();
                if distance <= reach && best.as_ref().is_none_or(|(d, _, _)| distance < *d) {
                    best = Some((distance, End::new(node.id, pin.name), side));
                }
            }
        }
    }
    best.map(|(_, end, side)| (end, side))
}

/// The band a group's title is drawn in, astride its top edge.
pub(crate) fn group_title(group: &Group) -> Rect {
    Rect {
        x: group.x + 12.0,
        y: group.y - 10.0,
        w: label_width(&group.title) + 12.0,
        h: 20.0,
    }
}

/// The group whose title is under a canvas point; later groups win.
pub(crate) fn group_title_at(graph: &Graph, p: [f32; 2]) -> Option<usize> {
    (0..graph.groups.len())
        .rev()
        .find(|&index| group_title(&graph.groups[index]).contains(p))
}

/// Everything on the canvas: nodes and groups. `None` for an empty graph.
pub(crate) fn extent(graph: &Graph) -> Option<Rect> {
    let nodes = graph.nodes.iter().map(|node| rect(graph, node));
    let groups = graph.groups.iter().map(|group| Rect {
        x: group.x,
        y: group.y,
        w: group.w,
        h: group.h,
    });
    nodes.chain(groups).reduce(|a, b| a.union(&b))
}

#[cfg(test)]
#[path = "layout/tests.rs"]
mod tests;
