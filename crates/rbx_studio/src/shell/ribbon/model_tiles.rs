//! The Model tab's tiles that act on the whole selection's parts at once:
//! Material, Color, Lock, Anchor, and the Import menu. Each is one undo
//! step however many parts it reaches, written through the same
//! `properties::edit::commit_all` the Properties panel uses.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, IntoElement as _, StatefulInteractiveElement as _, Styled as _,
};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{selected, tile, unavailable_tile};
use crate::shell::brick_color::OnPick;
use crate::shell::import3d::ImportKind;
use crate::shell::menu::{self, MenuId};
use crate::shell::{chrome, Shell};
use crate::{properties, tokens};

const NO_PART: &str = "no part is selected";

/// `Material` members that are terrain-only: a part can't be made of them.
const TERRAIN_ONLY: [&str; 2] = ["Air", "Water"];

/// Every `BasePart` the selection reaches: the selected parts themselves and
/// the parts under selected models, as Studio's Material and Color do.
/// Each once, in tree order.
pub(super) fn target_parts(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    selection: &[Ref],
) -> Vec<Ref> {
    let mut seen = HashSet::new();
    let mut parts = Vec::new();
    let mut stack: Vec<Ref> = selection.iter().rev().copied().collect();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        if !seen.insert(referent) {
            continue;
        }
        if database.is_subclass_of(instance.class(), "BasePart") {
            parts.push(referent);
        }
        stack.extend(instance.children().iter().rev());
    }
    parts
}

/// Whether a viewport click must pass through this instance: Studio does not
/// let a locked part be clicked or dragged in the viewport (it stays
/// selectable in the Explorer).
pub(in crate::shell) fn is_locked(dom: &WeakDom, referent: Ref) -> bool {
    bool_of(dom, referent, "Locked")
}

fn bool_of(dom: &WeakDom, referent: Ref, name: &str) -> bool {
    dom.get(referent)
        .is_some_and(|i| matches!(i.properties().get(name), Some(Variant::Bool(true))))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Flag {
    Locked,
    Anchored,
}

impl Flag {
    fn property(self) -> &'static str {
        match self {
            Flag::Locked => "Locked",
            Flag::Anchored => "Anchored",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    Off,
    On,
    Mixed,
}

pub(super) fn state(dom: &WeakDom, parts: &[Ref], property: &str) -> State {
    let on = parts.iter().filter(|&&p| bool_of(dom, p, property)).count();
    match on {
        0 => State::Off,
        n if n == parts.len() => State::On,
        _ => State::Mixed,
    }
}

/// Studio's toggle: all set clears them, anything else sets every one.
pub(super) fn toggle_target(state: State) -> bool {
    state != State::On
}

/// Material names in menu order, without the terrain-only ones.
pub(super) fn materials(database: &ReflectionDatabase) -> Vec<String> {
    let mut names: Vec<String> = database
        .enum_items("Material")
        .unwrap_or_default()
        .iter()
        .map(|(name, _)| name.clone())
        .filter(|name| !TERRAIN_ONLY.contains(&name.as_str()))
        .collect();
    names.sort();
    names
}

impl Shell {
    /// Writes `property = text` on every part the selection reaches, as one
    /// undo step. Nothing is pushed when no value changed.
    fn apply_to_parts(&mut self, property: &str, text: &str, cx: &mut Context<Self>) {
        let parts = target_parts(&self.dom, &self.database, self.selected_all());
        if parts.is_empty() {
            return;
        }
        self.dom.take_changes();
        let before = self.dom.clone();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        // Every value is parsed before any is written, and ours are fixed
        // known-good texts, so a failure leaves the DOM untouched.
        let _ = properties::edit::commit_all(&mut dom, &self.database, &parts, property, text);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        if !changes.is_empty() {
            self.push_history_snapshot(before);
            self.record_history_change(changes);
        }
        cx.notify();
    }

    fn selected_parts(&self) -> Vec<Ref> {
        target_parts(&self.dom, &self.database, self.selected_all())
    }

    pub(super) fn material_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.selected_parts().is_empty() {
            return unavailable_tile("ribbon-material", IconName::Layers, "Material", NO_PART)
                .into_any_element();
        }
        let items = materials(&self.database)
            .into_iter()
            .map(|name| {
                let label = name.clone();
                menu::item(label)
                    .on_click(move |shell, cx| shell.apply_to_parts("Material", &name, cx))
            })
            .collect();
        let trigger = tile(
            &self.ribbon_nav,
            "ribbon-material",
            IconName::Layers,
            "Material",
            cx,
        );
        menu::dropdown(
            self,
            MenuId::RibbonMaterial,
            chrome::Trigger::new(trigger),
            items,
            cx,
        )
        .into_any_element()
    }

    pub(super) fn color_tile(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.selected_parts().is_empty() {
            return unavailable_tile("ribbon-color", IconName::Droplet, "Color", NO_PART)
                .into_any_element();
        }
        let trigger = tile(
            &self.ribbon_nav,
            "ribbon-color",
            IconName::Droplet,
            "Color",
            cx,
        );
        let on_pick: OnPick = Rc::new(|shell, number, cx| {
            shell.apply_to_parts("BrickColor", &number.to_string(), cx)
        });
        self.brick_color_popover("ribbon-color-popover", trigger, on_pick, cx)
            .into_any_element()
    }

    /// Lock and Anchor: lit when every selected part has the flag, dimmer
    /// when only some do, and a click sets them all (or clears them all
    /// when they were all set).
    pub(super) fn flag_tile(&self, flag: Flag, cx: &mut Context<Self>) -> AnyElement {
        let (id, icon, label) = match flag {
            Flag::Locked => ("ribbon-lock", IconName::Lock, "Lock"),
            Flag::Anchored => ("ribbon-anchor", IconName::Anchor, "Anchor"),
        };
        let parts = self.selected_parts();
        if parts.is_empty() {
            return unavailable_tile(id, icon, label, NO_PART).into_any_element();
        }
        let property = flag.property();
        let state = state(&self.dom, &parts, property);
        tile(&self.ribbon_nav, id, icon, label, cx)
            .when(state == State::On, |this| {
                selected(this, tokens::accent_line())
            })
            .when(state == State::Mixed, |this| {
                this.bg(tokens::accent_soft())
                    .text_color(tokens::check_on())
            })
            .when(state == State::Mixed, |this| {
                this.tooltip(move |window, cx| {
                    crate::shell::tooltip::text(
                        format!("{label} — only some of the selected parts are set"),
                        window,
                        cx,
                    )
                })
            })
            .on_click(cx.listener(move |shell, _, _, cx| {
                let value = toggle_target(state);
                shell.apply_to_parts(property, if value { "true" } else { "false" }, cx);
            }))
            .into_any_element()
    }

    /// Studio's Import 3D opens the model importer; the menu bar's Mesh and
    /// Image imports sit beside it here.
    pub(super) fn import_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let item = |label: &'static str, kind: ImportKind| {
            menu::item(label).on_click(move |shell, cx| shell.choose_import(kind, cx))
        };
        let trigger = tile(
            &self.ribbon_nav,
            "ribbon-import",
            IconName::Import,
            "Import",
            cx,
        );
        vec![menu::dropdown(
            self,
            MenuId::RibbonImport,
            chrome::Trigger::new(trigger),
            vec![
                item("Model", ImportKind::Model),
                item("Mesh", ImportKind::Mesh),
                item("Image", ImportKind::Image),
            ],
            cx,
        )
        .into_any_element()]
    }
}

#[cfg(test)]
mod tests;
