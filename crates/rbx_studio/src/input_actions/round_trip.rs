use std::path::Path;

use rbx_dom::{NumberRange, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::properties::edit::commit;
use crate::save::Format;

const BINDING_PROPERTIES: [&str; 24] = [
    "KeyCode",
    "Up",
    "Down",
    "Left",
    "Right",
    "Forward",
    "Backward",
    "PrimaryModifier",
    "SecondaryModifier",
    "Type",
    "UIButton",
    "UIModifier",
    "PressedThreshold",
    "ReleasedThreshold",
    "ResponseCurve",
    "Scale",
    "Vector2Scale",
    "Vector3Scale",
    "ClampMagnitudeToOne",
    "Deadzone",
    "DisplayName",
    "DisplayImage",
    "PointerIndex",
    "Name",
];

fn find(dom: &WeakDom, name: &str) -> Ref {
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let instance = dom.get(referent).expect("listed instance exists");
        if instance.name() == name {
            return referent;
        }
        stack.extend_from_slice(instance.children());
    }
    panic!("no instance named {name}");
}

/// A property's value as text, with a reference replaced by its target's name
/// so two DOMs with different referents compare equal.
fn show(dom: &WeakDom, db: &ReflectionDatabase, referent: Ref, property: &str) -> String {
    if property == "Name" {
        return dom.get(referent).expect("instance").name().to_owned();
    }
    match value_of(dom, db, referent, property) {
        None => "absent".to_owned(),
        Some(Variant::Ref(target)) => match dom.get(*target) {
            Some(instance) => format!("ref:{}", instance.name()),
            None => "nil".to_owned(),
        },
        Some(other) => format!("{other:?}"),
    }
}

fn snapshot(dom: &WeakDom, db: &ReflectionDatabase, referent: Ref) -> Vec<String> {
    BINDING_PROPERTIES
        .iter()
        .map(|property| format!("{property}={}", show(dom, db, referent, property)))
        .collect()
}

fn describe(dom: &WeakDom, model: &Model) -> Vec<String> {
    let name_of = |r: Ref| dom.get(r).map(|i| i.name().to_owned()).unwrap_or_default();
    let mut lines = Vec::new();
    let actions = model
        .contexts
        .iter()
        .flat_map(|c| {
            lines.push(format!(
                "context {} {} {} {}",
                c.name, c.enabled, c.priority, c.sink
            ));
            c.actions.iter()
        })
        .chain(model.loose.iter())
        .collect::<Vec<_>>();
    for action in actions {
        lines.push(format!(
            "action {} {:?} {}",
            action.name, action.kind, action.enabled
        ));
        for b in &action.bindings {
            lines.push(format!(
                "binding {} {} {:?} {:?} {:?} {:?} {:?} {} {} {} {} {} {} {}",
                b.name,
                b.scriptable,
                b.keys,
                b.modifiers,
                b.ui_button.as_ref().map(|(_, n)| n.clone()),
                b.ui_modifier.map(name_of),
                b.families,
                b.pressed_threshold,
                b.released_threshold,
                b.response_curve,
                b.scale,
                b.vector2_scaled,
                b.vector3_scaled,
                b.summary(),
            ));
        }
    }
    lines
}

fn build(db: &ReflectionDatabase) -> WeakDom {
    let mut dom = WeakDom::new();
    let game = dom.new_instance("DataModel", "Game", None);
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", Some(game));
    let gui = dom.new_instance("StarterGui", "StarterGui", Some(game));
    let screen = dom.new_instance("ScreenGui", "Screen", Some(gui));
    let button = dom.new_instance("TextButton", "SprintButton", Some(screen));
    let modifier = dom.new_instance("TextButton", "ModifierButton", Some(screen));
    let context = dom.new_instance("InputContext", "Gameplay", Some(storage));
    let action = dom.new_instance("InputAction", "Move", Some(context));
    let binding = dom.new_instance("InputBinding", "MoveKeys", Some(action));

    let edits: [(Ref, &str, String); 29] = [
        (context, "Priority", "2000".into()),
        (context, "Sink", "true".into()),
        (context, "Enabled", "false".into()),
        (action, "Type", "Direction2D".into()),
        (action, "DisplayName", "Move around".into()),
        (action, "Enabled", "false".into()),
        (binding, "KeyCode", "E".into()),
        (binding, "Up", "W".into()),
        (binding, "Down", "S".into()),
        (binding, "Left", "A".into()),
        (binding, "Right", "D".into()),
        (binding, "Forward", "Space".into()),
        (binding, "Backward", "LeftShift".into()),
        (binding, "PrimaryModifier", "LeftControl".into()),
        (binding, "SecondaryModifier", "LeftShift".into()),
        (binding, "Type", "Scriptable".into()),
        (binding, "UIButton", button.value().to_string()),
        (binding, "UIModifier", modifier.value().to_string()),
        (binding, "PressedThreshold", "0.75".into()),
        (binding, "ReleasedThreshold", "0.25".into()),
        (binding, "ResponseCurve", "3.5".into()),
        (binding, "Scale", "2.5".into()),
        (binding, "Vector2Scale", "2, 3".into()),
        (binding, "Vector3Scale", "2, 3, 4".into()),
        (binding, "ClampMagnitudeToOne", "false".into()),
        (binding, "DisplayName", "Walk".into()),
        (binding, "DisplayImage", "rbxassetid://123456".into()),
        (binding, "PointerIndex", "2".into()),
        (binding, "Name", "MoveKeys".into()),
    ];
    for (referent, property, text) in &edits {
        commit(&mut dom, db, *referent, property, text)
            .unwrap_or_else(|err| panic!("commit {property}={text}: {err}"));
    }
    dom.set_property(
        binding,
        "Deadzone",
        Variant::NumberRange(NumberRange { min: 0.1, max: 0.9 }),
    )
    .expect("Deadzone");
    dom
}

fn reload(dom: &WeakDom, format: Format) -> WeakDom {
    let bytes = format.encode(dom).expect("encode");
    match format {
        Format::Binary => rbx_binary::deserialize(&bytes).expect("binary decode"),
        Format::Xml => {
            rbx_xml::deserialize(std::str::from_utf8(&bytes).expect("utf8")).expect("xml decode")
        }
    }
}

fn check_format(format: Format) {
    let db = ReflectionDatabase::shared();
    let original = build(db);
    let reloaded = reload(&original, format);

    for (class_name, props) in [
        ("Gameplay", &["Priority", "Sink", "Enabled", "Name"][..]),
        ("Move", &["Type", "DisplayName", "Enabled", "Name"][..]),
        ("MoveKeys", &BINDING_PROPERTIES[..]),
    ] {
        let a = find(&original, class_name);
        let b = find(&reloaded, class_name);
        for property in props {
            assert_eq!(
                show(&original, db, a, property),
                show(&reloaded, db, b, property),
                "{format:?}: {class_name}.{property} changed"
            );
        }
    }
    let binding = find(&original, "MoveKeys");
    assert_eq!(
        snapshot(&original, db, binding),
        snapshot(&reloaded, db, find(&reloaded, "MoveKeys")),
        "{format:?}"
    );
    assert_eq!(
        describe(&original, &model(&original, db)),
        describe(&reloaded, &model(&reloaded, db)),
        "{format:?}: model differs"
    );
}

#[test]
fn binary_keeps_every_input_property() {
    check_format(Format::Binary);
}

#[test]
fn xml_keeps_every_input_property() {
    check_format(Format::Xml);
}

#[test]
fn edited_values_are_not_defaults() {
    let db = ReflectionDatabase::shared();
    let dom = build(db);
    let read = model(&dom, db);
    assert_eq!(read.contexts.len(), 1);
    let context = &read.contexts[0];
    assert_eq!(
        (context.priority, context.sink, context.enabled),
        (2000, true, false)
    );
    let binding = &context.actions[0].bindings[0];
    assert!(binding.scriptable);
    assert!(binding.vector2_scaled && binding.vector3_scaled);
    assert_eq!(
        binding.ui_button.as_ref().map(|(_, n)| n.as_str()),
        Some("SprintButton")
    );
}

#[test]
#[ignore = "needs RBX_INPUT_ACTIONS_FIXTURE, a Studio-saved place"]
fn studio_saved_place_survives_binary_re_encode() {
    let path = std::env::var("RBX_INPUT_ACTIONS_FIXTURE")
        .expect("set RBX_INPUT_ACTIONS_FIXTURE to a Studio-saved place");
    let db = ReflectionDatabase::shared();
    let dom = rbx_viewer::read_place(Path::new(&path)).expect("read fixture");
    let read = model(&dom, db);
    assert!(!read.contexts.is_empty(), "fixture has no InputContext");

    // A Studio place carries binary-only property kinds the XML writer has
    // no encoding for (`Terrain.Materials`, empty `Tags` blobs). They are
    // dropped from the XML pass only; none belongs to the Input classes.
    let mut xml_ready = dom.clone();
    let mut stack: Vec<Ref> = xml_ready.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let instance = xml_ready.get_mut(referent).expect("listed instance exists");
        instance
            .properties_mut()
            .retain(|_, value| !matches!(value, rbx_dom::Variant::Unknown { .. }));
        stack.extend_from_slice(instance.children());
    }
    for (format, source) in [(Format::Binary, &dom), (Format::Xml, &xml_ready)] {
        let again = reload(source, format);
        for context in &read.contexts {
            for action in &context.actions {
                for binding in &action.bindings {
                    eprintln!("{}/{}: {}", action.name, binding.name, binding.summary());
                    let after = find(&again, &binding.name);
                    assert_eq!(
                        snapshot(&dom, db, binding.referent),
                        snapshot(&again, db, after),
                        "binding {} changed through {format:?}",
                        binding.name
                    );
                }
            }
        }
    }
}
