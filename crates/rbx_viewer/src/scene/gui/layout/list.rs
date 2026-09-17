//! `UIListLayout` placement, flex included.
//!
//! One axis is the *fill* axis the siblings run along; the other is the
//! *cross* axis, which holds one line when `Wraps` is off and several when it
//! is on. Both axes are laid out by the same [`spread`]: a line of items on
//! the fill axis, and the lines themselves on the cross axis.

use super::{content_size, offset, ordered, Rect, TextMeasure};
use crate::scene::gui::plan::{Align, Flex, FlexItem, LineAlign, List, Node};

/// Slack big enough to swallow the float error a scale-resolved size carries,
/// small enough never to fit an item that genuinely overflows.
const EPSILON: f32 = 1e-3;

/// One thing laid along an axis: the size it asks for, and its share of
/// whatever space the line has left over (or has to claw back).
struct Flexed {
    basis: f32,
    grow: f32,
    shrink: f32,
}

/// `UIListLayout` placement, one rect per node in `nodes`'s own order, plus
/// the extent the result covers. The siblings are sorted, laid end to end
/// along the fill axis with `Padding` between them, and resized only where
/// flex says so. `Position` and `AnchorPoint` are ignored, as Roblox ignores
/// them.
pub(super) fn stacked(
    nodes: &[Node],
    list: &List,
    parent: &Rect,
    measure: &mut dyn TextMeasure,
) -> (Vec<Rect>, [f32; 2]) {
    let extent = parent.size();
    let along = usize::from(list.vertical);
    let across = 1 - along;
    let padding = list.padding.0 * extent[along] + list.padding.1;

    // Each sibling's flex basis: the size it asks for before any of the
    // line's free space is handed out.
    let basis: Vec<[f32; 2]> = nodes
        .iter()
        .map(|node| super::sizing::extent(node, extent, measure))
        .collect();
    let order = ordered(nodes, list.by_name);
    let lines = wrap(&order, &basis, along, padding, extent[along], list.wraps);

    let (main_align, cross_align) = match list.vertical {
        true => (list.vertical_align, list.horizontal),
        false => (list.horizontal, list.vertical_align),
    };
    // The lines are spread across the other axis by the same arithmetic, each
    // asking for the tallest (widest) item on it. Roblox names no separate
    // property for the gap between lines, so `Padding` serves for both.
    let stretch = list.cross_flex == Flex::Fill;
    let bands: Vec<Flexed> = lines
        .iter()
        .map(|line| Flexed {
            basis: line
                .iter()
                .map(|&index| basis[index][across])
                .fold(0.0, f32::max),
            grow: f32::from(u8::from(stretch)),
            shrink: f32::from(u8::from(stretch)),
        })
        .collect();
    let (bands, _) = spread(
        &bands,
        padding,
        extent[across],
        list.cross_flex,
        cross_align,
    );

    let mut rects = vec![
        Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0
        };
        nodes.len()
    ];
    for (line, &(band_at, band_size)) in lines.iter().zip(&bands) {
        let items: Vec<Flexed> = line
            .iter()
            .map(|&index| {
                let (grow, shrink) = ratios(nodes[index].flex, list);
                Flexed {
                    basis: basis[index][along],
                    grow,
                    shrink,
                }
            })
            .collect();
        let (placed, _) = spread(&items, padding, extent[along], list.flex, main_align);
        for (&index, &(at, size)) in line.iter().zip(&placed) {
            let (cross_at, cross_size) = across_line(
                nodes[index].flex,
                list,
                basis[index][across],
                (band_at, band_size),
            );
            let mut origin = [0.0; 2];
            let mut span = [0.0; 2];
            origin[along] = at;
            span[along] = size;
            origin[across] = cross_at;
            span[across] = cross_size;
            rects[index] = Rect {
                x: parent.x + origin[0],
                y: parent.y + origin[1],
                width: span[0],
                height: span[1],
            };
        }
    }

    let size = content_size(&rects);
    (rects, size)
}

/// An item's grow:shrink ratios. Its own `UIFlexItem` wins where it has one,
/// "letting you configure flex behavior on a per-object basis"; otherwise a
/// `Fill` layout gives every sibling the 1:1 ratio `Fill` means.
fn ratios(item: Option<FlexItem>, list: &List) -> (f32, f32) {
    match item {
        Some(flex) => (flex.grow, flex.shrink),
        None if list.flex == Flex::Fill => (1.0, 1.0),
        None => (0.0, 0.0),
    }
}

/// Where one item sits across the line it landed on, and how tall (wide) it
/// ends up.
fn across_line(
    item: Option<FlexItem>,
    list: &List,
    basis: f32,
    (at, band): (f32, f32),
) -> (f32, f32) {
    let align = match list.vertical {
        true => list.horizontal,
        false => list.vertical_align,
    };
    // A `UIFlexItem`'s own `ItemLineAlignment` overrides the layout's, and
    // `Automatic` on either means "no opinion".
    let line_align = match item.map(|flex| flex.line_align) {
        Some(LineAlign::Automatic) | None => list.line_align,
        Some(other) => other,
    };
    match line_align {
        // Cross-direction `Fill` "makes the siblings fill the entire
        // [cross-axis] space", which is what stretching to the line comes to.
        LineAlign::Automatic if list.cross_flex == Flex::Fill => (at, band),
        LineAlign::Automatic => (at + offset(align, band, basis), basis),
        LineAlign::Start => (at, basis),
        LineAlign::Center => (at + (band - basis) * 0.5, basis),
        LineAlign::End => (at + band - basis, basis),
        LineAlign::Stretch => (at, band),
    }
}

/// Breaks the sorted siblings into lines. Without `Wraps` that is one line
/// however far it overflows, which is Roblox's own non-wrapping behaviour.
fn wrap(
    order: &[usize],
    basis: &[[f32; 2]],
    along: usize,
    padding: f32,
    extent: f32,
    wraps: bool,
) -> Vec<Vec<usize>> {
    if !wraps {
        return vec![order.to_vec()];
    }

    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for &index in order {
        // Wrapping is decided on the basis sizes, since the docs phrase it as
        // siblings whose "default size exceeds the width/height" — the flexed
        // sizes are not known until the line is closed.
        let size = basis[index][along];
        match lines.last_mut() {
            Some(line) if used + padding + size <= extent + EPSILON => {
                used += padding + size;
                line.push(index);
            }
            _ => {
                lines.push(vec![index]);
                used = size;
            }
        }
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

/// Lays `items` end to end inside `extent`, handing out (or clawing back) the
/// space left over. Returns each item's (position, size) and the extent the
/// run covers.
fn spread(
    items: &[Flexed],
    padding: f32,
    extent: f32,
    flex: Flex,
    align: Align,
) -> (Vec<(f32, f32)>, f32) {
    let count = items.len();
    if count == 0 {
        return (Vec::new(), 0.0);
    }

    let gaps = padding * (count - 1) as f32;
    let mut sizes: Vec<f32> = items.iter().map(|item| item.basis).collect();
    let free = extent - sizes.iter().sum::<f32>() - gaps;
    // Grow or shrink, never both: which one applies is decided by the sign of
    // the free space, exactly as `UIFlexItem.FlexMode` describes it.
    let ratios: Vec<f32> = match free >= 0.0 {
        true => items.iter().map(|item| item.grow).collect(),
        false => items.iter().map(|item| item.shrink).collect(),
    };
    let total: f32 = ratios.iter().sum();
    if total > 0.0 {
        for (size, ratio) in sizes.iter_mut().zip(&ratios) {
            *size = (*size + free * ratio / total).max(0.0);
        }
    }

    let used = sizes.iter().sum::<f32>() + gaps;
    let slack = (extent - used).max(0.0);
    let count = count as f32;
    let (lead, gap) = match flex {
        Flex::SpaceBetween if items.len() > 1 => (0.0, padding + slack / (count - 1.0)),
        Flex::SpaceAround => (slack / (2.0 * count), padding + slack / count),
        Flex::SpaceEvenly => (slack / (count + 1.0), padding + slack / (count + 1.0)),
        // `None`, `Fill` (whose free space the ratios above already spent) and
        // a lone `SpaceBetween` item all fall back to plain alignment.
        _ => (offset(align, extent, used), padding),
    };

    let mut placed = Vec::with_capacity(items.len());
    let mut cursor = lead;
    for size in sizes {
        placed.push((cursor, size));
        cursor += size + gap;
    }
    (placed, cursor - gap - lead)
}
