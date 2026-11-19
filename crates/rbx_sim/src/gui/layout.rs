//! Where a client's `ScreenGui`s put their objects, in window pixels: our
//! own layout from the GuiObject, UDim2 and layout-object docs, for
//! `AbsolutePosition`/`AbsoluteSize` and for finding what the mouse is
//! over. Covers `Position`/`Size`/`AnchorPoint`/`SizeConstraint`,
//! `UIPadding`, `UIListLayout`, `UIGridLayout`, `UISizeConstraint`,
//! `UIAspectRatioConstraint`, `ScrollingFrame` canvases, `ClipsDescendants`,
//! `DisplayOrder` and both `ZIndexBehavior`s. The editor's renderer lays
//! the same trees out for drawing with its own, fuller engine.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// `GuiService:GetGuiInset()`'s top: the top bar's height (GuiRuntime
/// golden: `0,58 0,0`).
pub(crate) const TOPBAR_INSET: f32 = 58.0;

/// One laid out `ScreenGui` or `GuiObject`.
#[derive(Clone, Debug)]
pub(crate) struct Laid {
    pub referent: Ref,
    /// x, y, width, height in window pixels.
    pub rect: [f32; 4],
    /// The part of the window clipping ancestors leave it.
    pub clip: [f32; 4],
    /// Visible itself and all the way up, in an enabled screen.
    pub shown: bool,
    pub rotation: f32,
    pub screen: bool,
}

fn get(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> Option<Variant> {
    db.stored_or_default(dom.get(r)?, name)
        .map(|(_, v)| v.clone())
}

fn udim2(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> [f32; 4] {
    match get(dom, db, r, name) {
        Some(Variant::UDim2(u)) => [u.x.scale, u.x.offset as f32, u.y.scale, u.y.offset as f32],
        _ => [0.0; 4],
    }
}

fn udim(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> [f32; 2] {
    match get(dom, db, r, name) {
        Some(Variant::UDim(u)) => [u.scale, u.offset as f32],
        _ => [0.0; 2],
    }
}

fn vec2(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> [f32; 2] {
    match get(dom, db, r, name) {
        Some(Variant::Vector2(v)) => [v.x, v.y],
        _ => [0.0; 2],
    }
}

fn float(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> f32 {
    match get(dom, db, r, name) {
        Some(Variant::Float32(x)) => x,
        Some(Variant::Float64(x)) => x as f32,
        Some(Variant::Int32(x)) => x as f32,
        Some(Variant::Int64(x)) => x as f32,
        _ => 0.0,
    }
}

pub(crate) fn flag(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> bool {
    matches!(get(dom, db, r, name), Some(Variant::Bool(true)))
}

/// The item name an enum property holds, "" when unset.
pub(crate) fn item<'a>(
    dom: &WeakDom,
    db: &'a ReflectionDatabase,
    r: Ref,
    name: &str,
    enum_name: &str,
) -> &'a str {
    let Some(Variant::Enum(value)) = get(dom, db, r, name) else {
        return "";
    };
    db.enum_items(enum_name)
        .and_then(|items| items.iter().find(|(_, v)| *v == value))
        .map_or("", |(n, _)| n.as_str())
}

fn is(db: &ReflectionDatabase, dom: &WeakDom, r: Ref, class: &str) -> bool {
    dom.get(r)
        .is_some_and(|i| db.is_subclass_of(i.class(), class))
}

fn child_of(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, class: &str) -> Option<Ref> {
    dom.get(r)?
        .children()
        .iter()
        .copied()
        .find(|c| is(db, dom, *c, class))
}

fn intersect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
}

pub(crate) fn inside(rect: [f32; 4], p: [f32; 2]) -> bool {
    p[0] >= rect[0] && p[1] >= rect[1] && p[0] < rect[0] + rect[2] && p[1] < rect[1] + rect[3]
}

/// Every `ScreenGui` under `root` (a `PlayerGui`) and the objects in it,
/// in paint order: screens by `DisplayOrder`, then each screen's objects
/// bottom to top.
pub(crate) fn layout(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    root: Ref,
    viewport: [f32; 2],
) -> Vec<Laid> {
    let mut screens: Vec<(i64, Ref)> = dom
        .get(root)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| is(db, dom, *c, "ScreenGui"))
        .map(|c| (float(dom, db, c, "DisplayOrder") as i64, c))
        .collect();
    screens.sort_by_key(|(order, _)| *order);
    let window = [0.0, 0.0, viewport[0], viewport[1]];
    let mut out = Vec::new();
    for (_, screen) in screens {
        let ignore = flag(dom, db, screen, "IgnoreGuiInset")
            || item(dom, db, screen, "ScreenInsets", "ScreenInsets") == "None";
        let frame = if ignore {
            window
        } else {
            [0.0, TOPBAR_INSET, viewport[0], viewport[1] - TOPBAR_INSET]
        };
        let shown = flag(dom, db, screen, "Enabled");
        out.push(Laid {
            referent: screen,
            rect: frame,
            clip: window,
            shown,
            rotation: 0.0,
            screen: true,
        });
        let global = item(dom, db, screen, "ZIndexBehavior", "ZIndexBehavior") == "Global";
        let mut objects = Vec::new();
        children(
            dom,
            db,
            screen,
            Parent {
                content: frame,
                clip: window,
                shown,
                rotation: 0.0,
            },
            &mut objects,
        );
        if global {
            // Global: by ZIndex across the whole screen, tree order among
            // equals (a stable sort of the sibling-ordered list).
            objects.sort_by_key(|(z, _)| *z);
        }
        out.extend(objects.into_iter().map(|(_, laid)| laid));
    }
    out
}

#[derive(Clone, Copy)]
struct Parent {
    content: [f32; 4],
    clip: [f32; 4],
    shown: bool,
    rotation: f32,
}

/// Lays out `parent`'s GuiObject children against its content box, each
/// followed by its own children (Sibling order: by ZIndex, then child
/// order; children above their parent).
fn children(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    parent: Ref,
    within: Parent,
    out: &mut Vec<(i64, Laid)>,
) {
    let mut kids: Vec<Ref> = dom
        .get(parent)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| is(db, dom, *c, "GuiObject"))
        .collect();
    let [px, py, pw, ph] = within.content;
    let mut rects: Vec<(Ref, [f32; 4])> = kids
        .iter()
        .map(|&c| {
            let [w, h] = size_of(dom, db, c, [pw, ph]);
            let p = udim2(dom, db, c, "Position");
            let anchor = vec2(dom, db, c, "AnchorPoint");
            let x = px + pw * p[0] + p[1] - anchor[0] * w;
            let y = py + ph * p[2] + p[3] - anchor[1] * h;
            (c, [x, y, w, h])
        })
        .collect();
    if let Some(list) = child_of(dom, db, parent, "UIListLayout") {
        list_layout(dom, db, list, within.content, &mut rects);
    } else if let Some(grid) = child_of(dom, db, parent, "UIGridLayout") {
        grid_layout(dom, db, grid, within.content, &mut rects);
    }
    kids.sort_by_key(|c| float(dom, db, *c, "ZIndex") as i64);
    for c in kids {
        let Some(&(_, rect)) = rects.iter().find(|(r, _)| *r == c) else {
            continue;
        };
        let shown = within.shown && flag(dom, db, c, "Visible");
        let rotation = within.rotation + float(dom, db, c, "Rotation");
        out.push((
            float(dom, db, c, "ZIndex") as i64,
            Laid {
                referent: c,
                rect,
                clip: within.clip,
                shown,
                rotation,
                screen: false,
            },
        ));
        let clips = flag(dom, db, c, "ClipsDescendants");
        let clip = if clips {
            intersect(within.clip, rect)
        } else {
            within.clip
        };
        let mut content = padded(dom, db, c, rect);
        if is(db, dom, c, "ScrollingFrame") {
            let canvas = udim2(dom, db, c, "CanvasSize");
            let at = vec2(dom, db, c, "CanvasPosition");
            content = [
                content[0] - at[0],
                content[1] - at[1],
                content[2] * canvas[0] + canvas[1],
                content[3] * canvas[2] + canvas[3],
            ];
        }
        children(
            dom,
            db,
            c,
            Parent {
                content,
                clip,
                shown,
                rotation,
            },
            out,
        );
    }
}

/// `Size` against the parent's content box (`SizeConstraint`), then the
/// object's `UISizeConstraint` and `UIAspectRatioConstraint`.
fn size_of(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, parent: [f32; 2]) -> [f32; 2] {
    let s = udim2(dom, db, r, "Size");
    let [pw, ph] = match item(dom, db, r, "SizeConstraint", "SizeConstraint") {
        "RelativeXX" => [parent[0], parent[0]],
        "RelativeYY" => [parent[1], parent[1]],
        _ => parent,
    };
    let mut size = [pw * s[0] + s[1], ph * s[2] + s[3]];
    if let Some(limit) = child_of(dom, db, r, "UISizeConstraint") {
        let min = vec2(dom, db, limit, "MinSize");
        let max = vec2(dom, db, limit, "MaxSize");
        for i in 0..2 {
            size[i] = size[i].max(min[i]).min(max[i].max(min[i]));
        }
    }
    if let Some(aspect) = child_of(dom, db, r, "UIAspectRatioConstraint") {
        let ratio = float(dom, db, aspect, "AspectRatio").max(1e-6);
        let by_width = match item(dom, db, aspect, "DominantAxis", "DominantAxis") {
            "Height" => false,
            _ => {
                item(dom, db, aspect, "AspectType", "AspectType") == "ScaleWithParentSize"
                    || size[0] / size[1].max(1e-6) <= ratio
            }
        };
        if by_width {
            size[1] = size[0] / ratio;
        } else {
            size[0] = size[1] * ratio;
        }
    }
    size
}

/// `rect` less its `UIPadding`, each side a UDim of the rect's own size.
fn padded(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, rect: [f32; 4]) -> [f32; 4] {
    let Some(pad) = child_of(dom, db, r, "UIPadding") else {
        return rect;
    };
    let side = |name: &str, length: f32| {
        let u = udim(dom, db, pad, name);
        length * u[0] + u[1]
    };
    let left = side("PaddingLeft", rect[2]);
    let right = side("PaddingRight", rect[2]);
    let top = side("PaddingTop", rect[3]);
    let bottom = side("PaddingBottom", rect[3]);
    [
        rect[0] + left,
        rect[1] + top,
        (rect[2] - left - right).max(0.0),
        (rect[3] - top - bottom).max(0.0),
    ]
}

/// The siblings in the order a layout object sorts them: `LayoutOrder`
/// (then child order) or `Name`; visible ones only.
fn sorted(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    layout: Ref,
    rects: &[(Ref, [f32; 4])],
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rects.len())
        .filter(|&i| flag(dom, db, rects[i].0, "Visible"))
        .collect();
    if item(dom, db, layout, "SortOrder", "SortOrder") == "Name" {
        order.sort_by(|&a, &b| {
            let name = |i: usize| dom.get(rects[i].0).map(|x| x.name().to_owned());
            name(a).cmp(&name(b))
        });
    } else {
        order.sort_by_key(|&i| float(dom, db, rects[i].0, "LayoutOrder") as i64);
    }
    order
}

fn align(along: &str, free: f32) -> f32 {
    match along {
        "Center" => free / 2.0,
        "Right" | "Bottom" => free,
        _ => 0.0,
    }
}

fn list_layout(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    list: Ref,
    content: [f32; 4],
    rects: &mut [(Ref, [f32; 4])],
) {
    let vertical = item(dom, db, list, "FillDirection", "FillDirection") != "Horizontal";
    let (main, cross) = if vertical { (1, 0) } else { (0, 1) };
    let pad = udim(dom, db, list, "Padding");
    let gap = content[2 + main] * pad[0] + pad[1];
    let order = sorted(dom, db, list, rects);
    let total: f32 = order.iter().map(|&i| rects[i].1[2 + main]).sum::<f32>()
        + gap * order.len().saturating_sub(1) as f32;
    let h = item(dom, db, list, "HorizontalAlignment", "HorizontalAlignment");
    let v = item(dom, db, list, "VerticalAlignment", "VerticalAlignment");
    let (main_align, cross_align) = if vertical { (v, h) } else { (h, v) };
    let mut at = content[main] + align(main_align, content[2 + main] - total);
    for i in order {
        let rect = &mut rects[i].1;
        rect[main] = at;
        rect[cross] = content[cross] + align(cross_align, content[2 + cross] - rect[2 + cross]);
        at += rect[2 + main] + gap;
    }
}

fn grid_layout(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    grid: Ref,
    content: [f32; 4],
    rects: &mut [(Ref, [f32; 4])],
) {
    let cell = udim2(dom, db, grid, "CellSize");
    let pad = udim2(dom, db, grid, "CellPadding");
    let size = [
        content[2] * cell[0] + cell[1],
        content[3] * cell[2] + cell[3],
    ];
    let gap = [content[2] * pad[0] + pad[1], content[3] * pad[2] + pad[3]];
    let rows_first = item(dom, db, grid, "FillDirection", "FillDirection") != "Vertical";
    let (main, cross) = if rows_first { (0, 1) } else { (1, 0) };
    let fit = ((content[2 + main] + gap[main]) / (size[main] + gap[main]).max(1e-6))
        .floor()
        .max(1.0) as usize;
    for (n, i) in sorted(dom, db, grid, rects).into_iter().enumerate() {
        let rect = &mut rects[i].1;
        rect[2] = size[0];
        rect[3] = size[1];
        rect[main] = content[main] + (n % fit) as f32 * (size[main] + gap[main]);
        rect[cross] = content[cross] + (n / fit) as f32 * (size[cross] + gap[cross]);
    }
}

/// The objects under `point`, topmost first: shown, inside their box and
/// their clip. Screens themselves never count.
pub(crate) fn hits(laid: &[Laid], point: [f32; 2]) -> Vec<Ref> {
    laid.iter()
        .rev()
        .filter(|l| !l.screen && l.shown && inside(l.rect, point) && inside(l.clip, point))
        .map(|l| l.referent)
        .collect()
}
