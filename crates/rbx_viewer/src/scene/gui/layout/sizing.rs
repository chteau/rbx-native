//! How big a `GuiObject` ends up, and how much of that box is left for its
//! children.
//!
//! The order the modifiers apply in is not stated anywhere in the Roblox docs.
//! This is the order [`extent`] uses, and the reasoning for it:
//!
//! 1. `Size` against the parent, through `SizeConstraint`, which is the only
//!    thing that decides what a scale even means.
//! 2. `UIScale`, documented purely as a multiplier on `AbsoluteSize`.
//! 3. `AutomaticSize`, for which "the object's `Size` property controls its
//!    minimum size" — so it has to see the size the first two produced.
//! 4. `UIAspectRatioConstraint`, which the docs say overrides a layout, so it
//!    comes after anything that merely proposes a size.
//! 5. `UISizeConstraint`, a hard pixel clamp, last so nothing can push the
//!    element back outside `MinSize`/`MaxSize`.

use super::super::plan::{Aspect, Node};
use super::Rect;

/// The element's final pixel size inside a parent box of `parent` pixels.
pub(super) fn extent(node: &Node, parent: [f32; 2]) -> [f32; 2] {
    let mut size = node.size.against(node.size_constraint.against(parent));

    if let Some(scale) = node.constraints.scale {
        size = [size[0] * scale, size[1] * scale];
    }

    if node.automatic_size != [false, false] {
        let content = content_extent(node, size);
        for axis in 0..2 {
            if node.automatic_size[axis] {
                size[axis] = size[axis].max(content[axis]);
            }
        }
    }

    if let Some(aspect) = &node.constraints.aspect {
        size = with_aspect(size, parent, aspect);
    }

    if let Some(bounds) = &node.constraints.size_bounds {
        for (axis, extent) in size.iter_mut().enumerate() {
            // `MaxSize` is documented as being at least `MinSize`; a place that
            // broke that rule would otherwise make `clamp` panic.
            *extent = extent.clamp(bounds.min[axis], bounds.max[axis].max(bounds.min[axis]));
        }
    }
    size
}

/// The box this element's children — and any layout arranging them — resolve
/// against: its own rect, less `UIPadding`.
pub(super) fn padded(node: &Node, rect: &Rect) -> Rect {
    let Some(padding) = &node.constraints.padding else {
        return *rect;
    };
    inset(rect, &padding.against(rect.size()))
}

fn inset(rect: &Rect, sides: &[f32; 4]) -> Rect {
    Rect {
        x: rect.x + sides[0],
        y: rect.y + sides[2],
        width: (rect.width - sides[0] - sides[1]).max(0.0),
        height: (rect.height - sides[2] - sides[3]).max(0.0),
    }
}

/// How much room this element's content asks for, `UIPadding` included.
///
/// Along an automatic axis the element's own size is what is being worked out,
/// so the box the children resolve against is zero wide there and a child
/// sized or positioned by scale contributes only its offsets. The docs
/// describe neither that circularity nor what a scale-sized child should do,
/// and this is the only reading that terminates.
fn content_extent(node: &Node, size: [f32; 2]) -> [f32; 2] {
    let probe = [
        if node.automatic_size[0] { 0.0 } else { size[0] },
        if node.automatic_size[1] { 0.0 } else { size[1] },
    ];
    let sides = node
        .constraints
        .padding
        .as_ref()
        .map_or([0.0; 4], |padding| padding.against(probe));
    // At the origin, not where the padding would actually put it: the
    // children's extent is measured from the padded box's own corner and the
    // padding is added back on both sides below.
    let inner = Rect {
        x: 0.0,
        y: 0.0,
        width: (probe[0] - sides[0] - sides[1]).max(0.0),
        height: (probe[1] - sides[2] - sides[3]).max(0.0),
    };

    // The content's own bounding box, which a centred `UIListLayout` can put
    // partly left of the origin once the box it centres in is zero wide.
    let mut low = [0.0f32; 2];
    let mut high = node.content_size.unwrap_or([0.0; 2]);
    for rect in super::arrange(&node.children, node.list.as_ref(), &inner).rects {
        low[0] = low[0].min(rect.x);
        low[1] = low[1].min(rect.y);
        high[0] = high[0].max(rect.x + rect.width);
        high[1] = high[1].max(rect.y + rect.height);
    }

    [
        high[0] - low[0] + sides[0] + sides[1],
        high[1] - low[1] + sides[2] + sides[3],
    ]
}

/// `UIAspectRatioConstraint`: the dominant axis keeps the size it already had
/// and the other follows from `AspectRatio`, then the pair shrinks — together,
/// so the ratio survives — until it fits the box the `AspectType` names.
///
/// `FitWithinMaxSize` fits it inside the element's own size, which makes the
/// result "the maximum size possible within its own AbsoluteSize" whichever
/// axis dominates; `ScaleWithParentSize` fits it inside the parent instead,
/// where the dominant axis is what the size is actually taken from.
fn with_aspect(size: [f32; 2], parent: [f32; 2], aspect: &Aspect) -> [f32; 2] {
    let candidate = match aspect.height_dominant {
        false => [size[0], size[0] / aspect.ratio],
        true => [size[1] * aspect.ratio, size[1]],
    };
    let bound = match aspect.with_parent {
        true => parent,
        false => size,
    };

    let mut shrink = 1.0f32;
    for axis in 0..2 {
        if candidate[axis] > 0.0 {
            shrink = shrink.min(bound[axis] / candidate[axis]);
        }
    }
    [candidate[0] * shrink, candidate[1] * shrink]
}
