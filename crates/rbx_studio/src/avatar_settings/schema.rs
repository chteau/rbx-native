//! What each control of the dock reads and writes: Studio's defaults for
//! the properties `reflection-defaults.json` predates, the asset slots
//! (a checkbox and an ID), and the slider bounds.

use rbx_dom::{NumberRange, Variant, Vector3Data, WeakDom};

use super::{Rule, Rules, Writes};

/// Studio's value for a property a file leaves out, read off a place it
/// saved with nothing changed.
pub(super) fn default(name: &str) -> Variant {
    let range = |min, max| Variant::NumberRange(NumberRange { min, max });
    match name {
        "CustomWorldHeight" | "CustomHeight" => range(5.5, 5.5),
        "CustomBuildHeight" | "CustomHeightScale" => range(0.9, 1.05),
        "CustomBuildWidth" | "CustomWidthScale" => range(0.7, 1.0),
        "CustomBuildHead" | "CustomHeadScale" => range(0.95, 1.0),
        "CustomBuildBodyType"
        | "CustomBodyTypeScale"
        | "CustomBuildProportions"
        | "CustomProportionsScale" => range(0.0, 1.0),
        "SingleColliderSize" => Variant::Vector3(Vector3Data {
            x: 2.0,
            y: 3.0,
            z: 1.0,
        }),
        "LimitBounds" => super::vector([0.0; 3]),
        "AvatarType" | "LegacyCollisionMode" | "LimitMethod" => Variant::Enum(1),
        _ if name.starts_with("Enable") || name == "KeepPlayerHead" => Variant::Bool(true),
        _ if name.ends_with("Enabled") => Variant::Bool(false),
        _ if name.ends_with("Override") || name.ends_with("Id") => Variant::Int64(0),
        _ => Variant::Enum(0),
    }
}

/// One asset row: its label and the asset kinds it covers. A row covering
/// several kinds (Tops is shirts, T-shirts and sweaters) ticks them all,
/// and its ID lands on the kind matching the asset's type.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) label: &'static str,
    rule: Rule,
    kinds: &'static [Kind],
    /// Whether a ticked row may stay empty, which removes that category
    /// (clothing and accessories); a body part or a clip needs an ID.
    pub(crate) blank_removes: bool,
}

/// One asset kind: the `…Enabled`/`…Id` pair Studio saves it in (the
/// API dump marks them deprecated, yet a place Studio saved keeps every
/// value there), the `…Override` the schema adds for it, which Studio
/// leaves at 0, and the catalog asset type an ID of this kind has.
#[derive(Debug, Clone, Copy)]
struct Kind {
    enabled: &'static str,
    id: &'static str,
    current: Option<&'static str>,
    asset_type: u32,
}

/// A slot's state: unticked, ticked and empty (remove), or an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotValue {
    Off,
    Remove,
    Asset(i64),
}

impl Slot {
    /// An `…Override` holding an ID wins, for a file written by a Studio
    /// that has moved to them; otherwise the first ticked kind with an ID.
    pub(crate) fn read(&self, rules: &Rules, dom: &WeakDom) -> SlotValue {
        let mut ticked = false;
        for kind in self.kinds {
            let current = kind
                .current
                .map_or(0, |name| rules.int_of(dom, (self.rule, name)));
            if current > 0 {
                return SlotValue::Asset(current);
            }
            if rules.bool_of(dom, (self.rule, kind.enabled)) {
                ticked = true;
                let id = rules.int_of(dom, (self.rule, kind.id));
                if id > 0 {
                    return SlotValue::Asset(id);
                }
            }
        }
        if ticked {
            SlotValue::Remove
        } else {
            SlotValue::Off
        }
    }

    /// Whether an ID's placement depends on its asset type.
    pub(crate) fn typed(&self) -> bool {
        self.kinds.len() > 1
    }

    /// What Studio saves for `value`: every kind ticked unless `Off`, the
    /// ID on the kind of `asset_type` (the row's first when unknown), and
    /// every `…Override` at 0.
    pub(crate) fn write(&self, value: SlotValue, asset_type: Option<u32>) -> Writes {
        let target = self
            .kinds
            .iter()
            .position(|kind| Some(kind.asset_type) == asset_type)
            .unwrap_or(0);
        let key = |name| (self.rule, name);
        let mut writes = Writes::new();
        for (index, kind) in self.kinds.iter().enumerate() {
            let id = match value {
                SlotValue::Asset(id) if index == target => id,
                _ => 0,
            };
            writes.push((key(kind.enabled), Variant::Bool(value != SlotValue::Off)));
            writes.push((key(kind.id), Variant::Int64(id)));
            if let Some(current) = kind.current {
                writes.push((key(current), Variant::Int64(0)));
            }
        }
        writes
    }
}

const fn kind(
    enabled: &'static str,
    id: &'static str,
    current: Option<&'static str>,
    asset_type: u32,
) -> Kind {
    Kind {
        enabled,
        id,
        current,
        asset_type,
    }
}

const fn slot(
    label: &'static str,
    rule: Rule,
    kinds: &'static [Kind],
    blank_removes: bool,
) -> Slot {
    Slot {
        label,
        rule,
        kinds,
        blank_removes,
    }
}

/// A kind named `Custom<legacy>…` with an optional current property.
macro_rules! kind {
    ($legacy:literal, $suffix:literal, $current:expr, $asset_type:expr) => {
        kind(
            concat!("Custom", $legacy, $suffix, "Enabled"),
            concat!("Custom", $legacy, $suffix, "Id"),
            $current,
            $asset_type,
        )
    };
}

macro_rules! body {
    ($label:literal, $legacy:literal, $current:expr) => {
        slot(
            $label,
            Rule::Body,
            &[kind!($legacy, "", $current, 0)],
            false,
        )
    };
}

/// Body › Custom Parts, in Studio's order. Face and Mood have no current
/// property.
pub(crate) const BODY_SLOTS: [Slot; 8] = [
    body!("Face", "Face", None),
    body!("Head", "Head", Some("HeadOverride")),
    body!("Mood", "Mood", None),
    body!("Torso", "Torso", Some("TorsoOverride")),
    body!("Left Arm", "LeftArm", Some("LeftArmOverride")),
    body!("Right Arm", "RightArm", Some("RightArmOverride")),
    body!("Left Leg", "LeftLeg", Some("LeftLegOverride")),
    body!("Right Leg", "RightLeg", Some("RightLegOverride")),
];

/// A clothing kind: its `Custom<name>Accessory…` pair, `<name>AccessoryOverride`
/// and catalog asset type.
macro_rules! cloth {
    ($name:literal, $asset_type:expr) => {
        kind!(
            $name,
            "Accessory",
            Some(concat!($name, "AccessoryOverride")),
            $asset_type
        )
    };
}

/// Clothing › Custom Clothing. Studio groups layered clothing the way the
/// avatar editor does: Tops (shirts, T-shirts, sweaters), Outerwear
/// (jackets), Bottoms (pants, shorts, dresses and skirts). Asset types are
/// Roblox's `Enum.AssetType` values.
pub(crate) const CLOTHING_SLOTS: [Slot; 8] = [
    slot(
        "Tops",
        Rule::Clothing,
        &[
            cloth!("Shirt", 65),
            cloth!("TShirt", 64),
            cloth!("Sweater", 68),
        ],
        true,
    ),
    slot("Outerwear", Rule::Clothing, &[cloth!("Jacket", 67)], true),
    slot(
        "Bottoms",
        Rule::Clothing,
        &[
            cloth!("Pants", 66),
            cloth!("Shorts", 69),
            cloth!("DressSkirt", 72),
        ],
        true,
    ),
    slot(
        "Left Shoes",
        Rule::Clothing,
        &[cloth!("LeftShoes", 70)],
        true,
    ),
    slot(
        "Right Shoes",
        Rule::Clothing,
        &[cloth!("RightShoes", 71)],
        true,
    ),
    slot(
        "Classic Shirts",
        Rule::Clothing,
        &[cloth!("ClassicShirts", 11)],
        true,
    ),
    slot(
        "Classic T-Shirts",
        Rule::Clothing,
        &[cloth!("ClassicTShirts", 2)],
        true,
    ),
    slot(
        "Classic Pants",
        Rule::Clothing,
        &[cloth!("ClassicPants", 12)],
        true,
    ),
];

macro_rules! accessory {
    ($label:literal, $kind:literal) => {
        slot($label, Rule::Accessory, &[cloth!($kind, 0)], true)
    };
}

/// Accessories › Custom Accessories.
pub(crate) const ACCESSORY_SLOTS: [Slot; 8] = [
    accessory!("Hair", "Hair"),
    accessory!("Head", "Head"),
    accessory!("Face", "Face"),
    accessory!("Neck", "Neck"),
    accessory!("Shoulder", "Shoulder"),
    accessory!("Front", "Front"),
    accessory!("Back", "Back"),
    accessory!("Waist", "Waist"),
];

macro_rules! clip {
    ($label:literal, $current:expr, $legacy:literal) => {
        slot(
            $label,
            Rule::Animation,
            &[kind!($legacy, "Animation", $current, 0)],
            false,
        )
    };
}

/// Movement › Custom Clips. Studio lists two alternate idles; the current
/// schema has one property for the first and none for the second.
pub(crate) const CLIP_SLOTS: [Slot; 10] = [
    clip!("Run", Some("RunAnimationOverride"), "Run"),
    clip!("Walk", Some("WalkAnimationOverride"), "Walk"),
    clip!("Fall", Some("FallAnimationOverride"), "Fall"),
    clip!("Jump", Some("JumpAnimationOverride"), "Jump"),
    clip!("Idle", Some("IdleAnimationOverride"), "Idle"),
    clip!("Idle Alt 1", Some("IdleAltAnimationOverride"), "IdleAlt1"),
    clip!("Idle Alt 2", None, "IdleAlt2"),
    clip!("Swim", Some("SwimAnimationOverride"), "Swim"),
    clip!("Swim Idle", Some("SwimIdleAnimationOverride"), "SwimIdle"),
    clip!("Climb", Some("ClimbAnimationOverride"), "Climb"),
];

/// The catalog asset type of `id`, from Roblox's public economy API (no
/// key needed), as Studio looks it up to place a Tops or Bottoms ID;
/// `None` when offline or unknown.
pub(crate) fn asset_type(id: i64) -> Option<u32> {
    let url = format!("https://economy.roblox.com/v2/assets/{id}/details");
    let details: serde_json::Value = rbx_net::agent()
        .get(&url)
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;
    details.get("AssetTypeId")?.as_u64()?.try_into().ok()
}
