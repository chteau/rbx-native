//! The walk from a container's box to its emitted elements: what is handed
//! down the tree ([`Context`]), what one box holds ([`Scope`]), and the order
//! its contents are placed and painted in.

use super::super::plan::{Group, Layout, Node};
use super::{arrange, emit, Arranged, Element, Rect, TextMeasure};

/// What an element hands down to the subtree under it.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::scene::gui) struct Context {
    /// The scissor rect inherited from the nearest `ClipsDescendants`
    /// ancestor, already intersected down the whole chain.
    pub(in crate::scene::gui) clip: Option<Rect>,
    /// Whether this element or any ancestor carries a non-zero `Rotation`,
    /// which is what turns `ClipsDescendants` off.
    pub(in crate::scene::gui) rotated: bool,
    /// The ancestors' cumulative `AbsoluteRotation`, and the screen point it
    /// turns about — the nearest rotated ancestor's own centre.
    pub(in crate::scene::gui) angle: f32,
    pub(in crate::scene::gui) pivot: [f32; 2],
    /// `ZIndexBehavior.Global`, where siblings are emitted in tree order and
    /// [`resolve`] sorts the whole screen by `ZIndex` afterwards.
    pub(in crate::scene::gui) global_z_index: bool,
}

impl Context {
    /// Where a child's axis-aligned box actually lands: an ancestor's rotation
    /// carries the whole box around that ancestor's centre, and only then does
    /// the child turn about its own.
    pub(in crate::scene::gui) fn carried(&self, rect: Rect) -> Rect {
        rect.turned(self.angle, self.pivot)
    }
}

/// One box's worth of contents to place: the elements themselves, the layout
/// arranging them, and the scopes any plain instances among them open — see
/// [`Group`].
#[derive(Clone, Copy)]
pub(in crate::scene::gui) struct Scope<'a> {
    pub(in crate::scene::gui) nodes: &'a [Node],
    pub(in crate::scene::gui) groups: &'a [Group],
    pub(in crate::scene::gui) layout: Option<&'a Layout>,
}

impl<'a> Scope<'a> {
    pub(in crate::scene::gui) fn of(group: &'a Group) -> Self {
        Scope {
            nodes: &group.children,
            groups: &group.groups,
            layout: group.layout.as_ref(),
        }
    }
}

/// One element about to be emitted, with the rect its own layout scope gave
/// it — and the cells a `UITableLayout` in that scope handed down.
struct Placed<'a> {
    node: &'a Node,
    rect: Rect,
    cells: Option<Vec<Rect>>,
}

/// Places every sibling inside `parent`, then emits them in paint order.
///
/// The two orders are distinct: a layout decides where a sibling sits,
/// `ZIndex` decides which one is drawn over the other.
///
/// `given` is the one exception to a sibling being placed here at all: a
/// `UITableLayout` sizes its cells, which are its siblings' children, so it
/// hands them down ready-made.
pub(in crate::scene::gui) fn children(
    scope: Scope<'_>,
    parent: &Rect,
    given: Option<&[Rect]>,
    context: Context,
    measure: &mut dyn TextMeasure,
    into: &mut Vec<Element>,
) {
    let mut placed = Vec::with_capacity(scope.nodes.len());
    place_scope(scope, parent, given, measure, &mut placed);
    // Stable, so siblings sharing a `ZIndex` keep tree order. Under
    // `ZIndexBehavior.Global` they are left in tree order outright: that is
    // the hierarchy order the screen-wide sort breaks ties with, and
    // reordering them here would interleave their subtrees wrongly.
    if !context.global_z_index {
        placed.sort_by_key(|item| item.node.z_index);
    }
    for item in placed {
        emit(
            item.node,
            context.carried(item.rect),
            item.cells.as_deref(),
            context,
            measure,
            into,
        );
    }
}

/// One layout scope's worth of placement, appended to `into` in tree order.
///
/// A [`Group`] is a scope of its own inside the same `parent` box: its
/// contents are arranged by the group's own layout, never by `layout`, and
/// they land where the group sits among `nodes` so that a container which is
/// not itself drawn still leaves its contents in tree order.
fn place_scope<'a>(
    scope: Scope<'a>,
    parent: &Rect,
    given: Option<&[Rect]>,
    measure: &mut dyn TextMeasure,
    into: &mut Vec<Placed<'a>>,
) {
    let arranged = match given {
        Some(rects) => Arranged {
            rects: rects.to_vec(),
            cells: None,
        },
        None => arrange(scope.nodes, scope.layout, parent, measure),
    };
    let mut next = 0;
    for index in 0..=scope.nodes.len() {
        while let Some(group) = scope.groups.get(next).filter(|group| group.at <= index) {
            place_scope(Scope::of(group), parent, None, measure, into);
            next += 1;
        }
        if index < scope.nodes.len() {
            into.push(Placed {
                node: &scope.nodes[index],
                rect: arranged.rects[index],
                cells: arranged.cells.as_ref().map(|cells| cells[index].clone()),
            });
        }
    }
}
