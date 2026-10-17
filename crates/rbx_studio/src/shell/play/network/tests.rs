use rbx_play::netsim::{OneWay, PRESETS};

use super::{Choice, Field, NetworkSim, Way};

#[test]
fn off_is_the_default_and_picking_only_stages() {
    let mut network = NetworkSim::new(Vec::new());
    assert_eq!(network.applied.choice, Choice::Off);
    assert_eq!(network.shown().pill(), "Off");

    network.stage(Choice::BuiltIn(3));
    assert_eq!(network.shown().pill(), "4G");
    assert_eq!(network.applied.choice, Choice::Off);
    network.commit();
    assert_eq!(network.applied.conditions, PRESETS[3].conditions);
}

#[test]
fn editing_a_value_makes_it_custom_but_reshowing_it_does_not() {
    let mut network = NetworkSim::new(Vec::new());
    network.stage(Choice::BuiltIn(0));
    assert!(!network.edit(Way::Inbound, Field::Latency, 8.0));
    assert_eq!(network.shown().choice, Choice::BuiltIn(0));

    assert!(network.edit(Way::Outbound, Field::Loss, 3.0));
    let shown = network.shown();
    assert_eq!(shown.choice, Choice::Custom);
    assert_eq!(shown.pill(), "Custom");
    // Clamped to Studio's 0.50 % ceiling.
    assert_eq!(shown.conditions.outbound, OneWay::new(8.0, 0.0, 0.5));
}

#[test]
fn reset_stages_ideal_fiber_without_applying() {
    let mut network = NetworkSim::new(Vec::new());
    network.stage(Choice::BuiltIn(4));
    network.commit();
    network.reset();
    assert_eq!(network.shown().choice, Choice::BuiltIn(0));
    assert_eq!(network.applied.choice, Choice::BuiltIn(4));
}

#[test]
fn save_rejects_reserved_names_and_replaces_its_own() {
    let mut network = NetworkSim::new(Vec::new());
    network.edit(Way::Inbound, Field::Latency, 40.0);
    assert!(network.save("Custom").is_err());
    assert!(network.save(" Home Wi-Fi ").is_err());
    assert!(network.save("  ").is_err());

    assert_eq!(network.check_name("Office"), Ok(false));
    network.save(" Office ").unwrap();
    assert_eq!(network.shown().choice, Choice::Saved("Office".to_owned()));
    assert_eq!(network.check_name("Office"), Ok(true));

    network.edit(Way::Inbound, Field::Latency, 60.0);
    network.save("Office").unwrap();
    assert_eq!(network.saved.len(), 1);
    assert_eq!(network.saved[0].conditions.inbound.latency_ms, 60.0);
}

#[test]
fn delete_removes_the_saved_preset_and_stages_ideal_fiber() {
    let mut network = NetworkSim::new(Vec::new());
    network.edit(Way::Outbound, Field::Jitter, 5.0);
    network.save("Jittery").unwrap();
    network.commit();
    network.staged = None;

    network.stage(Choice::Saved("Jittery".to_owned()));
    assert!(network.delete());
    assert!(network.saved.is_empty());
    assert_eq!(network.shown().choice, Choice::BuiltIn(0));
    assert!(!network.delete());
}
