//! The window's edits, kept apart from what they edit until Save: the
//! cloud half against the settings Roblox last returned, the local half
//! against the place file's properties when the window read them. Both
//! forget an edit that puts a value back, so "dirty" means a real change.

use std::collections::BTreeMap;

use rbx_cloud::{ExperienceSettings, ExperienceUpdate, PlaceSettings, PlaceUpdate, Visibility};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::catalog::{CloudField, LocalField, LocalKind, LOCAL_FIELDS};

#[cfg(test)]
mod tests;

/// What Roblox returned for the linked experience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Remote {
    pub(super) experience: ExperienceSettings,
    /// The root place, whose name and description are the experience's.
    pub(super) root_place: PlaceSettings,
    /// The linked place; equal to `root_place` when it is the root.
    pub(super) place: PlaceSettings,
    pub(super) place_id: u64,
}

/// One cloud field's value, shaped for its control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CloudValue {
    Text(String),
    Bool(bool),
    Count(u32),
    /// Private servers: `None` is off, `Some` their price in Robux.
    Price(Option<u32>),
    /// A read-only field, as words ("Public", "13+").
    Fixed(String),
}

/// The updates a save sends; each `None`/empty one is skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Updates {
    pub(super) experience: ExperienceUpdate,
    /// For the root place (Basic Info's name and description).
    pub(super) root_place: PlaceUpdate,
    /// For the linked place when it is not the root (Places tab). When the
    /// linked place is the root, the Places tab's edits are merged into
    /// `root_place` instead, Basic Info's winning where both set a field.
    pub(super) place: PlaceUpdate,
}

#[derive(Debug, Clone, Default)]
pub(super) struct CloudDraft {
    pub(super) remote: Option<Remote>,
    edits: BTreeMap<CloudField, CloudValue>,
}

impl CloudDraft {
    /// Starts over from what Roblox now holds, dropping every edit.
    pub(super) fn reset(&mut self, remote: Remote) {
        self.remote = Some(remote);
        self.edits.clear();
    }

    /// Takes `remote` as the new baseline and keeps only the edits that
    /// still differ from it: what a save landed stops being pending, while
    /// edits made since the request went out survive.
    pub(super) fn rebase(&mut self, remote: Remote) {
        let edits = std::mem::take(&mut self.edits);
        self.remote = Some(remote);
        for (field, value) in edits {
            self.set(field, value);
        }
    }

    /// The remote value, as Roblox has it; `None` before the first load.
    /// Visibility reads "Public"/"Private" (or Roblox's own word), age
    /// rating "All ages"/"9+"/"13+"/"17+" or "Not rated".
    pub(super) fn remote_value(&self, field: CloudField) -> Option<CloudValue> {
        let remote = self.remote.as_ref()?;
        let exp = &remote.experience;
        Some(match field {
            CloudField::Name => CloudValue::Text(remote.root_place.display_name.clone()),
            CloudField::Description => CloudValue::Text(remote.root_place.description.clone()),
            CloudField::PlaceName => CloudValue::Text(remote.place.display_name.clone()),
            CloudField::PlaceDescription => CloudValue::Text(remote.place.description.clone()),
            CloudField::ServerSize => CloudValue::Count(remote.place.server_size),
            CloudField::Desktop => CloudValue::Bool(exp.desktop_enabled),
            CloudField::Mobile => CloudValue::Bool(exp.mobile_enabled),
            CloudField::Tablet => CloudValue::Bool(exp.tablet_enabled),
            CloudField::Console => CloudValue::Bool(exp.console_enabled),
            CloudField::Vr => CloudValue::Bool(exp.vr_enabled),
            CloudField::VoiceChat => CloudValue::Bool(exp.voice_chat_enabled),
            CloudField::PrivateServers => CloudValue::Price(exp.private_server_price_robux),
            CloudField::Visibility => CloudValue::Fixed(match &exp.visibility {
                Visibility::Public => "Public".to_string(),
                Visibility::Private => "Private".to_string(),
                Visibility::Other(word) => word.clone(),
            }),
            CloudField::AgeRating => CloudValue::Fixed(
                match exp.age_rating.as_deref() {
                    Some("ALL") => "All ages",
                    Some("9_PLUS") => "9+",
                    Some("13_PLUS") => "13+",
                    Some("17_PLUS") => "17+",
                    _ => "Not rated",
                }
                .to_string(),
            ),
        })
    }

    /// The edit if there is one, else the remote value.
    pub(super) fn value(&self, field: CloudField) -> Option<CloudValue> {
        self.edits
            .get(&field)
            .cloned()
            .or_else(|| self.remote_value(field))
    }

    /// Records an edit; one equal to the remote value is forgotten instead.
    /// Ignored for read-only fields and before the first load.
    pub(super) fn set(&mut self, field: CloudField, value: CloudValue) {
        if field.read_only() {
            return;
        }
        let Some(remote) = self.remote_value(field) else {
            return;
        };
        if remote == value {
            self.edits.remove(&field);
        } else {
            self.edits.insert(field, value);
        }
    }

    pub(super) fn is_edited(&self, field: CloudField) -> bool {
        self.edits.contains_key(&field)
    }

    pub(super) fn is_dirty(&self) -> bool {
        !self.edits.is_empty()
    }

    /// Every edit as the requests that carry it (see [`Updates`]).
    pub(super) fn updates(&self) -> Updates {
        let mut out = Updates::default();
        let merge = self
            .remote
            .as_ref()
            .is_some_and(|r| r.place_id == r.experience.root_place_id);
        // Basic Info's fields come first in `ALL`, so on a root place they
        // are written before the Places tab's and win.
        for (&field, value) in &self.edits {
            let exp = &mut out.experience;
            match (field, value) {
                (CloudField::Name, CloudValue::Text(t)) => {
                    out.root_place.display_name = Some(t.clone())
                }
                (CloudField::Description, CloudValue::Text(t)) => {
                    out.root_place.description = Some(t.clone())
                }
                (CloudField::Desktop, CloudValue::Bool(b)) => exp.desktop_enabled = Some(*b),
                (CloudField::Mobile, CloudValue::Bool(b)) => exp.mobile_enabled = Some(*b),
                (CloudField::Tablet, CloudValue::Bool(b)) => exp.tablet_enabled = Some(*b),
                (CloudField::Console, CloudValue::Bool(b)) => exp.console_enabled = Some(*b),
                (CloudField::Vr, CloudValue::Bool(b)) => exp.vr_enabled = Some(*b),
                (CloudField::VoiceChat, CloudValue::Bool(b)) => exp.voice_chat_enabled = Some(*b),
                (CloudField::PrivateServers, CloudValue::Price(p)) => {
                    exp.private_server_price_robux = Some(*p)
                }
                _ => {}
            }
        }
        for (&field, value) in &self.edits {
            let target = if merge {
                &mut out.root_place
            } else {
                &mut out.place
            };
            match (field, value) {
                (CloudField::PlaceName, CloudValue::Text(t)) => {
                    target.display_name.get_or_insert_with(|| t.clone());
                }
                (CloudField::PlaceDescription, CloudValue::Text(t)) => {
                    target.description.get_or_insert_with(|| t.clone());
                }
                (CloudField::ServerSize, CloudValue::Count(n)) => target.server_size = Some(*n),
                _ => {}
            }
        }
        out
    }

    /// The scopes the pending edits need, deduplicated, in `CloudField::ALL` order.
    pub(super) fn scopes(&self) -> Vec<&'static str> {
        let mut scopes: Vec<&'static str> = Vec::new();
        for field in CloudField::ALL {
            if self.is_edited(field) && !scopes.contains(&field.scope()) {
                scopes.push(field.scope());
            }
        }
        scopes
    }
}

/// The place file's half: one slot per [`LOCAL_FIELDS`] entry, by index.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct LocalDraft {
    /// The value when read: the property if the service sets it, else the
    /// reflection default; `None` when the place has no such service.
    original: Vec<Option<Variant>>,
    edits: BTreeMap<usize, Variant>,
}

impl LocalDraft {
    pub(super) fn read(dom: &WeakDom, db: &ReflectionDatabase) -> Self {
        let original = LOCAL_FIELDS
            .iter()
            .map(|f| {
                let instance = dom.get(service(dom, f.class)?)?;
                match instance.properties().get(f.property) {
                    Some(v) => Some(v.clone()),
                    None => db.default_value(f.class, f.property).cloned(),
                }
            })
            .collect();
        Self {
            original,
            edits: BTreeMap::new(),
        }
    }

    /// Takes `fresh`'s originals (the place changed under the window, by
    /// undo or the Properties panel) and keeps the edits that still differ
    /// from them. Returns whether any original changed.
    pub(super) fn rebase(&mut self, fresh: LocalDraft) -> bool {
        if self.original == fresh.original {
            return false;
        }
        let edits = std::mem::take(&mut self.edits);
        self.original = fresh.original;
        for (index, value) in edits {
            self.set(index, value);
        }
        true
    }

    pub(super) fn value(&self, index: usize) -> Option<&Variant> {
        self.edits
            .get(&index)
            .or_else(|| self.original.get(index)?.as_ref())
    }

    /// Records an edit; one equal to the original is forgotten instead.
    /// Ignored for a field whose service is missing.
    pub(super) fn set(&mut self, index: usize, value: Variant) {
        let Some(Some(original)) = self.original.get(index) else {
            return;
        };
        if *original == value {
            self.edits.remove(&index);
        } else {
            self.edits.insert(index, value);
        }
    }

    pub(super) fn is_edited(&self, index: usize) -> bool {
        self.edits.contains_key(&index)
    }

    pub(super) fn is_dirty(&self) -> bool {
        !self.edits.is_empty()
    }

    /// The edited fields with their new values, in index order.
    pub(super) fn writes(&self) -> Vec<(&'static LocalField, Variant)> {
        self.edits
            .iter()
            .map(|(&i, v)| (&LOCAL_FIELDS[i], v.clone()))
            .collect()
    }
}

/// The first root instance of `class` — services sit at the root.
pub(super) fn service(dom: &WeakDom, class: &str) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .copied()
        .find(|&r| dom.get(r).is_some_and(|i| i.class() == class))
}

/// `text` as `field`'s value, or why it isn't one: a float in the field's
/// range for `Float`, an item name (case-insensitive) for `Enum`.
pub(super) fn parse_local(
    field: &LocalField,
    db: &ReflectionDatabase,
    text: &str,
) -> Result<Variant, String> {
    let text = text.trim();
    match field.kind {
        LocalKind::Bool => match text.to_ascii_lowercase().as_str() {
            "true" => Ok(Variant::Bool(true)),
            "false" => Ok(Variant::Bool(false)),
            _ => Err("Enter true or false.".to_string()),
        },
        LocalKind::Float { min, max } => {
            let range = format!("Enter a number from {min} to {max}.");
            match text.parse::<f32>() {
                Ok(v) if v.is_finite() && (min..=max).contains(&v) => Ok(Variant::Float32(v)),
                _ => Err(range),
            }
        }
        LocalKind::Enum(name) => {
            let items = db.enum_items(name).unwrap_or(&[]);
            items
                .iter()
                .find(|(item, _)| item.eq_ignore_ascii_case(text))
                .map(|(_, value)| Variant::Enum(*value))
                .ok_or_else(|| {
                    let names: Vec<&str> = items.iter().map(|(n, _)| n.as_str()).collect();
                    format!("Enter one of: {}.", names.join(", "))
                })
        }
    }
}
