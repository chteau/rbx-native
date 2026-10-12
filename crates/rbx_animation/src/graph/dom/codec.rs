//! Values to and from attributes, and the small instances every node,
//! state and parameter pill carries.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use rbx_dom::{attributes, Ref, Variant, Vector2Data, WeakDom};

use super::{children, set, ATTRIBUTES, CONFIGURATION, PARAM_PREFIX};
use crate::graph::model::enums::{self, EnumType};
use crate::graph::model::{Prop, Value};

/// An attribute map being built, with the enum each `EnumItem` belongs to,
/// which the blob writes beside the value.
#[derive(Default)]
pub(super) struct Attrs {
    map: BTreeMap<String, Variant>,
    enums: BTreeMap<String, &'static str>,
}

impl Attrs {
    pub fn insert(&mut self, name: &str, value: Variant) {
        self.map.insert(name.to_owned(), value);
    }

    /// `value` as `enum_type`'s `EnumItem` when it names (or numbers) one of
    /// its items, else as the plain value it is.
    pub fn value(&mut self, name: &str, value: &Value, enum_type: Option<&'static EnumType>) {
        let item = enum_type.and_then(|e| {
            let ordinal = match value {
                Value::Text(t) => e.value(t),
                Value::Number(n) => e.item(*n as u32).map(|_| *n as u32),
                Value::Bool(_) => None,
            }?;
            Some((e.name, ordinal))
        });
        match item {
            Some((enum_name, ordinal)) => {
                self.enums.insert(name.to_owned(), enum_name);
                self.insert(name, Variant::Enum(ordinal));
            }
            None => self.insert(name, variant_of(value)),
        }
    }

    pub fn enum_item(&mut self, name: &str, enum_type: &'static EnumType, item: &str) {
        self.value(name, &Value::Text(item.to_owned()), Some(enum_type));
    }

    pub fn write(self, dom: &mut WeakDom, of: Ref) {
        if self.map.is_empty() {
            return;
        }
        // Every enum was named as it went in, so encoding cannot be refused.
        if let Some(raw) =
            attributes::encode(&self.map, |n| self.enums.get(n).map(|e| (*e).to_owned()))
        {
            set(dom, of, ATTRIBUTES, Variant::Unknown { type_id: 0x01, raw });
        }
    }
}

pub(super) fn variant_of(v: &Value) -> Variant {
    match v {
        Value::Number(n) => Variant::Float64(*n),
        Value::Bool(b) => Variant::Bool(*b),
        Value::Text(t) => Variant::String(t.clone()),
    }
}

pub(super) fn value_of(v: &Variant) -> Option<Value> {
    Some(match v {
        Variant::Float64(n) => Value::Number(*n),
        Variant::Float32(n) => Value::Number(f64::from(*n)),
        Variant::Int32(n) => Value::Number(f64::from(*n)),
        Variant::Int64(n) => Value::Number(*n as f64),
        Variant::Enum(n) => Value::Number(f64::from(*n)),
        Variant::Bool(b) => Value::Bool(*b),
        Variant::String(s) => Value::Text(s.clone()),
        _ => return None,
    })
}

/// An attribute's value with an `EnumItem` turned back into its item name,
/// which only the attribute's name can say, since the blob's reader keeps
/// just the ordinal.
pub(super) fn named_value(name: &str, v: &Variant) -> Option<Value> {
    match (v, enums::of_prop(name)) {
        (Variant::Enum(n), Some(e)) => e.item(*n).map(|i| Value::Text(i.to_owned())),
        _ => value_of(v),
    }
}

/// A property attribute as the model holds it. `legacy` also reads the
/// `$name` / `=source` strings this crate's earlier layout wrote.
pub(super) fn prop_of(name: &str, v: &Variant, legacy: bool) -> Option<Prop> {
    let value = named_value(name, v)?;
    Some(match value {
        Value::Text(t) if t.starts_with(PARAM_PREFIX) => {
            Prop::Param(t[PARAM_PREFIX.len()..].to_owned())
        }
        Value::Text(t) if legacy && t.len() > 1 && t.starts_with('$') => {
            Prop::Param(t[1..].to_owned())
        }
        Value::Text(t) if legacy && t.starts_with('=') => Prop::Expr(t[1..].to_owned()),
        other => Prop::Value(other),
    })
}

pub(super) fn vector2(p: (f32, f32)) -> Variant {
    Variant::Vector2(Vector2Data { x: p.0, y: p.1 })
}

pub(super) fn point(v: Option<&Variant>) -> Option<(f32, f32)> {
    match v? {
        Variant::Vector2(p) => Some((p.x, p.y)),
        _ => None,
    }
}

/// The editor-layout attributes a node, state or pill keeps.
#[derive(Default)]
pub(super) struct Layout {
    pub position: (f32, f32),
    pub size: Option<(f32, f32)>,
    pub collapsed: Option<bool>,
    pub display_name: Option<String>,
    pub z_index: Option<f64>,
}

pub(super) fn write_layout(dom: &mut WeakDom, of: Ref, layout: Layout) {
    let config = dom.new_instance(CONFIGURATION, CONFIGURATION, Some(of));
    let mut a = Attrs::default();
    a.insert("AnimGraph_Position", vector2(layout.position));
    if let Some(size) = layout.size {
        a.insert("AnimGraph_Size", vector2(size));
    }
    if let Some(collapsed) = layout.collapsed {
        a.insert("AnimGraph_Collapsed", Variant::Bool(collapsed));
    }
    if let Some(name) = layout.display_name {
        a.insert("AnimGraph_DisplayName", Variant::String(name));
    }
    if let Some(z) = layout.z_index {
        a.insert("AnimGraph_ZIndex", Variant::Float64(z));
    }
    a.write(dom, config);
}

/// The `Configuration` child's layout, if `of` has one.
pub(super) fn read_layout(dom: &WeakDom, of: Ref) -> Option<Layout> {
    let config = children(dom, of, CONFIGURATION)
        .into_iter()
        .find(|c| super::name(dom, *c) == CONFIGURATION)?;
    let a = super::attrs(dom, config);
    Some(Layout {
        position: point(a.get("AnimGraph_Position")).unwrap_or_default(),
        size: point(a.get("AnimGraph_Size")),
        collapsed: match a.get("AnimGraph_Collapsed") {
            Some(Variant::Bool(b)) => Some(*b),
            _ => None,
        },
        display_name: match a.get("AnimGraph_DisplayName") {
            Some(Variant::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        },
        z_index: a
            .get("AnimGraph_ZIndex")
            .and_then(value_of)
            .map(|v| v.number()),
    })
}

/// A GUID in `HttpService:GenerateGUID(false)`'s shape (uppercase, no
/// braces, version 4), derived from `seed` so a graph saves the same way
/// twice. Roblox only needs it to be unique within the state machine.
pub(super) fn guid(seed: impl Hash) -> String {
    let half = |salt: u8| {
        let mut h = DefaultHasher::new();
        salt.hash(&mut h);
        seed.hash(&mut h);
        h.finish()
    };
    let (a, b) = (half(0), half(1));
    let version = (a & 0xFFFF_FFFF_FFFF_0FFF) | 0x4000;
    let variant = (b & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    format!(
        "{:08X}-{:04X}-{:04X}-{:04X}-{:012X}",
        version >> 32,
        (version >> 16) & 0xFFFF,
        version & 0xFFFF,
        variant >> 48,
        variant & 0xFFFF_FFFF_FFFF
    )
}
