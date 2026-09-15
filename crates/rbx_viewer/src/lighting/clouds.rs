//! `Clouds`, reduced to the four numbers the sky shader draws with.
//!
//! Entry point: [`read`]. Everything here is CPU-side; the GPU packing lives in
//! `renderer::lighting`.

use glam::Vec3;
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::{boolean, color, number};

const TERRAIN_CLASS: &str = "Terrain";
const CLOUDS_CLASS: &str = "Clouds";

// Studio's own defaults for a freshly inserted `Clouds` instance.
const DEFAULT_COVER: f32 = 0.5;
const DEFAULT_DENSITY: f32 = 0.5;

/// What the sky shader draws over the skybox.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Clouds {
    /// `Clouds.Cover`, 0 to 1. 0 draws nothing, same as `Enabled = false`.
    pub(crate) cover: f32,
    /// `Clouds.Density`: opacity and darkening of the layer.
    pub(crate) density: f32,
    pub(crate) color: Vec3,
}

/// The first enabled `Clouds` child of the place's `Terrain`, if any.
///
/// Roblox's own "Dynamic Clouds" guide is explicit that the object "only
/// render[s] if you parent [it] under the `Class.Terrain` class" — one is
/// otherwise inert wherever else it sits. `Terrain` itself is only ever
/// meaningful as a child of `Workspace` (Studio never lets it live anywhere
/// else), so looking it up through `workspace_descendants` rather than the
/// whole DOM is both correct and cheaper. `Enabled = false` collapses to
/// `None` rather than to `cover = 0`, which keeps "no clouds" a single case
/// for the GPU packing to fold back to zero.
pub(super) fn read(dom: &WeakDom, database: &ReflectionDatabase) -> Option<Clouds> {
    let terrain = crate::scene::workspace_descendants(dom, database).find(|&referent| {
        dom.get(referent)
            .is_some_and(|instance| database.is_subclass_of(instance.class(), TERRAIN_CLASS))
    })?;
    let children = dom.get(terrain)?.children();

    let properties = children.iter().find_map(|&child| {
        let instance = dom.get(child)?;
        if !database.is_subclass_of(instance.class(), CLOUDS_CLASS) {
            return None;
        }
        let properties = instance.properties();
        boolean(properties.get("Enabled"))
            .unwrap_or(true)
            .then_some(properties)
    })?;

    Some(Clouds {
        cover: number(properties.get("Cover"))
            .unwrap_or(DEFAULT_COVER)
            .clamp(0.0, 1.0),
        density: number(properties.get("Density"))
            .unwrap_or(DEFAULT_DENSITY)
            .clamp(0.0, 1.0),
        color: color(properties.get("Color")).unwrap_or(Vec3::ONE),
    })
}

#[cfg(test)]
#[path = "clouds/tests.rs"]
mod tests;
