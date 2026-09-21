//! `Highlight` support: reading the effect out of a DOM and working out which
//! parts each one covers.
//!
//! Pure data, like every other module beside [`Scene`](super::Scene) — the
//! silhouette itself is drawn in `renderer::highlight`, which takes nothing
//! from here but the list below.
//!
//! A `Highlight` is an `Instance`, not a `BasePart`, so it has no placement of
//! its own: it covers whatever its `Adornee` names, or — when that is unset,
//! which is the ordinary case — whatever it is parented to, and every
//! drawable descendant of that. Resolving those referents here rather than in
//! the renderer is the same split `renderer::selection` already relies on: the
//! GPU side only ever sees parts.

use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{descendants, descendants_of, is_drawable, srgb_to_linear};

const CLASS: &str = "Highlight";

/// Roblox's own ceiling, documented under "Limitations" in
/// `reference/engine/classes/Highlight`: a client displays 255 highlights at
/// once and silently ignores the rest. Disabled ones are documented to take a
/// slot too, but this list only ever holds enabled ones — a disabled
/// highlight draws nothing either way, so spending a slot on one here would
/// cost a visible highlight for no observable gain.
pub(crate) const MAX_HIGHLIGHTS: usize = 255;

/// Which side of the scene's depth a highlight is allowed to show on —
/// `Enum.HighlightDepthMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DepthMode {
    /// Drawn whatever stands between the camera and the object.
    AlwaysOnTop,
    /// Drawn only where the object itself is the nearest surface.
    Occluded,
}

/// One `Highlight`, resolved: the parts it covers and how it paints them.
///
/// Colours are linear, like every other colour a `Scene` hands the renderer
/// (see [`srgb_to_linear`]); the two alphas are `1 - Transparency`, the same
/// way [`Part::alpha`](super::Part) already inverts a part's.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Highlight {
    /// Every `BasePart` under the adornee, in [`descendants_of`]'s own walk
    /// order — which is a set as far as anything downstream is concerned, so
    /// the order is not part of what this promises. Never empty: a highlight
    /// covering nothing drawable is dropped by [`plan`], since there is no
    /// silhouette to trace.
    pub(crate) parts: Vec<Ref>,
    pub(crate) fill: [f32; 3],
    pub(crate) fill_alpha: f32,
    pub(crate) outline: [f32; 3],
    pub(crate) outline_alpha: f32,
    pub(crate) depth_mode: DepthMode,
    pub(crate) referent: Ref,
}

/// Every enabled `Highlight` in `dom` that covers something drawable, capped
/// at [`MAX_HIGHLIGHTS`].
///
/// The whole DOM rather than `workspace_descendants`: unlike a `Beam` or a
/// `ParticleEmitter`, a `Highlight` is routinely parented somewhere else
/// entirely — a `Tool`, `ReplicatedStorage`, the player's own `PlayerGui` —
/// and points at its target through `Adornee`. What has to be in the
/// Workspace is the *adornee*, which [`parts_under`] enforces by only keeping
/// referents the scene actually draws.
pub(crate) fn plan(dom: &WeakDom, database: &ReflectionDatabase) -> Vec<Highlight> {
    descendants(dom)
        .filter(|&referent| {
            dom.get(referent)
                .is_some_and(|instance| database.is_subclass_of(instance.class(), CLASS))
        })
        .filter_map(|referent| build(dom, database, referent))
        .take(MAX_HIGHLIGHTS)
        .collect()
}

fn build(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> Option<Highlight> {
    let instance = dom.get(referent)?;
    let properties = instance.properties();
    if !bool_or(properties, "Enabled", true) {
        return None;
    }

    // `Adornee` is documented as the way to apply the effect "outside of a
    // child/parent relationship", so an unset one falls back to the parent —
    // which is how all but a handful of real highlights are written.
    let adornee = match ref_or(properties, "Adornee") {
        Some(adornee) if dom.get(adornee).is_some() => adornee,
        _ => dom.parent(referent)?,
    };

    let parts = parts_under(dom, database, adornee);
    if parts.is_empty() {
        return None;
    }

    Some(Highlight {
        parts,
        fill: color_or(properties, "FillColor", WHITE),
        // Documented default: "any value between the default value of 0
        // (opaque) and 1 (invisible)". `OutlineTransparency`'s own default is
        // not documented anywhere Roblox publishes, and opaque is the value
        // its illustrations are drawn at, so both start opaque here.
        fill_alpha: 1.0 - float_or(properties, "FillTransparency", 0.0).clamp(0.0, 1.0),
        outline: color_or(properties, "OutlineColor", WHITE),
        outline_alpha: 1.0 - float_or(properties, "OutlineTransparency", 0.0).clamp(0.0, 1.0),
        depth_mode: depth_mode(properties),
        referent,
    })
}

/// `adornee` and every descendant of it the renderer draws.
///
/// Both, not just the descendants: a `Highlight` parented straight to a
/// `Part` is as ordinary as one parented to a `Model`, and the part itself is
/// then the only thing to trace.
fn parts_under(dom: &WeakDom, database: &ReflectionDatabase, adornee: Ref) -> Vec<Ref> {
    descendants_of(dom, adornee)
        .filter(|&referent| is_drawable(dom, database, referent))
        .collect()
}

/// Roblox's own default for both colours, per the illustrations in
/// `reference/engine/classes/Highlight` (a white outline over an untouched
/// object). The dump this project embeds carries no default values at all, so
/// this is read off the documentation rather than resolved.
const WHITE: [f32; 3] = [1.0, 1.0, 1.0];

/// `Enum.HighlightDepthMode`: `AlwaysOnTop` is 0 and `Occluded` is 1 (see
/// `assets/API-Dump.json`). Anything else — including a property stored as
/// something other than an enum — keeps the documented default, which is the
/// mode every illustration of the effect is drawn in.
fn depth_mode(properties: &BTreeMap<String, Variant>) -> DepthMode {
    match properties.get("DepthMode") {
        Some(&Variant::Enum(1)) => DepthMode::Occluded,
        _ => DepthMode::AlwaysOnTop,
    }
}

fn bool_or(properties: &BTreeMap<String, Variant>, key: &str, default: bool) -> bool {
    match properties.get(key) {
        Some(Variant::Bool(value)) => *value,
        _ => default,
    }
}

fn float_or(properties: &BTreeMap<String, Variant>, key: &str, default: f32) -> f32 {
    match properties.get(key) {
        Some(Variant::Float32(value)) if value.is_finite() => *value,
        Some(Variant::Float64(value)) if value.is_finite() => *value as f32,
        _ => default,
    }
}

fn ref_or(properties: &BTreeMap<String, Variant>, key: &str) -> Option<Ref> {
    match properties.get(key)? {
        Variant::Ref(referent) => Some(*referent),
        _ => None,
    }
}

/// A `Color3` property as linear channels. Both spellings are accepted for
/// the same reason `assemble_part` accepts `Color3uint8`: which one a file
/// carries depends on the writer, not on the property.
fn color_or(properties: &BTreeMap<String, Variant>, key: &str, default: [f32; 3]) -> [f32; 3] {
    let srgb = match properties.get(key) {
        Some(Variant::Color3(color)) => [color.r, color.g, color.b],
        Some(&Variant::Color3uint8 { r, g, b }) => {
            [r, g, b].map(|channel| f32::from(channel) / 255.0)
        }
        _ => default,
    };
    srgb.map(srgb_to_linear)
}

#[cfg(test)]
#[path = "highlight/tests.rs"]
mod tests;
