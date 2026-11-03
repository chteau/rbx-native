//! Copy Properties / Paste Properties on the Explorer's menu, from the
//! DevForum's "Copy Properties From Object" request
//! (https://devforum.roblox.com/t/copy-properties-from-object/224274); real
//! Studio has no such thing. Copies every saved property value of one
//! instance and pastes each one the target's class also has onto every
//! selected instance, as one undo step.
//!
//! Kept app-wide (a gpui `Global`) rather than per window so a set copied in
//! one place pastes into another.
//!
//! What is never copied, and why: identity (`UniqueId`, `HistoryId`,
//! `SourceAssetId`), a script's code (`Source`, `LinkedSource`,
//! `ScriptGuid`), where it sits (`CFrame`, `WorldPivotData`, pasting those
//! would stack every target on the source), `Ref` values (they point at the
//! source's own neighbours) and opaque blobs (`BinaryString` and
//! `SharedString` values: tags, attributes, a union's mesh, which have
//! editors of their own). A `BinaryString` loads as a plain
//! `Variant::String`, so those are skipped by name.

use gpui_kit::*;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::command_bar::Feedback;

use super::Shell;

const NEVER_COPIED: [&str; 17] = [
    "UniqueId",
    "HistoryId",
    "SourceAssetId",
    "Source",
    "LinkedSource",
    "ScriptGuid",
    "CFrame",
    "WorldPivotData",
    "Tags",
    "AttributesSerialize",
    "ChildData",
    "MeshData",
    "PhysicsData",
    "PhysicalConfigData",
    "SmoothGrid",
    "PhysicsGrid",
    "MaterialColors",
];

/// The last set copied: the source's class and name for the report, and its
/// values sorted by property name.
#[derive(Clone)]
pub(super) struct CopiedProperties {
    pub(super) class: String,
    pub(super) name: String,
    pub(super) values: Vec<(String, Variant)>,
}

impl Global for CopiedProperties {}

pub(super) fn copy_of(dom: &WeakDom, reference: Ref) -> Option<CopiedProperties> {
    let instance = dom.get(reference)?;
    let values = instance
        .properties()
        .iter()
        .filter(|(name, value)| {
            !NEVER_COPIED.contains(&name.as_str())
                && !matches!(
                    value,
                    Variant::Ref(_)
                        | Variant::SharedString(_)
                        | Variant::UniqueId(_)
                        | Variant::Unknown { .. }
                )
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    Some(CopiedProperties {
        class: instance.class().to_string(),
        name: instance.name().to_string(),
        values,
    })
}

/// The copied values `target` takes: its class has the property (or it
/// already holds one), of the same type as what it holds, and different from
/// it.
pub(super) fn applicable(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    copied: &CopiedProperties,
    target: Ref,
) -> Vec<(String, Variant)> {
    let Some(instance) = dom.get(target) else {
        return Vec::new();
    };
    copied
        .values
        .iter()
        .filter(|(name, value)| match instance.properties().get(name) {
            Some(held) => {
                std::mem::discriminant(held) == std::mem::discriminant(value) && held != value
            }
            None => db.resolve_property(instance.class(), name).is_some(),
        })
        .cloned()
        .collect()
}

pub(super) fn applies_to_any(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    copied: &CopiedProperties,
    targets: &[Ref],
) -> bool {
    targets
        .iter()
        .any(|&target| !applicable(dom, db, copied, target).is_empty())
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Shell {
    pub(super) fn copy_properties_of(&mut self, target: Ref, cx: &mut Context<Self>) {
        let Some(copied) = copy_of(&self.dom, target) else {
            return;
        };
        let message = format!(
            "Copied {} of {} \"{}\"",
            count(copied.values.len(), "property", "properties"),
            copied.class,
            copied.name
        );
        cx.set_global(copied);
        self.report_property_clipboard("Copy Properties", Feedback::Output(message), cx);
    }

    pub(super) fn paste_properties(&mut self, cx: &mut Context<Self>) {
        let Some(copied) = cx.try_global::<CopiedProperties>().cloned() else {
            return;
        };
        let before = self.history_snapshot();
        let targets = self.selected_all().to_vec();
        let (mut instances, mut written) = (0usize, 0usize);
        for target in targets {
            let values = applicable(&self.dom, &self.database, &copied, target);
            let mut wrote = false;
            for (name, value) in values {
                if self.dom.set_property(target, &name, value).is_ok() {
                    written += 1;
                    wrote = true;
                }
            }
            if wrote {
                instances += 1;
            }
        }
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.commit_history_step(before, changes);
        let feedback = if written == 0 {
            Feedback::Error(format!(
                "None of the copied {} properties apply here, or they already match",
                copied.class
            ))
        } else {
            Feedback::Output(format!(
                "Pasted {} onto {}",
                count(written, "property", "properties"),
                count(instances, "instance", "instances")
            ))
        };
        self.report_property_clipboard("Paste Properties", feedback, cx);
    }

    fn report_property_clipboard(
        &mut self,
        source: &'static str,
        feedback: Feedback,
        cx: &mut Context<Self>,
    ) {
        self.output.push(source, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Ref, Variant, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    use super::{applicable, applies_to_any, copy_of, CopiedProperties};

    fn set(dom: &mut WeakDom, target: Ref, name: &str, value: Variant) {
        dom.set_property(target, name, value).unwrap();
    }

    fn part(dom: &mut WeakDom, class: &str) -> Ref {
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        dom.new_instance(class, "Thing", Some(workspace))
    }

    fn names(copied: &CopiedProperties) -> Vec<&str> {
        copied.values.iter().map(|(n, _)| n.as_str()).collect()
    }

    #[test]
    fn deny_listed_names_and_refs_are_not_copied() {
        let mut dom = WeakDom::new();
        let source = part(&mut dom, "Part");
        let other = dom.new_instance("Part", "Other", Some(source));
        set(&mut dom, source, "Transparency", Variant::Float32(0.5));
        set(&mut dom, source, "Source", Variant::String("x".into()));
        set(&mut dom, source, "SourceAssetId", Variant::Int64(7));
        set(&mut dom, source, "Neighbour", Variant::Ref(other));
        let copied = copy_of(&dom, source).unwrap();
        let copied_names = names(&copied);
        assert!(copied_names.contains(&"Transparency"));
        for denied in ["Source", "SourceAssetId", "Neighbour", "CFrame"] {
            assert!(!copied_names.contains(&denied), "{denied}");
        }
        assert_eq!(copied.class, "Part");
        assert_eq!(copied.name, "Thing");
        let mut sorted = copied_names.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, copied_names);
    }

    #[test]
    fn part_to_part_copies_values_but_not_position() {
        let db = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let source = part(&mut dom, "Part");
        let target = dom.new_instance("Part", "Target", None);
        set(&mut dom, source, "Transparency", Variant::Float32(0.5));
        let copied = copy_of(&dom, source).unwrap();
        let taken = applicable(&dom, &db, &copied, target);
        assert!(taken.iter().any(|(n, _)| n == "Transparency"));
        assert!(taken.iter().all(|(n, _)| n != "CFrame"));
    }

    #[test]
    fn base_part_values_reach_a_mesh_part_but_lighting_ones_miss_a_part() {
        let db = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let source = part(&mut dom, "Part");
        let mesh = dom.new_instance("MeshPart", "Mesh", None);
        let lighting = dom.new_instance("Lighting", "Lighting", None);
        set(&mut dom, source, "Transparency", Variant::Float32(0.5));
        set(&mut dom, lighting, "Brightness", Variant::Float32(3.0));
        let from_part = copy_of(&dom, source).unwrap();
        assert!(applicable(&dom, &db, &from_part, mesh)
            .iter()
            .any(|(n, _)| n == "Transparency"));
        let from_lighting = copy_of(&dom, lighting).unwrap();
        let ambient = CopiedProperties {
            values: vec![("Ambient".into(), Variant::Bool(true))],
            ..from_lighting
        };
        assert!(!applies_to_any(&dom, &db, &ambient, &[source]));
    }

    #[test]
    fn equal_values_and_type_mismatches_are_skipped() {
        let db = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let source = part(&mut dom, "Part");
        let target = dom.new_instance("Part", "Target", None);
        set(&mut dom, source, "Transparency", Variant::Float32(0.5));
        set(&mut dom, target, "Transparency", Variant::Float32(0.5));
        let copied = copy_of(&dom, source).unwrap();
        assert!(applicable(&dom, &db, &copied, target)
            .iter()
            .all(|(n, _)| n != "Transparency"));
        set(&mut dom, target, "Transparency", Variant::Int32(1));
        assert!(applicable(&dom, &db, &copied, target)
            .iter()
            .all(|(n, _)| n != "Transparency"));
    }
}
