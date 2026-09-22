//! What the canvas shows of an auto layout while one element in it (or the
//! element holding it) is selected: the container, its children numbered
//! in the order the layout puts them, and the gaps between them and the
//! padding round them as bands — each of which a drag widens or narrows,
//! making the `UIPadding` a side's band stands for when there is none.

use rbx_dom::{Ref, Variant};
use rbx_viewer::GuiBox;

use super::inspector::Key;
use super::{is_gui_object, Shell};
use crate::ui_canvas::{box_of, Rect};

const LIST: &str = "UIListLayout";
const GRID: &str = "UIGridLayout";

/// A strip of the layout a drag resizes: `key` grows by the pointer's
/// travel along `axis`, times `sign` — a right or bottom side's padding
/// grows the other way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Band {
    pub(super) rect: Rect,
    pub(super) key: Key,
    pub(super) axis: usize,
    pub(super) sign: f32,
    /// A gap rather than padding: how it is drawn.
    pub(super) gap: bool,
}

/// One auto layout as the canvas shows it.
#[derive(Debug, Clone)]
pub(super) struct Shown {
    pub(super) layout: Ref,
    pub(super) container: Rect,
    /// The children in the layout's order, each with its box.
    pub(super) children: Vec<(Ref, Rect)>,
    /// A grid's cells, or the axis a list runs along.
    pub(super) grid: bool,
    pub(super) axis: usize,
    pub(super) bands: Vec<Band>,
}

impl Shell {
    /// The auto layout the selection shows: the selected element's own, or
    /// the one its parent lays it out by. Only for one element, and only
    /// where the container is square to the screen — a band on a turned
    /// container has no edge on the canvas's axes to be dragged along.
    pub(super) fn shown_layout(&self, root: &GuiBox, boxes: &[GuiBox]) -> Option<Shown> {
        let [selected] = self.selected_all() else {
            return None;
        };
        let (container, layout) = [Some(*selected), self.dom.parent(*selected)]
            .into_iter()
            .flatten()
            .find_map(|candidate| {
                let layout = self
                    .child_of(candidate, LIST)
                    .or_else(|| self.child_of(candidate, GRID))?;
                Some((candidate, layout))
            })?;
        let placed = match container == root.referent {
            true => root,
            false => box_of(boxes, container)?,
        };
        if placed.rotation != 0.0 {
            return None;
        }
        let rect = Rect::of(placed);
        let content = placed.content.map(Rect::from_array).unwrap_or(rect);
        let grid = self.dom.get(layout).is_some_and(|i| i.class() == GRID);
        let horizontal = matches!(
            self.dom
                .get(layout)
                .and_then(|i| self.database.stored_or_default(i, "FillDirection")),
            Some((_, Variant::Enum(0)))
        );
        let axis = usize::from(!horizontal);
        let mut children: Vec<(Ref, Rect)> = self
            .dom
            .get(container)?
            .children()
            .iter()
            .copied()
            .filter(|&child| is_gui_object(&self.dom, &self.database, child))
            .filter_map(|child| Some((child, Rect::of(box_of(boxes, child)?))))
            .collect();
        // The order they come out in on screen, which is the layout's own:
        // along a list's axis, or row by row through a grid.
        children.sort_by(|(_, a), (_, b)| {
            match grid {
                true => (a.y.round(), a.x).partial_cmp(&(b.y.round(), b.x)),
                false => a.along(axis).0.partial_cmp(&b.along(axis).0),
            }
            .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut bands = Vec::new();
        for pair in children.windows(2) {
            let (a, b) = (pair[0].1, pair[1].1);
            // A grid's next cell is either beside this one or on the next
            // row; a list's is always further along its axis.
            let (run, key) = match grid {
                true if (a.y - b.y).abs() < 0.5 => (0, Key::GridGapX),
                true => continue,
                false => (axis, Key::Gap),
            };
            let (start, length) = a.along(run);
            let end = b.along(run).0;
            let cross = 1 - run;
            let from = a.along(cross).0.max(b.along(cross).0);
            let to = (a.along(cross).0 + a.along(cross).1).min(b.along(cross).0 + b.along(cross).1);
            bands.push(Band {
                rect: span(run, start + length, end, from, to),
                key,
                axis: run,
                sign: 1.0,
                gap: true,
            });
        }
        if grid {
            // One band per row break, under the cell that ends the row.
            let rows: Vec<Rect> = children.iter().map(|(_, rect)| *rect).collect();
            for pair in rows.windows(2) {
                if pair[1].y - pair[0].y > 0.5 {
                    bands.push(Band {
                        rect: span(
                            1,
                            pair[0].y + pair[0].h,
                            pair[1].y,
                            pair[0].x,
                            pair[0].x + pair[0].w,
                        ),
                        key: Key::GridGapY,
                        axis: 1,
                        sign: 1.0,
                        gap: true,
                    });
                }
            }
        }
        let sides = [
            (
                Key::PadTop,
                1,
                1.0,
                span(1, rect.y, content.y, rect.x, rect.x + rect.w),
            ),
            (
                Key::PadBottom,
                1,
                -1.0,
                span(
                    1,
                    content.y + content.h,
                    rect.y + rect.h,
                    rect.x,
                    rect.x + rect.w,
                ),
            ),
            (
                Key::PadLeft,
                0,
                1.0,
                span(0, rect.x, content.x, rect.y, rect.y + rect.h),
            ),
            (
                Key::PadRight,
                0,
                -1.0,
                span(
                    0,
                    content.x + content.w,
                    rect.x + rect.w,
                    rect.y,
                    rect.y + rect.h,
                ),
            ),
        ];
        bands.extend(sides.map(|(key, axis, sign, rect)| Band {
            rect,
            key,
            axis,
            sign,
            gap: false,
        }));
        Some(Shown {
            layout,
            container: rect,
            children,
            grid,
            axis,
            bands,
        })
    }
}

/// The rectangle from `from` to `to` along `axis`, and `cross_from` to
/// `cross_to` across it.
fn span(axis: usize, from: f32, to: f32, cross_from: f32, cross_to: f32) -> Rect {
    let (start, end) = (from.min(to), from.max(to));
    let (cross_start, cross_end) = (cross_from.min(cross_to), cross_from.max(cross_to));
    match axis {
        0 => Rect {
            x: start,
            y: cross_start,
            w: end - start,
            h: cross_end - cross_start,
        },
        _ => Rect {
            x: cross_start,
            y: start,
            w: cross_end - cross_start,
            h: end - start,
        },
    }
}

/// Where a child dragged to `point` lands among the others (`children`
/// less itself, in order): before the first whose middle it is short of in
/// a list, at the nearest cell in a grid.
pub(super) fn drop_index(
    children: &[(Ref, Rect)],
    point: [f32; 2],
    grid: bool,
    axis: usize,
) -> usize {
    match grid {
        true => children
            .iter()
            .enumerate()
            .min_by(|(_, (_, a)), (_, (_, b))| {
                let distance = |r: &Rect| {
                    let c = r.centre();
                    (c[0] - point[0]).hypot(c[1] - point[1])
                };
                distance(a).total_cmp(&distance(b))
            })
            .map_or(0, |(index, _)| index),
        false => children
            .iter()
            .filter(|(_, rect)| rect.centre()[axis] < point[axis])
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(x: f32, y: f32) -> (Ref, Rect) {
        (
            Ref::new((x + y * 10.0) as u32 + 1),
            Rect {
                x,
                y,
                w: 10.0,
                h: 10.0,
            },
        )
    }

    #[test]
    fn a_list_drop_lands_before_the_first_middle_it_is_short_of() {
        let children = [cell(0.0, 0.0), cell(0.0, 20.0), cell(0.0, 40.0)];
        assert_eq!(drop_index(&children, [5.0, 2.0], false, 1), 0);
        assert_eq!(drop_index(&children, [5.0, 30.0], false, 1), 2);
        assert_eq!(drop_index(&children, [5.0, 99.0], false, 1), 3);
    }

    #[test]
    fn a_grid_drop_lands_on_the_nearest_cell() {
        let children = [cell(0.0, 0.0), cell(20.0, 0.0), cell(0.0, 20.0)];
        assert_eq!(drop_index(&children, [24.0, 3.0], true, 0), 1);
        assert_eq!(drop_index(&children, [2.0, 28.0], true, 0), 2);
    }

    #[test]
    fn a_span_is_the_same_whichever_way_its_ends_come() {
        assert_eq!(span(0, 10.0, 4.0, 3.0, 1.0), span(0, 4.0, 10.0, 1.0, 3.0));
        assert_eq!(
            span(1, 0.0, 5.0, 0.0, 2.0),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 2.0,
                h: 5.0
            }
        );
    }
}
