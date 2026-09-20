//! Read-only view of one instance's properties: `Name = value` rows spelled
//! the way `rbxdump` prints them, so the panel and the text dump agree.

use std::collections::BTreeMap;

use rbx_dom::{
    Axes, CFrameData, Color3Data, Content, Faces, Font, PhysicalProperties, Ref, Variant, WeakDom,
};
use rbx_reflection::ReflectionDatabase;

use crate::script_editor::source;

pub(crate) mod attributes;
pub(crate) mod edit;
mod folder_row;

/// Above this, a string no longer reads on a one-line row and only its size is
/// worth showing.
const MAX_STRING_LEN: usize = 64;

/// Category for a property the reflection dump has never heard of (e.g. an
/// unreflected `Tags`, or a class the dump does not know). Not a dump
/// category itself, so it can never collide with a real one.
const UNCATEGORIZED: &str = "Other";

/// What one field of a composite value holds.
///
/// Not decoration: it decides whether a field can be dragged and how far a
/// drag moves it, and it is why `Vector3int16` can no longer be handed
/// `3.7`. The DOM's own types are mixed even inside a single row — a `UDim`
/// is an `f32` scale beside an `i32` offset — so this is per *field*, never
/// per property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldKind {
    Decimal,
    Integer,
    /// Not a number at all — a `Font`'s family name. Typed, never dragged.
    Text,
}

impl FieldKind {
    /// How much one pixel of horizontal drag is worth.
    ///
    /// An integer moves a whole unit every few pixels rather than a
    /// fraction every pixel: dragging a `Vector3int16` should step through
    /// cells, not crawl. `None` is a field a drag must not touch.
    pub(crate) fn step_per_pixel(self) -> Option<f32> {
        match self {
            FieldKind::Decimal => Some(0.05),
            FieldKind::Integer => Some(1. / 6.),
            FieldKind::Text => None,
        }
    }
}

/// One field of a composite value: what it is called and what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Field {
    pub(crate) label: &'static str,
    pub(crate) kind: FieldKind,
}

/// A decimal field, which is most of them.
const fn decimal(label: &'static str) -> Field {
    Field {
        label,
        kind: FieldKind::Decimal,
    }
}

const fn integer(label: &'static str) -> Field {
    Field {
        label,
        kind: FieldKind::Integer,
    }
}

const fn text(label: &'static str) -> Field {
    Field {
        label,
        kind: FieldKind::Text,
    }
}

/// One captioned run of fields inside an [`EditKind::Groups`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FieldGroup {
    pub(crate) caption: &'static str,
    pub(crate) fields: &'static [Field],
}

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
        fields: &'static [Field],
        values: Vec<String>,
    },
    /// Several captioned groups of numeric fields, each on its own line —
    /// a `CFrame`'s Position and Orientation, a `Ray`'s Origin and
    /// Direction.
    ///
    /// The values are one flat list in group order, because the commit path
    /// is a single comma-joined string either way; the groups only decide
    /// how the row is *drawn*.
    Groups {
        groups: &'static [FieldGroup],
        values: Vec<String>,
    },
    /// Independent named flags — a `Faces`' six sides, an `Axes`' three.
    ///
    /// A checkbox each, because that is what a bit set is: six yes/no
    /// answers, not one value with sixty-four spellings. They used to
    /// render as read-only text like `Faces[Top, Front]`.
    Flags {
        labels: &'static [&'static str],
        values: Vec<bool>,
    },
    /// A value that is only half there: a checkbox, and the editor for the
    /// rest of it below once the box is ticked. Two `Variant`s are shaped
    /// this way, for different reasons — an `OptionalCFrame` may genuinely
    /// be absent, while a `PhysicalProperties` is always *something* but
    /// only carries five numbers in its `Custom` form — which is why the
    /// checkbox carries its own `label` rather than one fixed wording.
    ///
    /// `inner` is seeded even while `present` is false — the row does not
    /// draw it then, but that is what the value becomes the moment the
    /// checkbox turns it on (see `edit::IDENTITY_CFRAME` and
    /// `edit::DEFAULT_PHYSICAL`).
    Optional {
        present: bool,
        label: &'static str,
        inner: Box<EditKind>,
    },
    /// A `NumberSequence`'s curve or a `ColorSequence`'s ramp: too many
    /// numbers for a row, and the wrong numbers to type. The row draws the
    /// sequence itself and opens `crate::sequence_window` — the graph where
    /// keypoints are dragged — which commits through this same `text`.
    Sequence {
        /// Which of the two, since the row draws a ramp for one and a curve
        /// for the other and the text alone cannot say.
        color: bool,
        text: String,
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
    /// the DOM no longer holds. `folder_color` is this instance's tag from
    /// `crate::folder_colors::FolderColors`, if any — a filesystem-backed
    /// store this panel never reaches into itself (see `shell::folder_color`)
    /// — used only to seed the synthetic `Folder` colour row below.
    pub(crate) fn rows(
        &self,
        dom: &WeakDom,
        reference: Ref,
        folder_color: Option<(u8, u8, u8)>,
    ) -> Vec<PropertyRow> {
        let Some(instance) = dom.get(reference) else {
            return Vec::new();
        };
        let class = instance.class();

        let mut rows: Vec<PropertyRow> = instance
            .properties()
            .iter()
            // Real Studio never lists a `Hidden`-tagged property at all —
            // e.g. `BasePart.Position`/`Orientation`, exposed only through
            // the dedicated Position/Orientation UI — not even read-only.
            .filter(|(name, _)| !self.is_hidden(class, name))
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

        if let Some(row) = folder_row::row(
            class,
            self.category(class, edit::FOLDER_COLOR_PROPERTY),
            folder_color,
        ) {
            rows.push(row);
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
        folder_color: Option<(u8, u8, u8)>,
    ) -> Vec<PropertyRow> {
        let mut rows = self.rows(dom, reference, folder_color);
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

    /// Whether the reflection dump tags `class.name` `Hidden` — an unreflected
    /// property (the dump has never heard of it) defaults to shown, same as
    /// every other tag-driven default in this codebase.
    fn is_hidden(&self, class: &str, name: &str) -> bool {
        self.db
            .resolve_property(class, name)
            .is_some_and(|property| property.is_hidden())
    }

    /// Whether `class.name` should render with no edit affordance — see
    /// [`rbx_reflection::PropertyDescriptor::is_read_only`]. An unreflected
    /// property defaults to editable, same as [`Self::is_hidden`].
    fn is_read_only(&self, class: &str, name: &str) -> bool {
        self.db
            .resolve_property(class, name)
            .is_some_and(|property| property.is_read_only())
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

        // A property the dump says Studio can't save back (or explicitly
        // marks ReadOnly) gets the same no-edit-affordance treatment as any
        // other type this panel doesn't understand — e.g. `BasePart.Size`,
        // which stays visible but read-only (Studio derives it from the
        // mesh/CFrame rather than storing it directly).
        if self.is_read_only(class, name) {
            return None;
        }

        let text = edit::edit_text(value)?;

        Some(match value {
            // The only arm that needs `self`: resolving an enum's member
            // names is a reflection lookup, which `value_edit_kind` (shared
            // with `attributes::edit_kind_for`, an attribute never being an
            // `Enum` — see that module) has no database to make.
            Variant::Enum(raw) => self.enum_kind(class, name, *raw, text),
            other => value_edit_kind(other, text),
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
            // Spelled the way the editor's own fields read it back, not as
            // Rust's derived `Debug`: `Custom { density: 0.7, .. }` is a
            // struct dump, and this column is meant to agree with `rbxdump`.
            Variant::PhysicalProperties(PhysicalProperties::Default) => "Default".to_owned(),
            Variant::PhysicalProperties(PhysicalProperties::Custom {
                density,
                friction,
                elasticity,
                friction_weight,
                elasticity_weight,
            }) => format!(
                "Custom({density}, {friction}, {elasticity}, {friction_weight}, {elasticity_weight})"
            ),
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

/// Which widget `value` (already turned into `text` by `edit::edit_text`)
/// should edit through, for every type that needs no reflection lookup to
/// decide — i.e. every type but `Enum`, whose member names live in the
/// `ReflectionDatabase` `Properties::edit_kind` alone holds. Pulled out as a
/// free function so `properties::attributes` can build the exact same
/// `EditKind` for an attribute's value, which is never an `Enum` (not one of
/// the types `Instance:SetAttribute` accepts — see that module), without a
/// second copy of this match.
pub(crate) fn value_edit_kind(value: &Variant, text: String) -> EditKind {
    match value {
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
        Variant::Vector2(_) => fields(VECTOR2, &text),
        Variant::Vector3(_) => fields(VECTOR3, &text),
        // Integer components, so a drag steps by whole cells and a
        // typed `3.7` rounds rather than truncating toward zero.
        Variant::Vector3int16 { .. } => fields(VECTOR3_INT, &text),
        // A `UDim` is a float scale beside an *integer* offset — the
        // clearest case for why a field's kind is not a property's.
        Variant::UDim(_) => fields(UDIM, &text),
        Variant::UDim2(_) => fields(UDIM2, &text),
        Variant::Rect(_) => fields(RECT, &text),
        Variant::NumberRange(_) => fields(NUMBER_RANGE, &text),
        // Position and orientation, the way Roblox's own panel splits
        // them — a rotation matrix is not something anyone types. See
        // `edit::orientation` for the conversion and what it costs.
        Variant::CFrame(_) => groups(CFRAME, &text),
        // The one type the DOM can hold that may be absent rather than
        // wrong: a checkbox says whether there is a value, and the same
        // `CFrame` editor edits it once there is. Without the checkbox an
        // absent one was read-only, with nothing anywhere to say "give this
        // one a value".
        Variant::OptionalCFrame(frame) => EditKind::Optional {
            present: frame.is_some(),
            label: "Has value",
            inner: Box::new(groups(CFRAME, &text)),
        },
        // The same shape for a different reason. `PhysicalProperties` is
        // never absent — a part always has physics — but its `Default` form
        // carries no numbers at all: the engine derives them from the
        // material. So the box does not say "has value", it says which of
        // the enum's two forms this is, and the five fields appear only for
        // the one that actually has five fields. That is also how Studio's
        // own panel presents it, as a `CustomPhysicalProperties` boolean
        // with the numbers underneath.
        Variant::PhysicalProperties(physical) => EditKind::Optional {
            present: matches!(physical, PhysicalProperties::Custom { .. }),
            label: "Custom",
            inner: Box::new(fields(PHYSICAL_PROPERTIES, &text)),
        },
        Variant::Ray { .. } => groups(RAY, &text),
        Variant::Faces(faces) => EditKind::Flags {
            labels: FACES,
            values: vec![
                faces.right,
                faces.top,
                faces.back,
                faces.left,
                faces.bottom,
                faces.front,
            ],
        },
        Variant::Axes(axes) => EditKind::Flags {
            labels: AXES,
            values: vec![axes.x, axes.y, axes.z],
        },
        // A family name, a `FontWeight` name and `Normal`/`Italic`, typed:
        // the fonts package that could list the families lives in the
        // viewer, and a weight's nine names are quicker typed than picked.
        Variant::Font(_) => fields(FONT, &text),
        Variant::NumberSequence(_) => EditKind::Sequence { color: false, text },
        Variant::ColorSequence(_) => EditKind::Sequence { color: true, text },
        _ => EditKind::Text(text),
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
const VECTOR2: &[Field] = &[decimal("X"), decimal("Y")];
const VECTOR3: &[Field] = &[decimal("X"), decimal("Y"), decimal("Z")];
const VECTOR3_INT: &[Field] = &[integer("X"), integer("Y"), integer("Z")];
const UDIM: &[Field] = &[decimal("Scale"), integer("Offset")];
const UDIM2: &[Field] = &[
    decimal("X Scale"),
    integer("X Offset"),
    decimal("Y Scale"),
    integer("Y Offset"),
];
const RECT: &[Field] = &[
    decimal("Min X"),
    decimal("Min Y"),
    decimal("Max X"),
    decimal("Max Y"),
];
const NUMBER_RANGE: &[Field] = &[decimal("Min"), decimal("Max")];
/// The five numbers a `PhysicalProperties::Custom` carries, in the order
/// Roblox's own `PhysicalProperties.new` takes them — so a reader comparing
/// against the API, or against Studio's panel, finds them where they expect.
const PHYSICAL_PROPERTIES: &[Field] = &[
    decimal("Density"),
    decimal("Friction"),
    decimal("Elasticity"),
    decimal("Friction Weight"),
    decimal("Elasticity Weight"),
];
const FONT: &[Field] = &[text("Family"), text("Weight"), text("Style")];

/// A `Faces`' six sides, in the order Roblox's own `Enum.NormalId` lists
/// them — so a reader comparing against Studio finds them where they expect.
const FACES: &[&str] = &["Right", "Top", "Back", "Left", "Bottom", "Front"];
const AXES: &[&str] = &["X", "Y", "Z"];

/// A `CFrame`, as two captioned lines.
const CFRAME: &[FieldGroup] = &[
    FieldGroup {
        caption: "Position",
        fields: VECTOR3,
    },
    FieldGroup {
        caption: "Orientation",
        fields: VECTOR3,
    },
];

const RAY: &[FieldGroup] = &[
    FieldGroup {
        caption: "Origin",
        fields: VECTOR3,
    },
    FieldGroup {
        caption: "Direction",
        fields: VECTOR3,
    },
];

/// [`fields`]'s captioned cousin: the same comma-joined text, split across
/// however many fields the groups name between them.
fn groups(groups: &'static [FieldGroup], text: &str) -> EditKind {
    let values: Vec<String> = text.split(',').map(|part| part.trim().to_owned()).collect();
    debug_assert_eq!(
        values.len(),
        groups.iter().map(|group| group.fields.len()).sum::<usize>(),
        "{groups:?} vs {text:?}"
    );
    EditKind::Groups { groups, values }
}

fn fields(fields: &'static [Field], text: &str) -> EditKind {
    let values: Vec<String> = text.split(", ").map(str::to_owned).collect();
    debug_assert_eq!(values.len(), fields.len(), "{fields:?} vs {text:?}");
    EditKind::Fields { fields, values }
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
