//! What a part is made of: the built-in texture pack behind its
//! `Enum.Material`, or the `MaterialVariant` a place substituted for it.
//!
//! Pure DOM extraction, like `scene::filemesh`: the assets named here are
//! downloaded by `crate::assets` and uploaded by `renderer::material`. Every
//! distinct material in a scene becomes one [`Slot`], whose `layer` is the
//! texture-array layer the renderer gives it.
//!
//! A `MeshPart` wearing a `SurfaceAppearance` still gets a slot here — its
//! `Kind` is what keeps Neon and ForceField procedural — but none of its maps
//! are projected: see `scene::filemesh::appearance`.

use std::collections::BTreeMap;
use std::collections::HashMap;

use rbx_assets::AssetRef;
use rbx_dom::{Variant, WeakDom};
use rbx_materials::{MapKind, Set};
use rbx_reflection::ReflectionDatabase;

use crate::assets::Image;
use crate::textures::asset_uri;

const MATERIAL_SERVICE: &str = "MaterialService";
const MATERIAL_VARIANT: &str = "MaterialVariant";
const MATERIAL_ENUM: &str = "Material";
/// What a part with no `Material` property at all is made of.
const DEFAULT_MATERIAL: &str = "Plastic";

/// How the fragment shader treats a material. The discriminants are what the
/// instance buffer carries, so they have to match the `KIND_*` constants in
/// `renderer/material.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Untextured: the part's own colour with Roblox's plastic constants, which
    /// is also where anything whose texture pack failed to download lands.
    Plastic = 0,
    Textured = 1,
    Neon = 2,
    ForceField = 3,
    Glass = 4,
}

/// One scene material, as the instance buffer carries it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Slot {
    pub(crate) layer: u32,
    pub(crate) kind: Kind,
    pub(crate) studs_per_tile: f32,
}

/// The maps of one material, indexed by [`MapKind::ALL`].
type Maps = [Option<AssetRef>; 4];

struct Def {
    kind: Kind,
    studs_per_tile: f32,
    maps: Maps,
}

/// `MaterialService`: which texture set the place asks for, the variants it
/// carries, and which built-in materials they have been substituted for.
struct Service {
    set: Set,
    /// Material name -> variant name, only where they differ (`WoodName =
    /// "MyWood"`); a service that leaves `WoodName = "Wood"` overrides nothing.
    overrides: HashMap<String, String>,
    variants: HashMap<String, Def>,
}

/// Every distinct material a scene uses, deduplicated into texture-array layers.
pub(crate) struct Catalog {
    service: Service,
    defs: Vec<Def>,
    images: HashMap<AssetRef, Image>,
}

impl Slot {
    /// The plain plastic every part falls back to: layer 0, which the renderer
    /// always fills with neutral maps.
    fn plastic() -> Self {
        Slot {
            layer: 0,
            kind: Kind::Plastic,
            studs_per_tile: rbx_materials::DEFAULT_STUDS_PER_TILE,
        }
    }
}

impl Catalog {
    pub(crate) fn new(dom: &WeakDom, database: &ReflectionDatabase) -> Self {
        Catalog {
            service: service(dom, database),
            // Layer 0 is plastic, so an empty scene still has the fallback every
            // unresolved part points at.
            defs: vec![Def {
                kind: Kind::Plastic,
                studs_per_tile: rbx_materials::DEFAULT_STUDS_PER_TILE,
                maps: Maps::default(),
            }],
            images: HashMap::new(),
        }
    }

    /// The slot one part's properties ask for, adding a layer if this material
    /// has not been seen yet.
    pub(crate) fn slot_for(
        &mut self,
        properties: &BTreeMap<String, Variant>,
        database: &ReflectionDatabase,
    ) -> Slot {
        let def = self.define(properties, database);
        let layer = match self.defs.iter().position(|known| same(known, &def)) {
            Some(layer) => layer,
            None => {
                self.defs.push(def);
                self.defs.len() - 1
            }
        };

        self.slot(u32::try_from(layer).unwrap_or(0))
    }

    pub(crate) fn slot(&self, layer: u32) -> Slot {
        let Some(def) = self.defs.get(layer as usize) else {
            return Slot::plastic();
        };

        Slot {
            layer,
            kind: def.kind,
            studs_per_tile: def.studs_per_tile,
        }
    }

    /// Every map every layer needs, in first-seen order and without duplicates.
    pub(crate) fn asset_refs(&self) -> Vec<AssetRef> {
        let mut refs: Vec<AssetRef> = Vec::new();
        for reference in self.defs.iter().flat_map(|def| def.maps.iter().flatten()) {
            if !refs.contains(reference) {
                refs.push(reference.clone());
            }
        }
        refs
    }

    /// Joins the layers to whatever downloaded. A textured layer left without a
    /// colour map falls back to plastic — with `--no-materials`, or with no
    /// network, that is every one of them.
    pub(crate) fn resolve(&mut self, images: HashMap<AssetRef, Image>) {
        for def in &mut self.defs {
            let resolved = def
                .maps
                .iter()
                .flatten()
                .any(|reference| images.contains_key(reference));
            if def.kind == Kind::Textured && !resolved {
                def.kind = Kind::Plastic;
            }
        }
        self.images = images;
    }

    pub(crate) fn layers(&self) -> usize {
        self.defs.len()
    }

    /// The image of one layer's map, `None` where the material has no such map
    /// or it failed to download.
    pub(crate) fn image(&self, layer: usize, kind: MapKind) -> Option<&Image> {
        let reference = self.defs.get(layer)?.maps[kind.index()].as_ref()?;
        self.images.get(reference)
    }

    /// Builds the definition a part's `Material`/`MaterialVariantSerialized`
    /// pair stands for, without adding it to the catalog.
    fn define(&self, properties: &BTreeMap<String, Variant>, database: &ReflectionDatabase) -> Def {
        let name = match properties.get(MATERIAL_ENUM) {
            Some(&Variant::Enum(value)) => database
                .enum_name(MATERIAL_ENUM, value)
                .unwrap_or(DEFAULT_MATERIAL),
            _ => DEFAULT_MATERIAL,
        };

        // A part naming a variant wins over the service-wide substitution, which
        // is what Studio does: `MaterialVariantSerialized` is per part.
        let named = properties
            .get("MaterialVariantSerialized")
            .and_then(asset_uri)
            .filter(|text| !text.is_empty());
        let variant = named
            .or_else(|| self.service.overrides.get(name).map(String::as_str))
            .and_then(|variant| self.service.variants.get(variant));

        match variant {
            Some(def) => Def {
                kind: def.kind,
                studs_per_tile: def.studs_per_tile,
                maps: def.maps.clone(),
            },
            None => builtin(name, self.service.set),
        }
    }
}

/// The definition of a built-in material, which is a row of [`rbx_materials`]
/// turned into asset references.
fn builtin(name: &str, set: Set) -> Def {
    let Some(material) = rbx_materials::material(name, set) else {
        return Def {
            kind: Kind::Plastic,
            studs_per_tile: rbx_materials::DEFAULT_STUDS_PER_TILE,
            maps: Maps::default(),
        };
    };

    let kind = match material.flavor() {
        rbx_materials::Flavor::Neon => Kind::Neon,
        rbx_materials::Flavor::ForceField => Kind::ForceField,
        rbx_materials::Flavor::Glass => Kind::Glass,
        // A colour map, not "any map at all", is what makes a material textured:
        // `Plastic` ships a lone normal map, and shading it from that map's
        // colour instead of the part's `Color` would repaint every plastic part.
        rbx_materials::Flavor::Solid if material.map(MapKind::Color).is_none() => Kind::Plastic,
        rbx_materials::Flavor::Solid => Kind::Textured,
    };

    Def {
        kind,
        studs_per_tile: material.studs_per_tile(),
        maps: MapKind::ALL.map(|map| material.map(map).map(AssetRef::Id)),
    }
}

/// Reads `MaterialService`, which a place file may leave out entirely — in
/// which case every part is a built-in 2022 material.
fn service(dom: &WeakDom, database: &ReflectionDatabase) -> Service {
    let mut service = Service {
        set: Set::Current,
        overrides: HashMap::new(),
        variants: HashMap::new(),
    };

    let Some(referent) = super::descendants(dom).find(|&referent| {
        dom.get(referent)
            .is_some_and(|i| i.class() == MATERIAL_SERVICE)
    }) else {
        return service;
    };
    let Some(instance) = dom.get(referent) else {
        return service;
    };

    if let Some(&Variant::Bool(false)) = instance.properties().get("Use2022MaterialsXml") {
        service.set = Set::Legacy;
    }
    for (property, value) in instance.properties() {
        let (Some(material), Variant::String(variant)) = (property.strip_suffix("Name"), value)
        else {
            continue;
        };
        // The default value of `WoodName` is "Wood": a substitution is only one
        // when it names something else.
        if variant != material && !variant.is_empty() {
            service
                .overrides
                .insert(material.to_string(), variant.clone());
        }
    }

    for &child in instance.children() {
        let Some(child_instance) = dom.get(child) else {
            continue;
        };
        if child_instance.class() != MATERIAL_VARIANT {
            continue;
        }
        service.variants.insert(
            child_instance.name().to_string(),
            variant(child_instance.properties(), database),
        );
    }

    service
}

/// One `MaterialVariant`: its own maps over its `BaseMaterial`'s flavour.
///
/// Maps it leaves empty stay empty rather than falling back to the base
/// material's own pack — Studio requires a colour and a normal map to create
/// one, so a missing map is the author saying "none", not "inherit".
fn variant(properties: &BTreeMap<String, Variant>, database: &ReflectionDatabase) -> Def {
    let base = match properties.get("BaseMaterial") {
        Some(&Variant::Enum(value)) => database
            .enum_name(MATERIAL_ENUM, value)
            .unwrap_or(DEFAULT_MATERIAL),
        _ => DEFAULT_MATERIAL,
    };
    let studs_per_tile = match properties.get("StudsPerTile") {
        Some(&Variant::Float32(studs)) if studs > 0.0 => studs,
        Some(&Variant::Float64(studs)) if studs > 0.0 => studs as f32,
        _ => rbx_materials::DEFAULT_STUDS_PER_TILE,
    };
    let maps = MapKind::ALL.map(|kind| {
        properties
            .get(property_of(kind))
            .and_then(asset_uri)
            .and_then(|uri| AssetRef::parse(uri).ok())
            .filter(|reference| *reference != AssetRef::Empty)
    });

    Def {
        // A variant is textured by definition; only its base material's special
        // flavours (Neon, ForceField, Glass) survive the substitution.
        kind: match builtin(base, Set::Current).kind {
            Kind::Plastic | Kind::Textured => Kind::Textured,
            flavored => flavored,
        },
        studs_per_tile,
        maps,
    }
}

/// `pub(super)`: a `SurfaceAppearance` serializes its own four maps under
/// exactly these names, and `scene::filemesh::appearance` reads them.
pub(super) fn property_of(kind: MapKind) -> &'static str {
    match kind {
        MapKind::Color => "ColorMap",
        MapKind::Normal => "NormalMap",
        MapKind::Metalness => "MetalnessMap",
        MapKind::Roughness => "RoughnessMap",
    }
}

/// Two definitions share a layer when they would upload the same texels and
/// shade identically.
fn same(left: &Def, right: &Def) -> bool {
    left.kind == right.kind
        && left.studs_per_tile.to_bits() == right.studs_per_tile.to_bits()
        && left.maps == right.maps
}

#[cfg(test)]
#[path = "material/tests.rs"]
mod tests;
