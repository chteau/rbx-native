//! The panel's rows for a selection: one instance's own, or — for several —
//! the properties every one of them has, the way Studio's panel shows a
//! multi-selection.
//!
//! A value every instance shares shows as usual. One that differs shows
//! Studio's mixed state: an empty field, an indeterminate checkbox (see
//! [`PropertyRow::mixed`]) — and, for a value made of parts, only the parts
//! that differ left empty, so a row of parts differing only in height still
//! shows the X and Z they share. Typing into a mixed row writes to all of
//! them (see `edit::commit_all`).

use std::rc::Rc;

use rbx_dom::{Instance, Ref, Variant, WeakDom};

use super::sheet::{Named, Sheet};
use super::{edit, folder_row, EditKind, Properties, PropertyRow};

impl Properties {
    /// Every row for `selection`, sorted by name; nothing for an empty one.
    /// `folder_color` is the tag `crate::folder_colors::FolderColors` holds
    /// for a lone selected `Folder` (see `shell::folder_color`), used only to
    /// seed its synthetic colour row — which a multi-selection never gets,
    /// since that store is written one folder at a time.
    pub(crate) fn rows(
        &self,
        dom: &WeakDom,
        selection: &[Ref],
        folder_color: Option<(u8, u8, u8)>,
    ) -> Vec<PropertyRow> {
        let mut instances = selection
            .iter()
            .filter_map(|&reference| Some((reference, dom.get(reference)?)));
        let Some((reference, anchor)) = instances.next() else {
            return Vec::new();
        };
        let class = anchor.class();
        let sheet = self.sheet(class);
        let named = self.named(dom, reference, anchor, &sheet);
        let others: Vec<(Ref, &Instance, Rc<Sheet>)> = instances
            .map(|(reference, instance)| (reference, instance, self.sheet(instance.class())))
            .collect();

        let mut rows: Vec<PropertyRow> = if others.is_empty() {
            let mut rows: Vec<PropertyRow> = named
                .iter()
                .map(|named| {
                    self.row(
                        dom,
                        class,
                        named.name,
                        named.category,
                        named.read_only,
                        &named.value,
                    )
                })
                .collect();
            rows.extend(folder_row::row(
                class,
                self.category(class, edit::FOLDER_COLOR_PROPERTY),
                folder_color,
            ));
            rows
        } else {
            self.common(dom, class, &named, &others)
        };
        rows.sort_by(|left, right| left.name.cmp(&right.name));
        rows
    }

    /// The anchor's rows that every other instance has too — the same
    /// property, declared by the same class — mixed where their values
    /// differ.
    fn common(
        &self,
        dom: &WeakDom,
        class: &str,
        anchor: &[Named],
        others: &[(Ref, &Instance, Rc<Sheet>)],
    ) -> Vec<PropertyRow> {
        // ponytail: rebuilt every frame, O(selected × properties) — a few
        // milliseconds for a thousand parts; memoise on the selection and a
        // DOM change count if selections that size ever make the panel lag.
        anchor
            .iter()
            .filter_map(|named| {
                let mut read_only = named.read_only;
                let mut values = Vec::with_capacity(others.len() + 1);
                values.push(named.value.clone());
                for (reference, instance, sheet) in others {
                    let (value, locked) = self.value_in(named, dom, *reference, instance, sheet)?;
                    read_only |= locked;
                    values.push(value);
                }
                let row = self.row(
                    dom,
                    class,
                    named.name,
                    named.category,
                    read_only,
                    &named.value,
                );
                if values.iter().all(|value| *value == named.value) {
                    return Some(row);
                }
                let values: Vec<&Variant> = values.iter().map(|value| &**value).collect();
                Some(mixed(row, &values))
            })
            .collect()
    }
}

/// `row` as it shows when `values` — every selected instance's — are not
/// all the same.
fn mixed(mut row: PropertyRow, values: &[&Variant]) -> PropertyRow {
    row.value.clear();
    row.mixed = true;
    row.edit = match row.edit.take() {
        Some(EditKind::Text(_)) => Some(EditKind::Text(String::new())),
        Some(EditKind::Enum { items, .. }) => Some(EditKind::Enum {
            current: String::new(),
            items,
        }),
        // Drawn from `row.mixed`: an indeterminate box, an empty swatch.
        kind @ Some(EditKind::Bool(_) | EditKind::Color { .. }) => kind,
        Some(EditKind::Fields {
            fields,
            values: seeds,
        }) => Some(EditKind::Fields {
            fields,
            values: shared_parts(seeds, values),
        }),
        Some(EditKind::Groups {
            groups,
            values: seeds,
        }) => Some(EditKind::Groups {
            groups,
            values: shared_parts(seeds, values),
        }),
        // A set of flags, a half-present value and a curve have no empty
        // form to show; read-only until they agree again.
        _ => None,
    };
    row
}

/// The anchor's parts, each kept only where every value has the same one.
fn shared_parts(seeds: Vec<String>, values: &[&Variant]) -> Vec<String> {
    let parts: Vec<Vec<String>> = values
        .iter()
        .filter_map(|value| edit::edit_text(value))
        .map(|text| text.split(',').map(|part| part.trim().to_owned()).collect())
        .collect();
    seeds
        .into_iter()
        .enumerate()
        .map(|(index, seed)| {
            let shared = parts.iter().all(|other| other.get(index) == Some(&seed));
            if shared {
                seed
            } else {
                String::new()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
