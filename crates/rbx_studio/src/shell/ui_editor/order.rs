//! Paint order on the canvas: bringing the selection forward or sending it
//! back (Ctrl+] and Ctrl+[, Shift for all the way), and dragging a child of
//! a list or grid layout to a new place in it.
//!
//! Siblings paint by `ZIndex`, then by their order under the parent, so an
//! element stepping over a neighbour takes that neighbour's `ZIndex` and
//! the place after (or before) it — one step in what is seen, whatever
//! either held. A layout orders by `LayoutOrder` instead, which a reorder
//! renumbers.

use gpui_kit::*;
use rbx_dom::{Ref, Variant};

use super::tree::Writes;
use super::{is_gui_object, Shell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Arrange {
    Front,
    Forward,
    Backward,
    Back,
}

impl Shell {
    fn z_index(&self, referent: Ref) -> i32 {
        match self
            .dom
            .get(referent)
            .and_then(|instance| self.database.stored_or_default(instance, "ZIndex"))
        {
            Some((_, Variant::Int32(z))) => *z,
            _ => 1,
        }
    }

    /// `parent`'s `GuiObject` children in the order they paint, each with
    /// its `ZIndex`.
    fn paint_order(&self, parent: Ref) -> Vec<(Ref, i32)> {
        let mut order: Vec<(Ref, i32)> = self
            .dom
            .get(parent)
            .map(|instance| instance.children().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|&child| is_gui_object(&self.dom, &self.database, child))
            .map(|child| (child, self.z_index(child)))
            .collect();
        // Stable: equal `ZIndex`es keep the order they sit in.
        order.sort_by_key(|&(_, z)| z);
        order
    }

    pub(super) fn arrange_gui(&mut self, arrange: Arrange, cx: &mut Context<Self>) {
        let selected = self.inspected();
        let mut parents: Vec<Ref> = Vec::new();
        for parent in selected.iter().filter_map(|&r| self.dom.parent(r)) {
            if !parents.contains(&parent) {
                parents.push(parent);
            }
        }
        let mut plans: Vec<(Ref, Vec<(Ref, i32)>)> = Vec::new();
        let mut writes = Writes::new();
        for parent in parents {
            let before = self.paint_order(parent);
            let order = arranged(&before, &selected, arrange);
            if order == before {
                continue;
            }
            for &(referent, z) in &order {
                if before.iter().any(|&(r, old)| r == referent && old != z) {
                    writes.push((referent, "ZIndex", z.to_string()));
                }
            }
            plans.push((parent, order));
        }
        if plans.is_empty() {
            return;
        }
        self.edit_gui_tree(
            "arrange",
            |dom, _| {
                for (parent, order) in plans {
                    // Re-parenting in place appends: doing it to every one
                    // in turn leaves them in exactly this order.
                    for (referent, _) in order {
                        dom.set_parent(referent, Some(parent));
                    }
                }
                (writes, None)
            },
            cx,
        );
    }

    /// Puts `moved` at `index` among its layout's children as `order` holds
    /// them, renumbering every one's `LayoutOrder` to match — and making
    /// the layout sort by it, which a renumbering is otherwise lost on.
    pub(super) fn reorder_gui(
        &mut self,
        layout: Ref,
        mut order: Vec<Ref>,
        moved: Ref,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = order.iter().position(|&r| r == moved) else {
            return;
        };
        order.remove(from);
        order.insert(index.min(order.len()), moved);
        let mut writes: Writes = order
            .iter()
            .enumerate()
            .map(|(place, &child)| (child, "LayoutOrder", place.to_string()))
            .collect();
        writes.push((layout, "SortOrder", "LayoutOrder".to_owned()));
        self.write_drag(true, &writes, cx);
    }
}

/// `before` with `selected` moved as `arrange` says, `ZIndex`es and all.
fn arranged(before: &[(Ref, i32)], selected: &[Ref], arrange: Arrange) -> Vec<(Ref, i32)> {
    let mut order = before.to_vec();
    let moving = |r: &Ref| selected.contains(r);
    let top = before.iter().map(|&(_, z)| z).max().unwrap_or(1);
    let bottom = before.iter().map(|&(_, z)| z).min().unwrap_or(1);
    match arrange {
        Arrange::Front => {
            let (mut raised, rest): (Vec<_>, Vec<_>) =
                order.into_iter().partition(|(r, _)| moving(r));
            raised.iter_mut().for_each(|(_, z)| *z = top);
            order = rest.into_iter().chain(raised).collect();
        }
        Arrange::Back => {
            let (mut lowered, rest): (Vec<_>, Vec<_>) =
                order.into_iter().partition(|(r, _)| moving(r));
            lowered.iter_mut().for_each(|(_, z)| *z = bottom);
            order = lowered.into_iter().chain(rest).collect();
        }
        Arrange::Forward => {
            for index in (0..order.len().saturating_sub(1)).rev() {
                if moving(&order[index].0) && !moving(&order[index + 1].0) {
                    let (referent, _) = order.remove(index);
                    let z = order[index].1;
                    order.insert(index + 1, (referent, z));
                }
            }
        }
        Arrange::Backward => {
            for index in 1..order.len() {
                if moving(&order[index].0) && !moving(&order[index - 1].0) {
                    let (referent, _) = order.remove(index);
                    let z = order[index - 1].1;
                    order.insert(index - 1, (referent, z));
                }
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use rbx_dom::Ref;

    use super::{arranged, Arrange};

    fn refs(n: u32) -> Vec<Ref> {
        (1..=n).map(Ref::new).collect()
    }

    #[test]
    fn forward_steps_over_one_neighbour_and_takes_its_z_index() {
        let r = refs(3);
        let before = vec![(r[0], 1), (r[1], 1), (r[2], 5)];
        let after = arranged(&before, &[r[0]], Arrange::Forward);
        assert_eq!(after, vec![(r[1], 1), (r[0], 1), (r[2], 5)]);
        let after = arranged(&after, &[r[0]], Arrange::Forward);
        assert_eq!(after, vec![(r[1], 1), (r[2], 5), (r[0], 5)]);
    }

    #[test]
    fn front_and_back_go_all_the_way_and_keep_the_selection_s_own_order() {
        let r = refs(4);
        let before = vec![(r[0], 1), (r[1], 2), (r[2], 3), (r[3], 4)];
        let front = arranged(&before, &[r[0], r[1]], Arrange::Front);
        assert_eq!(front, vec![(r[2], 3), (r[3], 4), (r[0], 4), (r[1], 4)]);
        let back = arranged(&before, &[r[3]], Arrange::Back);
        assert_eq!(back[0], (r[3], 1));
    }

    #[test]
    fn nothing_moves_past_the_ends() {
        let r = refs(2);
        let before = vec![(r[0], 1), (r[1], 1)];
        assert_eq!(arranged(&before, &[r[1]], Arrange::Forward), before);
        assert_eq!(arranged(&before, &[r[0]], Arrange::Backward), before);
    }
}
