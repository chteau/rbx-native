use std::collections::BTreeSet;

use super::{kind, search, Code, PinType, Wanted, KINDS};

#[test]
fn every_key_is_unique_and_resolves() {
    let keys: BTreeSet<_> = KINDS.iter().map(|kind| kind.key).collect();
    assert_eq!(keys.len(), KINDS.len());
    for key in keys {
        assert_eq!(kind(key).unwrap().key, key);
    }
}

#[test]
fn every_template_names_only_its_own_inputs() {
    for kind in KINDS {
        let template = match kind.code {
            Code::Statement(t) | Code::Event(Some(t)) => t,
            Code::Expression { template, .. } => template,
            _ => continue,
        };
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            let close = open + rest[open..].find('}').unwrap();
            let pin = rest[open + 1..close].trim_start_matches('.');
            assert!(kind.input(pin).is_some(), "{}: no input {pin}", kind.key);
            rest = &rest[close + 1..];
        }
    }
}

#[test]
fn run_order_pins_lead_each_side() {
    for kind in KINDS {
        for pins in [kind.inputs, kind.outputs] {
            let first_value = pins.iter().position(|pin| pin.ty != PinType::Exec);
            let last_exec = pins.iter().rposition(|pin| pin.ty == PinType::Exec);
            if let (Some(value), Some(exec)) = (first_value, last_exec) {
                assert!(exec < value, "{}: a run pin after a value pin", kind.key);
            }
        }
    }
}

#[test]
fn search_matches_every_word_and_groups_by_section() {
    let found = search("get", None);
    assert!(found
        .iter()
        .all(|kind| kind.title.to_lowercase().contains("get")));
    assert!(found
        .windows(2)
        .all(|pair| pair[0].category <= pair[1].category));
    assert_eq!(search("child class", None)[0].key, "find_child_of_class");
}

#[test]
fn a_dragged_wire_narrows_the_menu_to_nodes_it_can_end_on() {
    let found = search("get", Some(Wanted::Input(PinType::Instance)));
    let keys: Vec<_> = found.iter().map(|kind| kind.key).collect();
    assert!(keys.contains(&"get_parent"));
    assert!(keys.contains(&"get_property"));
    assert!(!keys.contains(&"get_service"));
    let wanted = Wanted::Output(PinType::Bool);
    let found = search("", Some(wanted));
    assert!(found.iter().all(|kind| wanted.pin(kind).is_some()));
    assert!(found.iter().any(|kind| kind.key == "is_valid"));
}

#[test]
fn any_takes_every_value_but_never_the_run_order() {
    assert!(PinType::Any.accepts(PinType::Instance));
    assert!(PinType::Number.accepts(PinType::Any));
    assert!(!PinType::Any.accepts(PinType::Exec));
    assert!(!PinType::Exec.accepts(PinType::Any));
    assert!(!PinType::Number.accepts(PinType::String));
}
