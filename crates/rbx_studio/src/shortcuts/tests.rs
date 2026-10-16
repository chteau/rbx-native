use std::collections::HashSet;

use gpui_kit::{Keystroke, Modifiers};

use super::{bindable, display, parse, same_key, Category, Command, Scope, Store, ALL};

fn key(name: &str, modifiers: Modifiers) -> Keystroke {
    Keystroke {
        key: name.into(),
        modifiers,
        key_char: None,
    }
}

fn parsed(spelling: &str) -> Keystroke {
    parse(spelling).unwrap_or_else(|| panic!("`{spelling}` should parse"))
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

#[test]
fn every_id_is_unique() {
    let mut seen = HashSet::new();
    for spec in ALL {
        assert!(seen.insert(spec.id), "duplicate id `{}`", spec.id);
    }
}

#[test]
fn every_command_appears_once_in_all() {
    let commands: HashSet<Command> = ALL.iter().map(|spec| spec.command).collect();
    assert_eq!(commands.len(), ALL.len());
}

#[test]
fn every_default_parses() {
    for spec in ALL {
        for spelling in spec.defaults {
            assert!(
                parse(spelling).is_some(),
                "default `{spelling}` of `{}` does not parse",
                spec.id
            );
        }
    }
}

#[test]
fn from_id_round_trips_every_spec() {
    for spec in ALL {
        assert_eq!(Command::from_id(spec.id), Some(spec.command));
        assert_eq!(spec.command.spec().id, spec.id);
    }
    assert_eq!(Command::from_id("no_such_command"), None);
}

#[test]
fn category_scopes() {
    assert_eq!(Category::Edit.scope(), Scope::Global);
    assert_eq!(Category::Script.scope(), Scope::Script);
    assert_eq!(Category::UiEditor.scope(), Scope::UiEditor);
}

#[test]
fn default_store_bindings_match_parsed_defaults() {
    let store = Store::default();
    for spec in ALL {
        let expected: Vec<Keystroke> = spec.defaults.iter().map(|s| parsed(s)).collect();
        assert_eq!(store.bindings(spec.command), expected, "`{}`", spec.id);
        assert!(store.is_default(spec.command));
    }
}

#[test]
fn set_then_is_default_is_false() {
    let mut store = Store::default();
    store.set(Command::Save, vec![parsed("ctrl-q")]);
    assert!(!store.is_default(Command::Save));
    assert_eq!(store.bindings(Command::Save), vec![parsed("ctrl-q")]);
}

#[test]
fn setting_exactly_the_defaults_clears_the_override() {
    let mut store = Store::default();
    store.set(Command::Save, vec![parsed("ctrl-q")]);
    store.set(Command::Save, vec![parsed("secondary-s")]);
    assert!(store.is_default(Command::Save));
    assert_eq!(store, Store::default());
}

#[test]
fn empty_set_unbinds() {
    let mut store = Store::default();
    store.set(Command::Save, Vec::new());
    assert!(store.bindings(Command::Save).is_empty());
    assert!(!store.is_default(Command::Save));
}

#[test]
fn reset_restores_defaults_for_one_command() {
    let mut store = Store::default();
    store.set(Command::Save, vec![parsed("ctrl-q")]);
    store.set(Command::Undo, vec![parsed("ctrl-u")]);
    store.reset(Command::Save);
    assert!(store.is_default(Command::Save));
    assert!(!store.is_default(Command::Undo));
}

#[test]
fn reset_all_restores_every_default() {
    let mut store = Store::default();
    store.set(Command::Save, vec![parsed("ctrl-q")]);
    store.set(Command::Undo, Vec::new());
    store.reset_all();
    assert_eq!(store, Store::default());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn ctrl_s_presses_save_on_linux() {
    let store = Store::default();
    assert!(store.pressed(Command::Save, &parsed("ctrl-s")));
    assert!(!store.pressed(Command::Save, &parsed("ctrl-q")));
}

#[test]
fn same_key_ignores_key_name_case() {
    let upper = key("S", Modifiers::default());
    let lower = key("s", Modifiers::default());
    assert!(same_key(&upper, &lower));
    assert!(same_key(&lower, &upper));
}

#[test]
fn same_key_requires_each_modifier_to_match() {
    let base = parsed("s");
    let variants = ["ctrl-s", "alt-s", "shift-s", "cmd-s", "fn-s"];
    for spelling in variants {
        assert!(
            !same_key(&base, &parsed(spelling)),
            "`s` should not match `{spelling}`"
        );
    }
    let all_five = Modifiers {
        control: true,
        alt: true,
        shift: true,
        platform: true,
        function: true,
    };
    assert!(same_key(&key("s", all_five), &key("s", all_five)));
    assert!(!same_key(
        &key("s", all_five),
        &key(
            "s",
            Modifiers {
                control: true,
                alt: true,
                shift: true,
                platform: true,
                function: false,
            }
        )
    ));
}

#[test]
fn shifted_bracket_matches_its_base_key_with_shift() {
    assert!(same_key(&key("}", shift()), &parsed("shift-]")));
    assert!(same_key(&parsed("shift-]"), &key("}", shift())));
}

#[test]
fn shifted_digit_symbol_matches_its_base_key_with_shift() {
    assert!(same_key(&key("@", shift()), &parsed("shift-2")));
    assert!(same_key(&parsed("shift-2"), &key("@", shift())));
}

#[test]
fn json_round_trip_keeps_overrides() {
    let mut store = Store::default();
    store.set(Command::Save, vec![parsed("ctrl-q"), parsed("alt-q")]);
    store.set(Command::Undo, Vec::new());
    store.set(Command::Redo, vec![parsed("ctrl-y")]);
    let restored = Store::from_json(&store.to_json());
    assert_eq!(restored, store);
    assert!(restored.bindings(Command::Undo).is_empty());
}

#[test]
fn json_round_trip_of_defaults_is_empty() {
    assert_eq!(
        Store::from_json(&Store::default().to_json()),
        Store::default()
    );
}

#[test]
fn from_json_drops_unknown_ids() {
    let text = r#"{"overrides":{"no_such_command":["ctrl-s"],"save":["ctrl-q"]}}"#;
    let mut expected = Store::default();
    expected.set(Command::Save, vec![parsed("ctrl-q")]);
    assert_eq!(Store::from_json(text), expected);
}

#[test]
fn from_json_drops_bad_keys_and_keeps_good_ones() {
    let text = r#"{"overrides":{"save":["ctrl-q","frobnicate-s"]}}"#;
    let mut expected = Store::default();
    expected.set(Command::Save, vec![parsed("ctrl-q")]);
    assert_eq!(Store::from_json(text), expected);
}

#[test]
fn from_json_malformed_text_is_empty() {
    assert_eq!(Store::from_json("not json at all"), Store::default());
    assert_eq!(Store::from_json(""), Store::default());
}

#[test]
fn duplicate_conflicts_with_select_next_match() {
    let store = Store::default();
    let conflicts = store.conflicts(Command::Duplicate, &parsed("secondary-d"));
    assert!(conflicts.contains(&Command::SelectNextMatch));
}

#[test]
fn viewport_and_ui_editor_keys_do_not_conflict() {
    let store = Store::default();
    let conflicts = store.conflicts(Command::ToolSelect, &parsed("1"));
    assert!(!conflicts.contains(&Command::UiZoomFit));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn display_uses_plus_separated_title_case_on_linux() {
    assert_eq!(display(&parsed("ctrl-shift-g")), "Ctrl+Shift+G");
    assert_eq!(display(&parsed("f10")), "F10");
    assert_eq!(display(&parsed("up")), "Up");
}

#[cfg(target_os = "macos")]
#[test]
fn display_uses_modifier_glyphs_on_macos() {
    assert_eq!(display(&parsed("cmd-shift-g")), "⇧⌘G");
    assert_eq!(display(&parsed("f10")), "F10");
    assert_eq!(display(&parsed("up")), "Up");
}

#[test]
fn bindable_rejects_bare_modifier_keys() {
    assert!(!bindable(&key("shift", shift())));
    assert!(!bindable(&key("shift", Modifiers::default())));
    assert!(!bindable(&key("control", Modifiers::default())));
}

#[test]
fn bindable_rejects_bare_escape_enter_and_tab() {
    assert!(!bindable(&parsed("escape")));
    assert!(!bindable(&parsed("enter")));
    assert!(!bindable(&parsed("tab")));
}

#[test]
fn bindable_accepts_ordinary_keys() {
    assert!(bindable(&parsed("ctrl-s")));
    assert!(bindable(&parsed("f5")));
    assert!(bindable(&parsed("ctrl-tab")));
}
