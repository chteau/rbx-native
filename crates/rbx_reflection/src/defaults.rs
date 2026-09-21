//! What the API dump leaves out: the value each property of a freshly created
//! instance holds, and which properties a file stores under a name of their
//! own (`BasePart.Size` as `size`, `BasePart.Color` as `Color3uint8`).
//!
//! Read from `assets/reflection-defaults.json`, which
//! `scripts/reflection-defaults.sh` extracts from rbx-dom's reflection
//! database (MIT). rbx-dom generates that database from Studio itself; the
//! API dump records only types.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use rbx_dom::{
    Axes, CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, Faces, Font,
    FontStyle, NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, UDim,
    UDim2, Variant, Vector2Data, Vector3Data,
};
use serde::Deserialize;
use serde_json::Value;

use crate::database::ReflectionDatabase;

#[derive(Debug, Default)]
pub(crate) struct Defaults {
    classes: HashMap<String, ClassDefaults>,
    /// Each class's [`Rename`]s, worked out the first time a class is asked
    /// for: a place holds thousands of instances of a few dozen classes,
    /// and reading one renames each of them.
    renames: RwLock<HashMap<String, Arc<[Rename]>>>,
}

/// A spelling a file may use for a property, the name Roblox saves it
/// under, and the property itself — `("Color", "Color3uint8", "Color")`,
/// or `("size", "size_xml", "Size")` for a `Fire`.
#[derive(Debug, Clone)]
pub(crate) struct Rename {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) canonical: String,
}

#[derive(Debug, Default, Deserialize)]
struct ClassDefaults {
    /// A name a file may store, and the property it is a spelling of.
    #[serde(rename = "Aliases", default)]
    aliases: HashMap<String, String>,
    /// A property, and the name Roblox saves it under.
    #[serde(rename = "SerializesAs", default)]
    serializes_as: HashMap<String, String>,
    #[serde(skip)]
    values: HashMap<String, Variant>,
    #[serde(rename = "Defaults", default)]
    raw_values: HashMap<String, Value>,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(rename = "Classes")]
    classes: HashMap<String, ClassDefaults>,
}

impl Defaults {
    /// `weight` names a `FontWeight` member's value — the file spells a
    /// font's weight by name, the DOM stores the number.
    pub(crate) fn parse(
        json: &str,
        weight: impl Fn(&str) -> Option<u16>,
    ) -> Result<Self, serde_json::Error> {
        let raw: RawFile = serde_json::from_str(json)?;
        let mut classes = raw.classes;
        for class in classes.values_mut() {
            // A value this conversion does not understand costs that one
            // property its default, never the rest of the file.
            class.values = std::mem::take(&mut class.raw_values)
                .into_iter()
                .filter_map(|(name, value)| Some((name, variant(value, &weight)?)))
                .collect();
        }
        Ok(Defaults {
            classes,
            renames: RwLock::default(),
        })
    }
}

impl ReflectionDatabase {
    /// The value `property` holds on a freshly created `class`, as Studio
    /// reports it. Only a class Studio can create has any: an abstract
    /// `BasePart` holds nothing, while every `Part` default — inherited ones
    /// included — is recorded under `Part` itself.
    pub fn default_value(&self, class: &str, property: &str) -> Option<&Variant> {
        self.defaults.classes.get(class)?.values.get(property)
    }

    /// The property `name` is a spelling of: `Size` for a stored `size`,
    /// `Color` for `Color3uint8`. Any other name is its own.
    pub fn canonical_name<'a>(&'a self, class: &str, name: &'a str) -> &'a str {
        self.lineage(class)
            .find_map(|class| self.defaults.classes.get(class)?.aliases.get(name))
            .map_or(name, String::as_str)
    }

    /// Every name a file may hold `name`'s value under, the one Roblox saves
    /// first — `["size", "Size"]` for `BasePart.Size`. A value that has none
    /// of these stored is its class default.
    pub fn stored_names<'a>(&'a self, class: &str, name: &'a str) -> Vec<&'a str> {
        let canonical = self.canonical_name(class, name);
        let mut names: Vec<&str> = self
            .lineage(class)
            .find_map(|class| {
                self.defaults
                    .classes
                    .get(class)?
                    .serializes_as
                    .get(canonical)
            })
            .map(String::as_str)
            .into_iter()
            .chain([canonical])
            .collect();
        let mut aliases: Vec<&str> = self
            .lineage(class)
            .filter_map(|class| self.defaults.classes.get(class))
            .flat_map(|class| &class.aliases)
            .filter(|(alias, target)| *target == canonical && !names.contains(&alias.as_str()))
            .map(|(alias, _)| alias.as_str())
            .collect();
        // Sorted only so two runs agree: the map's own order is random.
        aliases.sort_unstable();
        aliases.dedup();
        names.extend(aliases);
        names
    }

    /// Every spelling a file may use for a property of `class` other than
    /// the one Roblox saves, the nearest class's first where two name one
    /// spelling.
    pub(crate) fn renames(&self, class: &str) -> Arc<[Rename]> {
        if let Some(renames) = self
            .defaults
            .renames
            .read()
            .ok()
            .and_then(|renames| renames.get(class).cloned())
        {
            return renames;
        }
        let renames: Arc<[Rename]> = self
            .build_renames(class)
            .into_iter()
            .map(|(from, to, canonical)| Rename {
                from: from.to_owned(),
                to: to.to_owned(),
                canonical: canonical.to_owned(),
            })
            .collect();
        if let Ok(mut memo) = self.defaults.renames.write() {
            memo.insert(class.to_owned(), Arc::clone(&renames));
        }
        renames
    }

    fn build_renames(&self, class: &str) -> Vec<(&str, &str, &str)> {
        let mut renames: Vec<(&str, &str, &str)> = Vec::new();
        for class_defaults in self
            .lineage(class)
            .filter_map(|class| self.defaults.classes.get(class))
        {
            let mut spellings: Vec<(&str, &str)> = class_defaults
                .serializes_as
                .keys()
                .map(|canonical| (canonical.as_str(), canonical.as_str()))
                .chain(
                    class_defaults
                        .aliases
                        .iter()
                        .map(|(alias, canonical)| (alias.as_str(), canonical.as_str())),
                )
                .collect();
            // The first rename to reach a saved name keeps its value, so the
            // maps' random order would pick a different one per run where a
            // file holds two spellings of one saved name (`MaxDistance` and
            // `RollOffMaxDistance`): the current property first, then by name.
            spellings.sort_unstable_by_key(|&(spelling, canonical)| {
                let deprecated = self
                    .resolve_property(class, canonical)
                    .is_some_and(|property| property.is_deprecated());
                (deprecated, spelling)
            });
            for (spelling, canonical) in spellings {
                let Some(&saved) = self.stored_names(class, canonical).first() else {
                    continue;
                };
                if spelling != saved && !renames.iter().any(|(from, ..)| *from == spelling) {
                    renames.push((spelling, saved, canonical));
                }
            }
        }
        renames
    }
}

#[derive(Deserialize)]
struct RawCFrame {
    position: [f32; 3],
    /// Rows, the way the DOM's own row-major `rotation` lays them out.
    orientation: [[f32; 3]; 3],
}

#[derive(Deserialize)]
struct RawSequence<K> {
    keypoints: Vec<K>,
}

#[derive(Deserialize)]
struct RawNumberKeypoint {
    time: f32,
    value: f32,
    envelope: f32,
}

#[derive(Deserialize)]
struct RawColorKeypoint {
    time: f32,
    color: [f32; 3],
}

#[derive(Deserialize)]
struct RawFont {
    family: String,
    weight: String,
    style: String,
    #[serde(rename = "cachedFaceId")]
    cached_face_id: Option<String>,
}

#[derive(Deserialize)]
struct RawRay {
    origin: [f32; 3],
    direction: [f32; 3],
}

/// rbx-dom's own spelling of a value, one variant per type the generator
/// keeps (see `scripts/reflection-defaults.sh`).
#[derive(Deserialize)]
enum Raw {
    Bool(bool),
    Int32(i32),
    Int64(i64),
    Float32(f32),
    Float64(f64),
    String(String),
    Enum(u32),
    BrickColor(u32),
    SecurityCapabilities(u64),
    Color3([f32; 3]),
    Color3uint8([u8; 3]),
    Vector2([f32; 2]),
    Vector3([f32; 3]),
    Vector3int16([i16; 3]),
    CFrame(RawCFrame),
    OptionalCFrame(Option<RawCFrame>),
    UDim((f32, i32)),
    UDim2([(f32, i32); 2]),
    Rect([[f32; 2]; 2]),
    NumberRange([f32; 2]),
    NumberSequence(RawSequence<RawNumberKeypoint>),
    ColorSequence(RawSequence<RawColorKeypoint>),
    PhysicalProperties(Value),
    Font(RawFont),
    Faces(Vec<String>),
    Axes(Vec<String>),
    Ray(RawRay),
    Content(Value),
}

fn variant(value: Value, weight: &impl Fn(&str) -> Option<u16>) -> Option<Variant> {
    Some(match serde_json::from_value::<Raw>(value).ok()? {
        Raw::Bool(value) => Variant::Bool(value),
        Raw::Int32(value) => Variant::Int32(value),
        Raw::Int64(value) => Variant::Int64(value),
        Raw::Float32(value) => Variant::Float32(value),
        Raw::Float64(value) => Variant::Float64(value),
        Raw::String(value) => Variant::String(value),
        Raw::Enum(value) => Variant::Enum(value),
        Raw::BrickColor(value) => Variant::BrickColor(value),
        Raw::SecurityCapabilities(value) => Variant::SecurityCapabilities(value),
        Raw::Color3(rgb) => Variant::Color3(color3(rgb)),
        Raw::Color3uint8([r, g, b]) => Variant::Color3uint8 { r, g, b },
        Raw::Vector2(xy) => Variant::Vector2(vector2(xy)),
        Raw::Vector3(xyz) => Variant::Vector3(vector3(xyz)),
        Raw::Vector3int16([x, y, z]) => Variant::Vector3int16 { x, y, z },
        Raw::CFrame(frame) => Variant::CFrame(cframe(frame)),
        Raw::OptionalCFrame(frame) => Variant::OptionalCFrame(frame.map(cframe)),
        Raw::UDim(udim) => Variant::UDim(self::udim(udim)),
        Raw::UDim2([x, y]) => Variant::UDim2(UDim2 {
            x: udim(x),
            y: udim(y),
        }),
        Raw::Rect([min, max]) => Variant::Rect(Rect {
            min: vector2(min),
            max: vector2(max),
        }),
        Raw::NumberRange([min, max]) => Variant::NumberRange(NumberRange { min, max }),
        Raw::NumberSequence(sequence) => Variant::NumberSequence(NumberSequence {
            keypoints: sequence
                .keypoints
                .into_iter()
                .map(|k| NumberSequenceKeypoint {
                    time: k.time,
                    value: k.value,
                    envelope: k.envelope,
                })
                .collect(),
        }),
        Raw::ColorSequence(sequence) => Variant::ColorSequence(ColorSequence {
            keypoints: sequence
                .keypoints
                .into_iter()
                .map(|k| ColorSequenceKeypoint {
                    time: k.time,
                    color: color3(k.color),
                    envelope: 0.0,
                })
                .collect(),
        }),
        // `Default` is the only form a new instance is ever created with;
        // anything else is left for the caller to go without.
        Raw::PhysicalProperties(Value::String(form)) if form == "Default" => {
            Variant::PhysicalProperties(PhysicalProperties::Default)
        }
        Raw::PhysicalProperties(_) => return None,
        Raw::Font(font) => Variant::Font(Font {
            family: font.family,
            weight: weight(&font.weight)?,
            style: match font.style.as_str() {
                "Normal" => FontStyle::Normal,
                "Italic" => FontStyle::Italic,
                _ => return None,
            },
            cached_face_id: font.cached_face_id.filter(|id| !id.is_empty()),
        }),
        Raw::Faces(names) => {
            let has = |face: &str| names.iter().any(|name| name == face);
            Variant::Faces(Faces {
                front: has("Front"),
                bottom: has("Bottom"),
                left: has("Left"),
                back: has("Back"),
                top: has("Top"),
                right: has("Right"),
            })
        }
        Raw::Axes(names) => {
            let has = |axis: &str| names.iter().any(|name| name == axis);
            Variant::Axes(Axes {
                x: has("X"),
                y: has("Y"),
                z: has("Z"),
            })
        }
        Raw::Ray(ray) => Variant::Ray {
            origin: vector3(ray.origin),
            direction: vector3(ray.direction),
        },
        Raw::Content(content) => Variant::Content(match content {
            Value::String(none) if none == "None" => Content::None,
            Value::Object(map) => match map.get("Uri") {
                Some(Value::String(uri)) => Content::Uri(uri.clone()),
                _ => return None,
            },
            _ => return None,
        }),
    })
}

fn color3([r, g, b]: [f32; 3]) -> Color3Data {
    Color3Data { r, g, b }
}

fn vector2([x, y]: [f32; 2]) -> Vector2Data {
    Vector2Data { x, y }
}

fn vector3([x, y, z]: [f32; 3]) -> Vector3Data {
    Vector3Data { x, y, z }
}

fn udim((scale, offset): (f32, i32)) -> UDim {
    UDim { scale, offset }
}

fn cframe(frame: RawCFrame) -> CFrameData {
    let [a, b, c] = frame.orientation;
    CFrameData {
        position: vector3(frame.position),
        rotation: [a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]],
    }
}

#[cfg(test)]
mod tests;
