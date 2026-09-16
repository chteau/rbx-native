//! Read-only view of one instance's properties: `Name = value` rows spelled
//! the way `rbxdump` prints them, so the panel and the text dump agree.

use std::collections::BTreeMap;

use rbx_dom::{Axes, CFrameData, Color3Data, Content, Faces, Font, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::script_editor::source;

pub(crate) mod edit;

/// Above this, a string no longer reads on a one-line row and only its size is
/// worth showing.
const MAX_STRING_LEN: usize = 64;

/// Category for a property the reflection dump has never heard of (e.g. an
/// unreflected `Tags`, or a class the dump does not know). Not a dump
/// category itself, so it can never collide with a real one.
const UNCATEGORIZED: &str = "Other";

/// Which widget a row's value should edit through, and the seed data that
/// widget starts from. `None` on [`PropertyRow::edit`] keeps the row
/// read-only, same as a `None` from `edit::edit_text` always has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditKind {
    /// A single free-text `Input`: scalars, strings, and any compound type
    /// not broken out into its own [`Self::Fields`] row (e.g. `NumberRange`,
    /// `UDim`, and — since no palette table is bundled here (see
    /// `edit::edit_text`'s `BrickColor` arm) — `BrickColor`'s raw index).
    Text(String),
    Bool(bool),
    /// 0-255 sRGB channels, matching how [`color3`] already displays a
    /// `Color3`/`Color3uint8` — no linear-light conversion, same space the
    /// DOM stores these in.
    Color {
        r: u8,
        g: u8,
        b: u8,
    },
    /// The current member's name (its raw ordinal, spelled as text, if the
    /// dump has none) plus every member name the dropdown should list.
    Enum {
        current: String,
        items: Vec<String>,
    },
    /// A short row of numeric fields, one label per component: `edit_text`'s
    /// comma-joined text split back into `labels.len()` seed values.
    Fields {
        labels: &'static [&'static str],
        values: Vec<String>,
    },
}

/// One line of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PropertyRow {
    pub(crate) name: String,
    pub(crate) value: String,
    /// The dump's `Category` for this property (e.g. `"Part"`,
    /// `"Appearance"`), or [`UNCATEGORIZED`] when the dump has no reflection
    /// data for it. Drives the panel's collapsible sections.
    pub(crate) category: String,
    /// `None` for a type [`Properties::edit_kind`] does not understand,
    /// which keeps the row read-only.
    pub(crate) edit: Option<EditKind>,
}

/// Groups rows by [`PropertyRow::category`], sorted alphabetically by
/// category name (there is no canonical order to reproduce, unlike Studio's
/// own panel — see `AGENTS.md`'s note on this); a row's order within its
/// group is unchanged. Every group is non-empty by construction.
pub(crate) fn group_by_category(rows: Vec<PropertyRow>) -> Vec<(String, Vec<PropertyRow>)> {
    let mut grouped: BTreeMap<String, Vec<PropertyRow>> = BTreeMap::new();
    for row in rows {
        grouped.entry(row.category.clone()).or_default().push(row);
    }
    grouped.into_iter().collect()
}

/// The reflection data that turns enum ordinals into names. The DOM is borrowed
/// per call rather than owned: the panel reads the one tree `Shell` mutates, so
/// nothing has to be copied to keep a row current — a copy of a place with tens
/// of thousands of instances costs tens of milliseconds, far too much for the
/// UI thread to spend several times a second on a moving camera.
pub(crate) struct Properties {
    db: ReflectionDatabase,
}

impl Properties {
    pub(crate) fn new(db: ReflectionDatabase) -> Self {
        Properties { db }
    }

    /// The panel's header: `Part "Baseplate"`.
    pub(crate) fn title(&self, dom: &WeakDom, reference: Ref) -> Option<String> {
        let instance = dom.get(reference)?;
        Some(format!("{} {:?}", instance.class(), instance.name()))
    }

    /// Every property of the instance, sorted by name; nothing for a reference
    /// the DOM no longer holds.
    pub(crate) fn rows(&self, dom: &WeakDom, reference: Ref) -> Vec<PropertyRow> {
        let Some(instance) = dom.get(reference) else {
            return Vec::new();
        };
        let class = instance.class();

        let mut rows: Vec<PropertyRow> = instance
            .properties()
            .iter()
            .map(|(name, value)| PropertyRow {
                name: name.clone(),
                value: self.format(dom, class, name, value),
                category: self.category(class, name),
                edit: self.edit_kind(class, name, value),
            })
            .collect();

        // A real file never stores `Name` as a property (see
        // `rbx_binary::deserializer::NAME_PROPERTY`); the row is synthesized
        // here so it can still be edited through `edit::commit`'s `set_name`
        // path. The guard only matters for tests that build an `Instance`
        // with a `Name` key of their own.
        if !instance.properties().contains_key(edit::NAME_PROPERTY) {
            let name = instance.name().to_owned();
            rows.push(PropertyRow {
                name: edit::NAME_PROPERTY.to_owned(),
                value: self.format(
                    dom,
                    class,
                    edit::NAME_PROPERTY,
                    &Variant::String(name.clone()),
                ),
                category: self.category(class, edit::NAME_PROPERTY),
                edit: Some(EditKind::Text(name)),
            });
        }

        rows.sort_by(|left, right| left.name.cmp(&right.name));
        rows
    }

    /// [`Self::rows`] narrowed to the names the filter box matches.
    pub(crate) fn rows_matching(
        &self,
        dom: &WeakDom,
        reference: Ref,
        filter: &str,
    ) -> Vec<PropertyRow> {
        let mut rows = self.rows(dom, reference);
        rows.retain(|row| matches(&row.name, filter));
        rows
    }

    /// The dump's `Category` for `class.name`, or [`UNCATEGORIZED`] when the
    /// dump has no reflection data for it (an unreflected property, or one
    /// synthesized for a class the dump does not know).
    fn category(&self, class: &str, name: &str) -> String {
        self.db
            .resolve_property(class, name)
            .map(|property| property.category.clone())
            .unwrap_or_else(|| UNCATEGORIZED.to_owned())
    }

    /// Which widget `value` should edit through; `None` keeps the row
    /// read-only, same as when `edit::edit_text` itself returns `None`.
    fn edit_kind(&self, class: &str, name: &str, value: &Variant) -> Option<EditKind> {
        // A script's code is edited in the Script Editor panel, not here. A
        // one-line field is the wrong shape for it, and this panel's commit
        // path trims what it writes (see `edit::parse`'s `String` arm), which
        // would silently eat a script's trailing newline. The row stays,
        // read-only, showing the source's size like any other long string.
        if name == source::SOURCE_PROPERTY && source::is_script_class(&self.db, class) {
            return None;
        }

        let text = edit::edit_text(value)?;

        Some(match value {
            Variant::Bool(flag) => EditKind::Bool(*flag),
            Variant::Color3(color) => EditKind::Color {
                r: channel(color.r),
                g: channel(color.g),
                b: channel(color.b),
            },
            Variant::Color3uint8 { r, g, b } => EditKind::Color {
                r: *r,
                g: *g,
                b: *b,
            },
            Variant::Enum(raw) => self.enum_kind(class, name, *raw, text),
            Variant::Vector2(_) => fields(&["X", "Y"], &text),
            // `CFrame`'s edit text is position-only (see `edit::edit_text`),
            // so it shares Vector3's 3-field shape.
            Variant::Vector3(_) | Variant::CFrame(_) => fields(&["X", "Y", "Z"], &text),
            Variant::UDim2(_) => fields(&["X Scale", "X Offset", "Y Scale", "Y Offset"], &text),
            _ => EditKind::Text(text),
        })
    }

    /// A dropdown when the dump names `raw`'s enum's members, otherwise the
    /// plain-text ordinal — the same fallback [`Self::enumeration`] uses for
    /// display.
    fn enum_kind(&self, class: &str, name: &str, raw: u32, text: String) -> EditKind {
        let Some(property) = self.db.resolve_property(class, name) else {
            return EditKind::Text(text);
        };
        match self.db.enum_items(&property.value_type) {
            Some(items) if !items.is_empty() => EditKind::Enum {
                current: self
                    .db
                    .enum_name(&property.value_type, raw)
                    .map(str::to_owned)
                    .unwrap_or(text),
                items: items.iter().map(|(name, _)| name.clone()).collect(),
            },
            _ => EditKind::Text(text),
        }
    }

    fn format(&self, dom: &WeakDom, class: &str, name: &str, value: &Variant) -> String {
        match value {
            Variant::String(text) if text.len() > MAX_STRING_LEN => bytes(text.len()),
            Variant::String(text) => format!("{text:?}"),
            Variant::Bool(flag) => flag.to_string(),
            Variant::Int32(number) => number.to_string(),
            Variant::Int64(number) => number.to_string(),
            Variant::Float32(number) => number.to_string(),
            Variant::Float64(number) => number.to_string(),
            Variant::BrickColor(number) => format!("BrickColor({number})"),
            Variant::Color3(color) => color3(color),
            Variant::Color3uint8 { r, g, b } => format!("({r}, {g}, {b})"),
            Variant::Vector2(v) => format!("({}, {})", v.x, v.y),
            Variant::Vector3(v) => format!("({}, {}, {})", v.x, v.y, v.z),
            Variant::Vector3int16 { x, y, z } => format!("({x}, {y}, {z})"),
            Variant::Ray { origin, direction } => format!(
                "Ray {{ origin: ({}, {}, {}), direction: ({}, {}, {}) }}",
                origin.x, origin.y, origin.z, direction.x, direction.y, direction.z
            ),
            Variant::Faces(faces) => self::faces(faces),
            Variant::Axes(axes) => self::axes(axes),
            Variant::CFrame(frame) => cframe(frame),
            Variant::OptionalCFrame(None) => "none".to_owned(),
            Variant::OptionalCFrame(Some(frame)) => cframe(frame),
            Variant::Enum(raw) => self.enumeration(class, name, *raw),
            Variant::Ref(target) => target_name(dom, *target),
            Variant::NumberSequence(sequence) => format!(
                "NumberSequence[{}]",
                join(&sequence.keypoints, |k| format!(
                    "{}: {} ±{}",
                    k.time, k.value, k.envelope
                ))
            ),
            Variant::ColorSequence(sequence) => format!(
                "ColorSequence[{}]",
                join(&sequence.keypoints, |k| format!(
                    "{}: {}",
                    k.time,
                    color3(&k.color)
                ))
            ),
            Variant::NumberRange(range) => format!("[{}, {}]", range.min, range.max),
            Variant::Rect(rect) => format!(
                "{{({}, {}), ({}, {})}}",
                rect.min.x, rect.min.y, rect.max.x, rect.max.y
            ),
            Variant::PhysicalProperties(physical) => format!("{physical:?}"),
            Variant::SharedString(id) => format!("SharedString({id})"),
            Variant::UDim(u) => format!("{{{}, {}}}", u.scale, u.offset),
            Variant::UDim2(u) => format!(
                "{{{{{}, {}}}, {{{}, {}}}}}",
                u.x.scale, u.x.offset, u.y.scale, u.y.offset
            ),
            // Wire order, so two ids from one save session line up visually on
            // their shared time and random halves.
            Variant::UniqueId(id) => {
                format!("{:08x}{:08x}{:016x}", id.index, id.time, id.random as u64)
            }
            Variant::Font(font) => self::font(font),
            // Hexadecimal because every bit is an independent capability flag.
            Variant::SecurityCapabilities(bits) => format!("SecurityCapabilities({bits:#x})"),
            Variant::Content(Content::None) => "Content(none)".to_owned(),
            Variant::Content(Content::Uri(uri)) => format!("Content({uri:?})"),
            Variant::Content(Content::Object(target)) => target_name(dom, *target),
            Variant::Unknown { raw, .. } => bytes(raw.len()),
        }
    }

    /// `256 (Plastic)`: the ordinal alone means nothing to a reader, but the
    /// name alone would hide a stale or custom value the dump does not know.
    fn enumeration(&self, class: &str, name: &str, raw: u32) -> String {
        let resolved = self
            .db
            .resolve_property(class, name)
            .and_then(|property| self.db.enum_name(&property.value_type, raw));

        match resolved {
            Some(label) => format!("{raw} ({label})"),
            None => raw.to_string(),
        }
    }
}

/// A reference reads as what it points at, the way Studio shows it; a
/// dangling one is `nil`, the way a script would see it.
fn target_name(dom: &WeakDom, target: Ref) -> String {
    dom.get(target)
        .map(|instance| instance.name().to_owned())
        .unwrap_or_else(|| "nil".to_owned())
}

/// Whether a row named `name` survives the filter box: a case-insensitive
/// substring match, with an empty box keeping everything.
pub(crate) fn matches(name: &str, filter: &str) -> bool {
    let filter = filter.trim();
    filter.is_empty() || name.to_lowercase().contains(&filter.to_lowercase())
}

fn bytes(count: usize) -> String {
    format!("<{count} bytes>")
}

/// Studio's own spelling, 0–255 per channel, rather than the stored 0–1 floats.
fn color3(color: &Color3Data) -> String {
    format!(
        "({}, {}, {})",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

/// A stored 0–1 float channel as the 0–255 byte Studio (and [`EditKind::Color`])
/// displays.
fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Splits `edit::edit_text`'s comma-joined text back into one seed value per
/// label — the two are built from the same match arms in
/// `Properties::edit_kind`/`edit::edit_text`, so the split always has
/// exactly `labels.len()` pieces.
fn fields(labels: &'static [&'static str], text: &str) -> EditKind {
    let values: Vec<String> = text.split(", ").map(str::to_owned).collect();
    debug_assert_eq!(values.len(), labels.len(), "{labels:?} vs {text:?}");
    EditKind::Fields { labels, values }
}

fn cframe(frame: &CFrameData) -> String {
    format!(
        "pos=({}, {}, {}) rot={:?}",
        frame.position.x, frame.position.y, frame.position.z, frame.rotation
    )
}

fn flags(prefix: &str, names: [&str; 6], set: [bool; 6]) -> String {
    let names: Vec<&str> = names
        .iter()
        .zip(set)
        .filter(|(_, set)| *set)
        .map(|(name, _)| *name)
        .collect();
    format!("{prefix}({})", names.join("|"))
}

// Wire bit order (Front, Bottom, Left, Back, Top, Right), not alphabetical, so
// the listing matches the binary spec.
fn faces(faces: &Faces) -> String {
    flags(
        "Faces",
        ["Front", "Bottom", "Left", "Back", "Top", "Right"],
        [
            faces.front,
            faces.bottom,
            faces.left,
            faces.back,
            faces.top,
            faces.right,
        ],
    )
}

fn axes(axes: &Axes) -> String {
    flags(
        "Axes",
        ["X", "Y", "Z", "", "", ""],
        [axes.x, axes.y, axes.z, false, false, false],
    )
}

fn font(font: &Font) -> String {
    let mut out = format!(
        "Font {{ family: {:?}, weight: {}, style: {:?}",
        font.family, font.weight, font.style
    );
    if let Some(cached) = &font.cached_face_id {
        out.push_str(&format!(", cached: {cached:?}"));
    }
    out.push_str(" }");
    out
}

fn join<T>(items: &[T], format_one: impl Fn(&T) -> String) -> String {
    items
        .iter()
        .map(format_one)
        .collect::<Vec<String>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
