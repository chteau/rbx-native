// Not `super::*`: that brings in `gpui_kit::*`, whose own `test` macro
// would shadow the standard one.
use std::any::TypeId;

use gpui_kit::{Keystroke, SharedString};

use super::commands::{display, filter, registry, wants_actions, Command, Run, HINTS};
use super::focus::{entry, Entry};
use super::instances::instances;
use crate::menu_bar::*;
use crate::shell::chrome::Document;
use crate::shell::ribbon::{
    palette_entries, RibbonCommand, GUI_INSERTS, PART_INSERTS, SUN_BODIES, SUN_MODES,
};
use crate::shell::Panel;
use crate::transform::Tool;

fn all() -> Vec<Command> {
    registry(&menus(&[]), Panel::ALL)
}

fn labels(commands: &[Command], rows: &[usize]) -> Vec<String> {
    rows.iter()
        .map(|&index| commands[index].label.to_string())
        .collect()
}

fn find<'a>(commands: &'a [Command], label: &str) -> &'a Command {
    commands
        .iter()
        .find(|command| command.label == label)
        .unwrap_or_else(|| panic!("no {label} in the palette"))
}

#[test]
fn every_live_menu_item_is_a_row_and_no_placeholder_is() {
    let commands = all();
    let menus = menus(&[]);
    for menu in &menus {
        for item in &menu.items {
            let gpui_kit::OwnedMenuItem::Action {
                name,
                action,
                disabled,
                ..
            } = item
            else {
                continue;
            };
            let any = action.as_any();
            if any.is::<MenuCommandPalette>() || any.is::<MenuQuickOpen>() {
                continue;
            }
            let label = format!("{}: {}", menu.name, name.trim_end_matches('…'));
            let listed = commands.iter().any(|command| command.label == label);
            assert_eq!(listed, !disabled, "{label}");
        }
    }
    assert!(commands.iter().all(|command| match &command.run {
        Run::Action(action) => !action.as_any().is::<MenuPlaceholder>(),
        _ => true,
    }));
}

#[test]
fn tools_and_docks_are_rows_too() {
    let commands = all();
    let move_tool = find(&commands, "Tool: Move");
    assert!(matches!(move_tool.run, Run::Tool(Tool::Move)));
    assert_eq!(move_tool.hint, Some("2"));
    for panel in Panel::ALL {
        let row = find(&commands, &format!("View: Focus {}", panel.key()));
        assert!(matches!(row.run, Run::Focus(listed) if listed == panel));
    }
    // A dock the document cannot show is not offered.
    let without_argon = registry(&menus(&[]), [Panel::Explorer]);
    assert!(without_argon
        .iter()
        .all(|command| command.label != "View: Focus Argon"));
}

#[test]
fn a_users_template_is_listed_under_model() {
    let template = crate::script_templates::Template {
        name: "Service".into(),
        class: "ModuleScript",
        source: String::new(),
    };
    let commands = registry(&menus(&[template]), []);
    find(&commands, "Model: Insert Service (ModuleScript)");
}

#[test]
fn every_hint_is_the_key_its_handler_actually_answers() {
    use crate::shell::{clipboard, group, keys};
    for (type_id, keys_text) in HINTS {
        let key = Keystroke::parse(keys_text).unwrap();
        let (k, m) = (key.key.as_str(), key.modifiers);
        let handled: Option<TypeId> =
            if let Some(crate::save::Action::Save) = crate::save::action_for(k, m) {
                Some(TypeId::of::<MenuSave>())
            } else if let Some(action) = crate::history::action_for(k, m) {
                Some(match action {
                    crate::history::Action::Undo => TypeId::of::<MenuUndo>(),
                    crate::history::Action::Redo => TypeId::of::<MenuRedo>(),
                })
            } else if let Some(action) = clipboard::action_for(k, m) {
                Some(match action {
                    clipboard::Action::Copy => TypeId::of::<MenuCopyInstance>(),
                    clipboard::Action::Cut => TypeId::of::<MenuCutInstance>(),
                    clipboard::Action::Paste => TypeId::of::<MenuPasteInstance>(),
                    clipboard::Action::PasteInto => TypeId::of::<MenuPasteIntoInstance>(),
                    clipboard::Action::Duplicate => TypeId::of::<MenuDuplicateInstance>(),
                })
            } else if let Some(action) = group::action_for(k, m) {
                Some(match action {
                    group::Action::Group => TypeId::of::<MenuGroup>(),
                    group::Action::Ungroup => TypeId::of::<MenuUngroup>(),
                })
            } else if let Some(action) = keys::action_for(k, m) {
                match action {
                    keys::Action::Delete => Some(TypeId::of::<MenuDeleteInstance>()),
                    keys::Action::InsertPart => Some(TypeId::of::<MenuInsertPart>()),
                    keys::Action::InsertFolder => Some(TypeId::of::<MenuInsertFolder>()),
                    keys::Action::Insert | keys::Action::Rename => None,
                }
            } else {
                None
            };
        // Alt+S is a real keymap binding rather than a matcher, held to it
        // by `the_keymap_binds_settings_and_the_palette` below.
        if keys_text == "alt-s" {
            assert_eq!(type_id(), TypeId::of::<MenuStudioSettings>());
            continue;
        }
        assert_eq!(handled, Some(type_id()), "{keys_text}");
    }
}

#[gpui_kit::test]
fn the_keymap_binds_settings_and_the_palette(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        crate::menu_bar::install_key_bindings(cx);
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let answers = |action: &dyn gpui_kit::Action, typed: &str| {
            let typed = [Keystroke::parse(typed).unwrap()];
            keymap
                .bindings_for_action(action)
                .any(|binding| binding.match_keystrokes(&typed) == Some(false))
        };
        assert!(answers(&MenuStudioSettings, "alt-s"));
        // Studio's Quick Open Actions key, written `secondary` so it is
        // Cmd+Alt+P on a Mac.
        assert!(answers(&MenuCommandPalette, "secondary-alt-p"));
        assert!(!answers(&MenuCommandPalette, "secondary-p"));
        // Studio's Quick Open.
        assert!(answers(&MenuQuickOpen, "secondary-p"));
        assert!(!answers(&MenuQuickOpen, "secondary-alt-p"));
        // This editor's quick Insert Part, neither of them.
        assert!(!answers(&MenuCommandPalette, "secondary-shift-p"));
        assert!(!answers(&MenuQuickOpen, "secondary-shift-p"));
    });
}

#[test]
fn hints_read_the_way_menus_write_them() {
    assert_eq!(display("ctrl-shift-g"), "Ctrl+Shift+G");
    assert_eq!(display("ctrl-alt-p"), "Ctrl+Alt+P");
    assert_eq!(display("delete"), "Del");
    assert_eq!(display("2"), "2");
}

#[test]
fn an_empty_query_lists_everything_recent_first() {
    let commands = all();
    let recent: Vec<SharedString> = vec!["Edit: Redo".into(), "File: Save to File".into()];
    let rows = labels(&commands, &filter(&commands, "", &recent));
    assert_eq!(rows.len(), commands.len());
    assert_eq!(rows[..2], ["Edit: Redo", "File: Save to File"]);
    // Then registry order, the recent ones not repeated.
    assert_eq!(rows[2], "File: Close Place");
    assert_eq!(rows[3], "File: Save to File As");
}

#[test]
fn a_query_ranks_prefix_then_word_starts_then_scattered() {
    let commands = all();
    let rows = labels(&commands, &filter(&commands, "save", &[]));
    // Prefix of the name beats a word start later on.
    assert_eq!(rows[0], "File: Save to File");
    assert_eq!(rows[1], "File: Save to File As");
    assert!(rows.contains(&"File: Save to Roblox".to_owned()));

    // Both are word starts; the one that matched on its name alone, not
    // with its category's help (File: EXPort), leads.
    let rows = labels(&commands, &filter(&commands, "fexp", &[]));
    assert_eq!(rows[..2], ["View: Focus Explorer", "File: Export as glTF"]);

    let rows = labels(&commands, &filter(&commands, "zzzz", &[]));
    assert!(rows.is_empty());
}

#[test]
fn recency_breaks_ties_within_a_tier_but_never_beats_a_better_match() {
    let commands = all();
    let recent: Vec<SharedString> = vec!["File: Save to Roblox".into()];
    let rows = labels(&commands, &filter(&commands, "save to", &recent));
    // All three are prefixes of "save to"; the recent one leads them.
    assert_eq!(rows[0], "File: Save to Roblox");
    // Paste holds an "s" only mid-word: however recent, it stays below
    // every name that starts with one.
    let recent: Vec<SharedString> = vec!["Edit: Paste".into()];
    let rows = labels(&commands, &filter(&commands, "s", &recent));
    let paste = rows.iter().position(|row| row == "Edit: Paste").unwrap();
    let select = rows.iter().position(|row| row == "Tool: Select").unwrap();
    assert!(select < paste);
}

#[test]
fn every_live_ribbon_command_is_a_row() {
    let commands = all();
    let ribbon: Vec<&Command> = commands
        .iter()
        .filter(|command| matches!(command.run, Run::Ribbon(_)))
        .collect();
    assert_eq!(ribbon.len(), palette_entries().len());
    // The Part menu's five shapes, the live UI inserts, both sun bodies and
    // all four gestures — read from the same lists the ribbon builds its
    // menus and tiles from.
    let has = |wanted: RibbonCommand| {
        ribbon
            .iter()
            .any(|command| matches!(command.run, Run::Ribbon(run) if run == wanted))
    };
    for item in PART_INSERTS.iter().chain(&GUI_INSERTS) {
        assert!(has(RibbonCommand::Insert(item)), "{}", item.label);
    }
    for (body, _) in SUN_BODIES {
        assert!(has(RibbonCommand::SunBody(body)));
    }
    for mode in SUN_MODES.into_iter().flatten() {
        assert!(has(RibbonCommand::SunMode(mode)));
    }
    let sphere = find(&commands, "Part: Sphere");
    assert!(matches!(
        sphere.run,
        Run::Ribbon(RibbonCommand::Insert(item)) if item.class == "Part" && item.shape.is_some()
    ));
    // AdGui is greyed in the ribbon, so it is no command.
    assert!(commands.iter().all(|command| command.label != "UI: AdGui"));
}

#[test]
fn ribbon_hints_are_the_keys_their_handlers_answer() {
    use crate::transform::{action_for, Action};
    let local = find(&all(), "Tool: Local Orientation").hint.unwrap();
    let key = Keystroke::parse(local).unwrap();
    assert_eq!(
        action_for(&key.key, key.modifiers),
        Some(Action::ToggleLocal)
    );
}

#[test]
fn every_document_has_a_focus_row() {
    let commands = all();
    for (label, document) in [
        ("View: Focus 3D View", Document::Viewport),
        ("View: Focus Script Editor", Document::Scripts),
        ("View: Focus UI Editor", Document::UiEditor),
    ] {
        let row = find(&commands, label);
        assert!(matches!(row.run, Run::Document(listed) if listed == document));
    }
}

/// Every dock with a focusable control gets the caret; the three without
/// one are exactly the ones `focus::entry` says have none.
#[test]
fn focus_moves_into_every_dock_that_can_hold_it() {
    let entries: Vec<(Panel, Option<Entry>)> = Panel::ALL
        .into_iter()
        .map(|panel| (panel, entry(panel)))
        .collect();
    assert_eq!(
        entries,
        [
            (Panel::Explorer, Some(Entry::Tree)),
            (Panel::Properties, Some(Entry::PropertiesFilter)),
            (Panel::Output, Some(Entry::OutputSearch)),
            (Panel::Viewport, Some(Entry::QualitySelect)),
            (Panel::Argon, Some(Entry::ArgonAddress)),
            (Panel::Wally, Some(Entry::WallySearch)),
            (Panel::ScriptAnalysis, None),
            (Panel::Watch, Some(Entry::WatchExpression)),
            (Panel::CallStack, None),
            (Panel::TerrainEditor, None),
        ]
    );
}

#[test]
fn a_leading_angle_bracket_switches_to_commands() {
    assert!(wants_actions(">"));
    assert!(wants_actions("  > save"));
    assert!(!wants_actions(""));
    assert!(!wants_actions("Part"));
    assert!(!wants_actions("a>b"));
}

#[test]
fn quick_open_lists_every_instance_by_name_and_path() {
    let mut dom = rbx_dom::WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Tower", Some(workspace));
    let door = dom.new_instance("Part", "Door", Some(model));
    let storage = dom.new_instance("ServerStorage", "ServerStorage", None);
    let spare = dom.new_instance("Part", "SpareDoor", Some(storage));

    let rows = instances(&dom);
    let paths: Vec<&str> = rows.iter().map(|row| row.label.as_ref()).collect();
    assert_eq!(
        paths,
        [
            "Workspace",
            "Workspace.Tower",
            "Workspace.Tower.Door",
            "ServerStorage",
            "ServerStorage.SpareDoor",
        ]
    );
    assert_eq!(rows[2].text().as_ref(), "Door");
    assert!(matches!(rows[2].run, Run::Instance(reference) if reference == door));

    // A name prefix beats a word start inside another name.
    let found: Vec<&str> = filter(&rows, "door", &[])
        .into_iter()
        .map(|index| rows[index].label.as_ref())
        .collect();
    assert_eq!(found, ["Workspace.Tower.Door", "ServerStorage.SpareDoor"]);
    // A path word finds what a name alone would not, after the name match.
    let found = filter(&rows, "tower", &[]);
    assert_eq!(found[0], 1);
    assert!(found.contains(&2), "Door's path holds Tower");
    // A path counts by its word starts, never by letters scattered along
    // it: "Workspace" holds a "p", which does not make Door a match.
    let found = filter(&rows, "p", &[]);
    assert!(
        !found.contains(&2),
        "Door matched on its path's scattered p"
    );
    // Recent instances lead an empty query, as in Studio.
    let recent: Vec<SharedString> = vec!["ServerStorage.SpareDoor".into()];
    let found = filter(&rows, "", &recent);
    assert!(matches!(rows[found[0]].run, Run::Instance(reference) if reference == spare));
}

/// What a screen reader is told about one option. The active-descendant
/// claim itself is only visible in a live accessibility tree (see
/// `RBX_STUDIO_A11Y_DUMP`), which no test window builds.
#[gpui_kit::test]
fn an_option_tells_a_screen_reader_its_place_state_and_shortcut(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::accesskit::Node;
    use gpui_kit::{Element as _, Role};
    cx.update(|_| {
        let commands = all();
        let undo = find(&commands, "Edit: Undo");
        let mut node = Node::new(Role::ListBoxOption);
        super::view::option(3, 40, undo, true).write_a11y_info(&mut node);
        assert_eq!(node.label(), Some("Edit: Undo"));
        assert_eq!(node.keyboard_shortcut(), Some("Ctrl+Z"));
        assert_eq!(node.is_selected(), Some(true));
        assert_eq!(node.position_in_set(), Some(4));
        assert_eq!(node.size_of_set(), Some(40));

        let mut dom = rbx_dom::WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        dom.new_instance("Part", "Door", Some(workspace));
        let rows = instances(&dom);
        let mut node = Node::new(Role::ListBoxOption);
        super::view::option(0, 1, &rows[1], false).write_a11y_info(&mut node);
        assert_eq!(node.label(), Some("Door"));
        assert_eq!(node.description(), Some("Workspace.Door"));
        assert_eq!(node.is_selected(), Some(false));
    });
}

#[test]
fn studios_action_prefix_is_ignored() {
    let commands = all();
    assert_eq!(
        filter(&commands, "> undo", &[]),
        filter(&commands, "undo", &[])
    );
}
