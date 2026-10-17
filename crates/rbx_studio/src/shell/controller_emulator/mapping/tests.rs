use super::*;

fn held(keys: &[&str]) -> HashSet<String> {
    keys.iter().map(|key| key.to_string()).collect()
}

#[test]
fn key_labels_read_as_printed_on_the_keyboard() {
    assert_eq!(key_label("w"), "W");
    assert_eq!(key_label("space"), "Space");
    assert_eq!(key_label("up"), "↑");
}

#[test]
fn defaults_bind_every_control_to_its_own_key() {
    let bindings = defaults();
    assert_eq!(bindings.len(), Control::all().len());
    let keys: HashSet<&String> = bindings.values().collect();
    assert_eq!(keys.len(), bindings.len());
}

#[test]
fn wasd_drives_the_left_stick_and_opposites_cancel() {
    let bindings = defaults();
    let state = state_for(&held(&["w", "d"]), &bindings);
    assert_eq!(state.stick(Side::Left), (1., 1.));
    assert_eq!(state.stick(Side::Right), (0., 0.));
    let state = state_for(&held(&["a", "d", "s"]), &bindings);
    assert_eq!(state.stick(Side::Left), (0., -1.));
}

#[test]
fn keys_press_buttons_and_pull_triggers() {
    let state = state_for(&held(&["space", "up", "c"]), &defaults());
    assert!(state.pressed(Button::South));
    assert!(state.pressed(Button::DpadUp));
    assert!(!state.pressed(Button::East));
    assert_eq!(state.trigger(Side::Right), 1.);
}

#[test]
fn binding_a_taken_key_moves_it_and_names_the_loser() {
    let mut bindings = defaults();
    let south = Control::Button(Button::South);
    let east = Control::Button(Button::East);
    assert_eq!(bind(&mut bindings, east, "space"), Some(south));
    assert_eq!(bindings.get(&east).map(String::as_str), Some("space"));
    assert!(!bindings.contains_key(&south));
    assert_eq!(bind(&mut bindings, east, "space"), None);
}

#[test]
fn the_keyboard_overrides_a_dragged_stick_only_while_held() {
    let mut pointer = PadState::default();
    pointer.set_stick(Side::Left, 0.3, 0.2);
    pointer.set_trigger(Side::Left, 0.4);
    let idle = merge(&pointer, &PadState::default());
    assert_eq!(idle.stick(Side::Left), (0.3, 0.2));
    assert_eq!(idle.trigger(Side::Left), 0.4);
    let keys = state_for(&held(&["a", "z"]), &defaults());
    let both = merge(&pointer, &keys);
    assert_eq!(both.stick(Side::Left), (-1., 0.));
    assert_eq!(both.trigger(Side::Left), 1.);
}

#[test]
fn saved_choices_round_trip_and_garbage_falls_back() {
    let mut saved = Saved {
        enabled: true,
        device: Device::Ps5,
        ..Saved::default()
    };
    let mut bindings = defaults();
    bind(&mut bindings, Control::Trigger(Side::Left), "v");
    saved.set_bindings(Device::Ps5, bindings);
    assert_eq!(Saved::from_json(&saved.to_json()), saved);
    assert_eq!(Saved::from_json("not json"), Saved::default());
    assert_eq!(
        Saved::from_json(r#"{"device":"nope","mappings":{"xbox":{"bogus":"k"}}}"#)
            .bindings(Device::Xbox),
        Bindings::new()
    );
}

#[test]
fn save_writes_the_file_load_reads() {
    let dir = std::env::temp_dir().join(format!("rbx-ctrl-emu-{}", std::process::id()));
    let path = dir.join("controller_emulator.json");
    let saved = Saved {
        enabled: true,
        ..Saved::default()
    };
    save_to(&saved, &path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(Saved::from_json(&text), saved);
    let _ = std::fs::remove_dir_all(dir);
}
