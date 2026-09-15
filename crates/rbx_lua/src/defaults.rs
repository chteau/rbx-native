//! Class defaults applied by `Instance.new`.
//!
//! Studio hands a freshly created instance a full set of default property
//! values; `rbx_reflection`'s database carries none (it only knows property
//! *types*, from the API dump). Without them a new `Part` has no `size` or
//! `CFrame`, so the viewer cannot draw it and a save sees a near-empty
//! instance. This is a small, hand-picked approximation of Studio's
//! defaults for the classes editing relies on today — not a full
//! reproduction of every default Studio would set.
//!
//! Keys match the file's own storage spelling (`size`, not `Size`) so they
//! round-trip through the DOM exactly like a value Studio would have saved.

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data};

use crate::ctx::Ctx;

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

// Enum ordinals below, from `assets/API-Dump.json`: `Material.Plastic` = 256,
// `PartType.Block` = 1.
const MATERIAL_PLASTIC: u32 = 256;
const PART_TYPE_BLOCK: u32 = 1;

fn base_part_defaults() -> [(&'static str, Variant); 11] {
    [
        (
            "size",
            Variant::Vector3(Vector3Data {
                x: 4.0,
                y: 1.2,
                z: 2.0,
            }),
        ),
        (
            "CFrame",
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: IDENTITY_ROTATION,
            }),
        ),
        (
            "Color3uint8",
            Variant::Color3uint8 {
                r: 163,
                g: 162,
                b: 165,
            },
        ),
        ("Material", Variant::Enum(MATERIAL_PLASTIC)),
        ("Anchored", Variant::Bool(false)),
        ("CanCollide", Variant::Bool(true)),
        ("Transparency", Variant::Float32(0.0)),
        ("Reflectance", Variant::Float32(0.0)),
        ("CastShadow", Variant::Bool(true)),
        ("Locked", Variant::Bool(false)),
        ("Massless", Variant::Bool(false)),
    ]
}

/// Sets `referent`'s hand-picked defaults for `class`, if any are known.
///
/// Every class the reflection database considers a `BasePart` (`Part`,
/// `WedgePart`, `SpawnLocation`, ...) gets the shared geometry/appearance
/// defaults above; `Part` additionally gets its `shape` (Studio always
/// creates blocks). Everything else (`Model`, `Folder`, services, ...) is
/// left untouched.
pub(crate) fn apply(ctx: &Ctx, class: &str, referent: Ref) {
    if !ctx.database().is_subclass_of(class, "BasePart") {
        return;
    }
    let mut dom = ctx.dom_mut();
    for (key, value) in base_part_defaults() {
        let _ = dom.set_property(referent, key, value);
    }
    if class == "Part" {
        let _ = dom.set_property(referent, "shape", Variant::Enum(PART_TYPE_BLOCK));
    }
}
