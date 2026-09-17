//! The Style Editor panel's model: the `StyleSheet`/`StyleDerive`/`StyleRule`
//! tree a place holds, and the DOM writes the panel makes to it.
//!
//! Roblox authors these through a dedicated Style Editor rather than through
//! raw property editing (`studio/ui-overview.md`, `ui/styling/editor.md`):
//! sheets in a left column, and the selected sheet's rules with their
//! properties in a main panel. This is the same material at this editor's
//! fidelity — one list, a row per sheet, derive, rule and rule property.
//!
//! Nothing here renders; `shell::style_panel` does that, and this module is
//! pure DOM in and DOM out so it can be tested without a window.

use std::collections::BTreeMap;

use rbx_dom::{
    Color3Data, Instance, NumberRange, Ref, UDim, UDim2, Variant, Vector2Data, Vector3Data, WeakDom,
};
use rbx_reflection::ReflectionDatabase;

use crate::explorer;
use crate::properties::edit;

pub(crate) const SHEET_CLASS: &str = "StyleSheet";
pub(crate) const RULE_CLASS: &str = "StyleRule";
const DERIVE_CLASS: &str = "StyleDerive";
const LINK_CLASS: &str = "StyleLink";

/// A `StyleRule`'s own property overrides, serialized in the same blob format
/// as instance attributes (see `rbx_dom::attributes`).
const PROPERTIES_PROPERTY: &str = "PropertiesSerialize";
/// A `StyleSheet`'s tokens, which are ordinary instance attributes.
const ATTRIBUTES_PROPERTY: &str = "AttributesSerialize";
pub(crate) const SELECTOR_PROPERTY: &str = "Selector";
pub(crate) const PRIORITY_PROPERTY: &str = "Priority";
/// The `Ref` both `StyleLink` and `StyleDerive` name their sheet with.
const SHEET_PROPERTY: &str = "StyleSheet";

/// `Variant::Unknown`'s wire type id for a string-shaped blob — the one id
/// XML's `BinaryString` round-trips (see `rbx_xml::value::STRING_TYPE_ID`),
/// so a blob written back under it survives a save in either format.
const STRING_TYPE_ID: u8 = 0x01;

/// Where the Style Editor's own Create Design puts a generated sheet
/// (`ui/styling/editor.md`), and so where a sheet made here goes.
const SHEET_SERVICE: &str = "ReplicatedStorage";

/// Classes a rule property's type is looked up on when the rule's selector
/// names no class of its own (a tag or name rule). Leaves rather than base
/// classes, and `resolve_property` walks up from each, so between them they
/// cover every `GuiObject`, `LayerCollector` and `UIComponent` property
/// without listing the hierarchy out.
const FALLBACK_CLASSES: &[&str] = &[
    "TextBox",
    "ImageButton",
    "ScrollingFrame",
    "VideoFrame",
    "ScreenGui",
    "UIStroke",
    "UICorner",
    "UIGradient",
    "UIPadding",
    "UIListLayout",
];

/// One line of the panel, in the order [`rows`] lays them out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StyleRow {
    /// A `StyleSheet` and the ancestor path it lives under, which can be
    /// anywhere in the place — `ReplicatedStorage` by convention only.
    Sheet {
        referent: Ref,
        name: String,
        parent: String,
    },
    /// A `StyleDerive`: the sheet it pulls tokens and rules in from, and the
    /// priority that orders it against its siblings.
    Derive {
        referent: Ref,
        sheet: String,
        priority: i32,
    },
    /// A `StyleRule`. `depth` is its nesting inside the sheet — a rule may
    /// hold rules, whose selectors merge with its own.
    Rule {
        referent: Ref,
        depth: usize,
        selector: String,
        priority: i32,
    },
    /// One decoded entry of a rule's `PropertiesSerialize`. `value` is the
    /// text that edits it, or `None` for a type this editor cannot type back
    /// in (see `properties::edit::edit_text`).
    Property {
        rule: Ref,
        depth: usize,
        name: String,
        value: Option<String>,
    },
}

impl StyleRow {
    /// The instance selecting this row selects, so the Properties panel
    /// shows it. A property row has no instance of its own; it stands for
    /// the rule that holds it.
    pub(crate) fn referent(&self) -> Ref {
        match self {
            StyleRow::Sheet { referent, .. }
            | StyleRow::Derive { referent, .. }
            | StyleRow::Rule { referent, .. } => *referent,
            StyleRow::Property { rule, .. } => *rule,
        }
    }
}

/// Every `StyleSheet` in the place and what hangs off it, in tree order.
pub(crate) fn rows(dom: &WeakDom) -> Vec<StyleRow> {
    let mut rows = Vec::new();
    for &root in dom.root_refs() {
        gather(dom, root, &mut rows);
    }
    rows
}

/// Depth-first for `StyleSheet`s. A sheet is not searched for further sheets:
/// nothing in the styling family nests one inside another.
fn gather(dom: &WeakDom, referent: Ref, rows: &mut Vec<StyleRow>) {
    let Some(instance) = dom.get(referent) else {
        return;
    };
    if instance.class() != SHEET_CLASS {
        for &child in instance.children() {
            gather(dom, child, rows);
        }
        return;
    }

    rows.push(StyleRow::Sheet {
        referent,
        name: instance.name().to_owned(),
        parent: path_to(dom, referent),
    });
    for &child in instance.children() {
        let Some(child_instance) = dom.get(child) else {
            continue;
        };
        match child_instance.class() {
            DERIVE_CLASS => rows.push(StyleRow::Derive {
                referent: child,
                sheet: sheet_name(dom, child_instance),
                priority: integer(child_instance, PRIORITY_PROPERTY),
            }),
            RULE_CLASS => gather_rule(dom, child, 0, rows),
            _ => {}
        }
    }
}

fn gather_rule(dom: &WeakDom, referent: Ref, depth: usize, rows: &mut Vec<StyleRow>) {
    let Some(instance) = dom.get(referent) else {
        return;
    };
    rows.push(StyleRow::Rule {
        referent,
        depth,
        selector: text(instance, SELECTOR_PROPERTY).to_owned(),
        priority: integer(instance, PRIORITY_PROPERTY),
    });
    for (name, value) in decode(instance) {
        rows.push(StyleRow::Property {
            rule: referent,
            depth,
            name,
            value: edit::edit_text(&value),
        });
    }
    for &child in instance.children() {
        if dom.get(child).is_some_and(|c| c.class() == RULE_CLASS) {
            gather_rule(dom, child, depth + 1, rows);
        }
    }
}

/// `game.ReplicatedStorage.Design`-style path of `referent`'s ancestors, the
/// sheet itself left off — a sheet may sit anywhere, so where it sits is
/// worth showing beside its name.
fn path_to(dom: &WeakDom, referent: Ref) -> String {
    let mut names = Vec::new();
    let mut current = dom.parent(referent);
    while let Some(ancestor) = current {
        let Some(instance) = dom.get(ancestor) else {
            break;
        };
        names.push(instance.name().to_owned());
        current = dom.parent(ancestor);
    }
    names.reverse();
    names.join(".")
}

/// The name of the sheet a `StyleLink`/`StyleDerive` points at, or `nil` when
/// its reference no longer resolves — the same spelling the Properties panel
/// gives a dangling `Ref`.
fn sheet_name(dom: &WeakDom, instance: &Instance) -> String {
    reference(instance)
        .and_then(|sheet| dom.get(sheet))
        .map(|sheet| sheet.name().to_owned())
        .unwrap_or_else(|| "nil".to_owned())
}

fn reference(instance: &Instance) -> Option<Ref> {
    match instance.properties().get(SHEET_PROPERTY) {
        Some(&Variant::Ref(referent)) => Some(referent),
        _ => None,
    }
}

fn decode(instance: &Instance) -> BTreeMap<String, Variant> {
    rbx_dom::attributes::decode(instance.properties().get(PROPERTIES_PROPERTY))
}

fn integer(instance: &Instance, name: &str) -> i32 {
    match instance.properties().get(name) {
        Some(&Variant::Int32(value)) => value,
        _ => 0,
    }
}

fn text<'a>(instance: &'a Instance, name: &str) -> &'a str {
    match instance.properties().get(name) {
        Some(Variant::String(value)) => value,
        _ => "",
    }
}

/// Writes `text` into rule property `name`, adding it when the rule has no
/// such property yet.
///
/// A rule property carries no class to look its type up on, so the type comes
/// from the reflection dump the same way the Properties panel's own
/// `EditKind` does — resolved on the class the selector names, or on
/// [`FALLBACK_CLASSES`] when it names none — and the text is then parsed
/// against a zero value of that type by the very parser the Properties panel
/// uses. A property already in the rule is parsed against its current value
/// instead, so its type never changes under an edit.
///
/// Text opening with `$` is a token reference and is stored verbatim,
/// whatever the property's type — Studio's own "Link Token" workflow
/// (`ui/styling/editor.md`), typed rather than picked from a menu.
pub(crate) fn set_rule_property(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    rule: Ref,
    name: &str,
    text: &str,
) -> Result<(), String> {
    let instance = dom.get(rule).ok_or_else(gone)?;
    let selector = self::text(instance, SELECTOR_PROPERTY).to_owned();
    let mut properties = decode(instance);

    let class = class_of(database, &selector, name)
        .ok_or_else(|| format!("no GUI class has a {name} property"))?;

    let value = if is_token(text.trim()) {
        // A `$Token` reference stands in for a value of any type and is
        // stored as the string itself; the cascade resolves it against the
        // sheet's attributes when it applies the rule (see the viewer's
        // `scene::gui::style::cascade`). A literal string that happens to
        // open with `$` is indistinguishable from one, there as here.
        Variant::String(text.trim().to_owned())
    } else {
        let current = match properties.get(name) {
            // A token the user is typing a literal over says nothing about
            // the property's real type, so the dump answers that instead.
            Some(value) if !is_token_value(value) => value.clone(),
            _ => zero(database, class, name)
                .ok_or_else(|| format!("{name} has a type this editor cannot write"))?,
        };
        edit::parse(&current, database, class, name, text)?
    };
    properties.insert(name.to_owned(), value);
    write_properties(dom, database, rule, class, &properties)
}

/// Drops one property from a rule. Its type never comes up: the remaining
/// values are re-encoded exactly as they decoded.
pub(crate) fn remove_rule_property(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    rule: Ref,
    name: &str,
) -> Result<(), String> {
    let instance = dom.get(rule).ok_or_else(gone)?;
    let selector = text(instance, SELECTOR_PROPERTY).to_owned();
    let mut properties = decode(instance);
    if properties.remove(name).is_none() {
        return Err(format!("{name} is not one of this rule's properties"));
    }
    let class = class_of(database, &selector, name).unwrap_or(FALLBACK_CLASSES[0]);
    write_properties(dom, database, rule, class, &properties)
}

/// Re-encodes a rule's whole property bag and writes it back.
///
/// `class` only feeds the enum names `rbx_dom::attributes::encode` needs
/// beside an ordinal; the dump's `value_type` for an enum property *is* its
/// enum's name (the same field `Properties::enum_kind` looks members up by).
/// A bag holding a value with no type id leaves the DOM untouched rather
/// than writing a blob that would decode to nothing.
fn write_properties(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    rule: Ref,
    class: &str,
    properties: &BTreeMap<String, Variant>,
) -> Result<(), String> {
    let raw = rbx_dom::attributes::encode(properties, |name| {
        database
            .resolve_property(class, name)
            .map(|property| property.value_type.clone())
    })
    .ok_or_else(|| "this rule holds a property type that cannot be written back".to_owned())?;

    dom.set_property(
        rule,
        PROPERTIES_PROPERTY,
        Variant::Unknown {
            type_id: STRING_TYPE_ID,
            raw,
        },
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

/// The class a rule's property `name` is typed by: the one its selector opens
/// with, if that class has the property, else the first of
/// [`FALLBACK_CLASSES`] that does.
fn class_of<'a>(database: &ReflectionDatabase, selector: &'a str, name: &str) -> Option<&'a str> {
    selector_class(selector)
        .into_iter()
        .chain(FALLBACK_CLASSES.iter().copied())
        .find(|class| database.resolve_property(class, name).is_some())
}

/// A zero value of the type the dump gives `class.name`, for the text parser
/// to reshape — the dump carries no defaults, and every type
/// `properties::edit::parse` accepts is reachable from one of these.
fn zero(database: &ReflectionDatabase, class: &str, name: &str) -> Option<Variant> {
    let value_type = &database.resolve_property(class, name)?.value_type;
    if database.enum_items(value_type).is_some() {
        return Some(Variant::Enum(0));
    }
    let udim = UDim {
        scale: 0.0,
        offset: 0,
    };
    Some(match value_type.as_str() {
        "bool" => Variant::Bool(false),
        "int" => Variant::Int32(0),
        "int64" => Variant::Int64(0),
        "float" => Variant::Float32(0.0),
        "double" => Variant::Float64(0.0),
        "string" | "Content" | "ContentId" | "BinaryString" => Variant::String(String::new()),
        "BrickColor" => Variant::BrickColor(0),
        "Color3" => Variant::Color3(Color3Data {
            r: 0.0,
            g: 0.0,
            b: 0.0,
        }),
        "Vector2" => Variant::Vector2(Vector2Data { x: 0.0, y: 0.0 }),
        "Vector3" => Variant::Vector3(Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        "UDim" => Variant::UDim(udim),
        "UDim2" => Variant::UDim2(UDim2 { x: udim, y: udim }),
        "NumberRange" => Variant::NumberRange(NumberRange { min: 0.0, max: 0.0 }),
        _ => return None,
    })
}

/// The class name a selector opens with, if any: `Frame`, `TextButton:Hover`
/// and `Frame > TextLabel` all start with one, while `.Tag`, `#Name` and
/// `:Hover` do not (`ui/styling/editor.md` lists the five selector kinds).
fn selector_class(selector: &str) -> Option<&str> {
    let head: &str = selector
        .trim_start()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()?;
    (!head.is_empty() && head.starts_with(|c: char| c.is_ascii_uppercase())).then_some(head)
}

/// A new, empty `StyleRule` under `sheet`.
///
/// Every property the panel and the cascade read is written up front:
/// `properties::edit::commit` type-checks an edit against the value already
/// there, so a rule with no `Selector` key could never have one typed into
/// it — and a class whose instances disagree about which properties they
/// carry is what the binary serializer refuses to write.
pub(crate) fn add_rule(dom: &mut WeakDom, sheet: Ref) -> Ref {
    let rule = dom.new_instance(RULE_CLASS, RULE_CLASS, Some(sheet));
    let _ = dom.set_property(rule, SELECTOR_PROPERTY, Variant::String(String::new()));
    let _ = dom.set_property(rule, PRIORITY_PROPERTY, Variant::Int32(0));
    let _ = dom.set_property(
        rule,
        PROPERTIES_PROPERTY,
        Variant::Unknown {
            type_id: STRING_TYPE_ID,
            raw: Vec::new(),
        },
    );
    rule
}

/// A new `StyleSheet` under [`SHEET_SERVICE`], creating that service when the
/// place has none.
pub(crate) fn add_sheet(dom: &mut WeakDom) -> Ref {
    let parent = explorer::find_by_name(dom, SHEET_SERVICE)
        .unwrap_or_else(|| dom.new_instance(SHEET_SERVICE, SHEET_SERVICE, None));
    let sheet = dom.new_instance(SHEET_CLASS, SHEET_CLASS, Some(parent));
    // Tokens are this property's attributes; an empty blob is a sheet with
    // none, and keeps every sheet in the file carrying the same properties.
    let _ = dom.set_property(
        sheet,
        ATTRIBUTES_PROPERTY,
        Variant::Unknown {
            type_id: STRING_TYPE_ID,
            raw: Vec::new(),
        },
    );
    sheet
}

/// A `StyleLink` under `parent` naming `sheet`, which is what applies the
/// sheet to `parent`'s tree (`StyleLink`, creator-docs). Refused on a parent
/// that is not a `LayerCollector`: a link anywhere else styles nothing.
pub(crate) fn add_link(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    parent: Ref,
    sheet: Ref,
) -> Result<Ref, String> {
    let class = dom.get(parent).ok_or_else(gone)?.class().to_owned();
    if !database.is_subclass_of(&class, "LayerCollector") {
        return Err(format!(
            "a StyleLink styles the tree under a ScreenGui, not a {class}"
        ));
    }
    let link = dom.new_instance(LINK_CLASS, LINK_CLASS, Some(parent));
    let _ = dom.set_property(link, SHEET_PROPERTY, Variant::Ref(sheet));
    Ok(link)
}

fn is_token(text: &str) -> bool {
    text.len() > 1 && text.starts_with('$')
}

fn is_token_value(value: &Variant) -> bool {
    matches!(value, Variant::String(text) if is_token(text))
}

fn gone() -> String {
    "the instance no longer exists".to_owned()
}

#[cfg(test)]
mod tests;
