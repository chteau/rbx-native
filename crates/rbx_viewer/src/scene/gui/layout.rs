//! Resolves a planned `ScreenGui` tree into flat, screen-space rectangles in
//! the order they have to be painted.
//!
//! Coordinates are pixels with the origin at the viewport's top-left corner,
//! which is the frame `UDim2` itself is written in; the renderer is what turns
//! them into clip space.

mod text;

use rbx_dom::Ref;

use super::plan::{Align, GroupTint, Node, Screen, Span, Viewport};
use super::space::SpaceGui;
use super::wheel::ScrollWindow;
// Reaches all the way to `renderer::gui::quads::image`, unlike everything
// else `plan` hands this module — see the type's own doc comment.
pub(crate) use super::plan::PixelRect;

mod arrange;
mod grid;
mod image;
mod list;
mod modifiers;
mod page;
mod rect;
mod scrolling;
mod sizing;
mod table;
mod walk;

#[allow(unused_imports)]
use arrange::Arranged as _;
pub(crate) use arrange::{arrange, Arranged};
use image::painted;
pub(crate) use image::{ImageScale, Painted};
pub(crate) use modifiers::{GradientPx, StrokePx};
pub(crate) use rect::Rect;
pub(crate) use text::{TextMeasure, Typeset};
pub(in crate::scene::gui) use walk::{children, Context, Scope};

/// One `GuiObject` at its final pixel position, ready to be drawn on its own.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Element {
    /// The `GuiObject` this was laid out for — a `ScrollingFrame`'s bar
    /// segments carry the frame's own. What an editor hit-tests against.
    pub(crate) referent: Ref,
    pub(crate) rect: Rect,
    /// The scissor rect inherited from the nearest `ClipsDescendants`
    /// ancestor, if any. Already intersected down the whole chain.
    pub(crate) clip: Option<Rect>,
    /// `AbsoluteRotation`: degrees clockwise around `rect`'s own centre, this
    /// element's `Rotation` plus every ancestor's. `rect` has already been
    /// carried around those ancestors' centres, so turning it about its own
    /// is all that is left to do.
    pub(crate) rotation: f32,
    pub(crate) background: [f32; 3],
    pub(crate) background_alpha: f32,
    /// `BorderSizePixel` and `BorderColor3`, `None` for a zero-width border
    /// — or for any rounded box, since a square outline around one is not
    /// what a `UICorner` shows; the docs say nothing about the two together.
    pub(crate) border: Option<(f32, [f32; 3])>,
    /// How far inside `rect` the border's outer edge sits, per `BorderMode`.
    pub(crate) border_inset: f32,
    /// `ZIndex`, kept so a `ZIndexBehavior.Global` screen can sort its whole
    /// flattened tree by it after the fact.
    pub(crate) z_index: i32,
    pub(crate) image: Option<Painted>,
    /// `UICorner` in pixels, top-left first then clockwise; all zero without.
    pub(crate) corner_radii: [f32; 4],
    /// Every `UIStroke`, in the order they are painted.
    pub(crate) strokes: Vec<StrokePx>,
    pub(crate) gradient: Option<GradientPx>,
    /// A text object's text, drawn over the background and image.
    pub(crate) text: Option<Typeset>,
    /// A `ViewportFrame`'s 3D content. The renderer bakes it to a texture of
    /// `rect`'s pixel size and fills `image` in with it, so it lands over the
    /// background exactly as an `ImageLabel`'s image would.
    pub(crate) viewport: Option<Viewport>,
    /// A `CanvasGroup` whose subtree the renderer is to flatten before
    /// tinting; `None` for every other element, and for a group under
    /// `ZIndexBehavior.Global`.
    pub(crate) group: Option<Grouped>,
    /// A `ScrollingFrame`'s window, for a host to hit-test the wheel against
    /// (see `super::wheel`); `None` for every other element.
    pub(crate) scroll: Option<ScrollWindow>,
}

/// A `CanvasGroup`'s tint and the run of elements it applies to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Grouped {
    pub(crate) tint: GroupTint,
    /// How many elements straight after the group's own are its subtree,
    /// contiguous because siblings are emitted depth-first.
    pub(crate) descendants: usize,
    /// The texture slot the renderer baked the subtree into, once it has;
    /// the layout leaves it `None`.
    pub(crate) texture: Option<usize>,
}

/// Every element of every screen, in paint order: `DisplayOrder` first, then
/// `ZIndex` among siblings, then tree order — and a child always over its
/// parent, which is what `ZIndexBehavior.Sibling` (the default) means.
///
/// Text is laid out at `TextSize` as is, unmeasured: what a test with no font
/// system wants, and what the renderer never calls — see [`resolve_with`].
#[cfg(test)]
pub(crate) fn resolve(screens: &[Screen], viewport: [f32; 2]) -> Vec<Element> {
    resolve_with(screens, viewport, &mut text::Unmeasured)
}

/// [`resolve`] with the text measured by `measure` — what the renderer calls,
/// so `TextScaled` and an `AutomaticSize` text box come out at the size the
/// glyphs will actually take.
pub(crate) fn resolve_with<'a>(
    screens: impl IntoIterator<Item = &'a Screen>,
    viewport: [f32; 2],
    measure: &mut dyn TextMeasure,
) -> Vec<Element> {
    let mut order: Vec<&Screen> = screens.into_iter().collect();
    // Stable, so two screens sharing a `DisplayOrder` keep the order the DOM
    // holds them in rather than an arbitrary one.
    order.sort_by_key(|screen| screen.display_order);

    let mut elements = Vec::new();
    for screen in order {
        // `ScreenInsets`: the canvas starts below the top bar, and is that
        // much shorter, so a `{1, 0}` child still reaches the bottom edge.
        let frame = Rect {
            y: screen.top_inset,
            height: (viewport[1] - screen.top_inset).max(0.0),
            ..canvas(viewport)
        };
        let start = elements.len();
        children(
            Scope {
                nodes: &screen.roots,
                groups: &screen.groups,
                layout: screen.list.as_ref(),
            },
            &frame,
            None,
            Context {
                global_z_index: screen.global_z_index,
                // `ClipToDeviceSafeArea` scissors the whole screen to the
                // canvas the insets leave, exactly as an ancestor's
                // `ClipsDescendants` would.
                clip: screen.clip_to_safe_area.then_some(frame),
                ..Context::default()
            },
            measure,
            &mut elements,
        );
        if screen.global_z_index {
            // "Sorts all descendants according to the ZIndex, then breaks ties
            // using the hierarchy order": the walk above already emitted them
            // in hierarchy order, so a stable sort is the whole of it.
            elements[start..].sort_by_key(|element| element.z_index);
        }
    }
    elements
}

/// The same resolution for a `BillboardGui`/`SurfaceGui`, against its own
/// canvas instead of the viewport: a container drawn into an offscreen texture
/// is a viewport of `canvas` pixels as far as a `UDim2` is concerned, which is
/// the whole reason this and [`resolve`] are one code path.
#[cfg(test)]
pub(crate) fn resolve_canvas(gui: &SpaceGui) -> Vec<Element> {
    resolve_canvas_with(gui, &mut text::Unmeasured)
}

/// [`resolve_canvas`] with the text measured — see [`resolve_with`].
pub(crate) fn resolve_canvas_with(gui: &SpaceGui, measure: &mut dyn TextMeasure) -> Vec<Element> {
    let frame = canvas(gui.canvas);
    let mut elements = Vec::new();
    children(
        Scope {
            nodes: &gui.roots,
            groups: &gui.groups,
            layout: gui.list.as_ref(),
        },
        &frame,
        None,
        Context {
            global_z_index: gui.global_z_index,
            ..Context::default()
        },
        measure,
        &mut elements,
    );
    if gui.global_z_index {
        elements.sort_by_key(|element| element.z_index);
    }
    elements
}

/// The box top-level children resolve against: the whole target, whatever it is.
fn canvas(size: [f32; 2]) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: size[0],
        height: size[1],
    }
}

pub(in crate::scene::gui) fn emit(
    node: &Node,
    rect: Rect,
    cells: Option<&[Rect]>,
    context: Context,
    measure: &mut dyn TextMeasure,
    into: &mut Vec<Element>,
) {
    // Settled before the children are placed: an `AutomaticSize` text box
    // grows here, and its children resolve against the grown box.
    let mut rect = rect;
    let text = node
        .text
        .as_ref()
        .map(|text| text::typeset(text, &mut rect, measure));
    let corner_radii = modifiers::radii(node.corner.as_ref(), rect.size());
    let rounded = corner_radii.iter().any(|&radius| radius > 0.0);
    let start = into.len();
    into.push(Element {
        referent: node.referent,
        rect,
        clip: context.clip,
        rotation: context.angle + node.rotation,
        background: node.background,
        background_alpha: node.background_alpha,
        border: (node.border > 0.0 && !rounded).then_some((node.border, node.border_color)),
        border_inset: node.border_mode.inset(node.border),
        z_index: node.z_index,
        image: node.fill.as_ref().map(|fill| painted(fill, &rect)),
        corner_radii,
        strokes: node
            .strokes
            .iter()
            .map(|stroke| modifiers::stroke(stroke, rect.size()))
            .collect(),
        gradient: node
            .gradient
            .as_ref()
            .map(|gradient| modifiers::gradient(gradient, rect.size())),
        text,
        viewport: node.viewport.clone(),
        group: None,
        scroll: None,
    });

    // Roblox's own docs describe two modes here, gated on the (NotScriptable,
    // RolloutState) `StarterGui.ClipsDescendantsSupportsRotation`: enabled,
    // clipping works correctly against rotated shapes; not enabled — the
    // mode this models, since there is no scriptable way to read the flag at
    // all — a non-zero `Rotation` on this element or any ancestor makes
    // `ClipsDescendants` a no-op rather than clipping to a box that no longer
    // matches what is actually drawn on screen.
    let rotated = context.rotated || node.rotation != 0.0;
    let angle = context.angle + node.rotation;
    let inner = Context {
        clip: match node.clips && !rotated {
            true => Some(context.clip.map_or(rect, |outer| outer.intersect(&rect))),
            false => context.clip,
        },
        rotated,
        angle,
        // Once this element turns, its children turn about *its* centre.
        pivot: match angle == 0.0 {
            true => context.pivot,
            false => [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5],
        },
        ..context
    };
    match &node.scrolling {
        Some(scrolling) => {
            let window = scrolling::scroll(node, scrolling, &rect, cells, inner, measure, into);
            into[start].scroll = Some(ScrollWindow {
                referent: node.referent,
                rect: window.rect,
                clip: context.clip,
                range: [0, 1].map(
                    |axis| match scrolling.enabled && scrolling.direction[axis] {
                        true => (window.canvas[axis] - window.rect.size()[axis]).max(0.0),
                        false => 0.0,
                    },
                ),
            });
        }
        None => children(
            Scope {
                nodes: &node.children,
                groups: &node.groups,
                layout: node.list.as_ref(),
            },
            &sizing::padded(node, &rect),
            cells,
            inner,
            measure,
            into,
        ),
    }

    // "Descendants of `CanvasGroup` will be rendered as a flattened texture
    // only when the ancestor `LayerCollector` has its `ZIndexBehavior` set to
    // `Sibling`" — which is also the only mode that leaves them contiguous
    // behind the group here, since [`resolve`] re-sorts a `Global` screen.
    if let Some(tint) = node.group.filter(|_| !context.global_z_index) {
        into[start].group = Some(Grouped {
            tint,
            descendants: into.len() - start - 1,
            texture: None,
        });
    }
}

/// Where a run of `length` starts inside `extent` for one alignment.
pub(super) fn offset(align: Align, extent: f32, length: f32) -> f32 {
    match align {
        Align::Start => 0.0,
        Align::Center => (extent - length) * 0.5,
        Align::End => extent - length,
    }
}

/// Standard `UDim2` resolution: `extent` is the size every size modifier has
/// already had its say on, the position is a fraction of the parent plus a
/// pixel offset against the parent's own corner, and `AnchorPoint` then slides
/// the box back by that fraction of its own size — the default `(0, 0)`
/// putting the element's top-left corner on `Position`.
pub(super) fn place(position: Span, extent: [f32; 2], anchor: [f32; 2], parent: &Rect) -> Rect {
    let origin = position.against(parent.size());

    Rect {
        x: parent.x + origin[0] - anchor[0] * extent[0],
        y: parent.y + origin[1] - anchor[1] * extent[1],
        width: extent[0],
        height: extent[1],
    }
}
