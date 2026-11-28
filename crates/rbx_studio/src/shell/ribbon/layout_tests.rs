use std::collections::{HashMap, HashSet};

use super::{groups, Gates, Item, Tab};
use crate::shell::layout::Panel;

const ALL_ON: Gates = Gates {
    update_planning: true,
    pseudo_loc: true,
    local_run: true,
};

/// What Studio 0.748 itself shows on more than one tab: the transform tools
/// and snap (Home, Avatar, Model), Part and the part-property tiles (Home,
/// Model), Character (Home, Avatar), GUI (Home, UI) and Script (Home,
/// Script).
const STUDIO_REPEATS: [Item; 12] = [
    Item::Tools,
    Item::Snap,
    Item::Part,
    Item::Character,
    Item::Gui,
    Item::Script,
    Item::Material,
    Item::Color,
    Item::Group,
    Item::Ungroup,
    Item::Lock,
    Item::Anchor,
];

fn tab_items(tab: Tab, gates: Gates) -> Vec<Item> {
    groups(tab, gates).into_iter().flatten().collect()
}

/// Every command the ribbon carried before it was reorganised, bar the ones
/// Studio keeps in its menus only (Cut/Copy/Paste/Duplicate, Experience
/// Settings, Input Actions, Localization, MicroProfiler, Find in Place),
/// which the menu tests hold to.
fn every_ribbon_command() -> Vec<Item> {
    use Item::*;
    let mut items = vec![
        Tools,
        Snap,
        Part,
        Terrain,
        Character,
        Gui,
        Script,
        Import,
        Material,
        Color,
        Group,
        Ungroup,
        Lock,
        Anchor,
        DataStores,
        UpdatePlanning,
        AvatarSettings,
        Setup,
        Accessory,
        UgcValidation,
        Upload,
        FacePreview,
        ClipEditor,
        StyleEditor,
        PseudoLoc,
        Format,
        Find,
        CommandBar,
        Pivot,
        Effect,
        Align,
        SolidModeling,
        Sun,
        Test,
        ViewportSettings,
        ShowUi,
        Fullbright,
        PluginToolbars,
        PluginTools,
    ];
    items.extend(
        [
            Panel::Explorer,
            Panel::Properties,
            Panel::Output,
            Panel::Viewport,
            Panel::Toolbox,
            Panel::SecurityAudit,
            Panel::PrePublish,
            Panel::Playtests,
            Panel::Layers,
            Panel::Loren,
            Panel::RemoteInspector,
            Panel::LeakFinder,
            Panel::LoadTest,
            Panel::Argon,
            Panel::Wally,
            Panel::ScriptAnalysis,
            Panel::Watch,
            Panel::CallStack,
            Panel::Tests,
            Panel::Bytecode,
        ]
        .map(Dock),
    );
    items
}

#[test]
fn the_tabs_are_studios_in_studios_order() {
    let labels: Vec<&str> = Tab::ALL.iter().map(|tab| tab.label()).collect();
    // Studio 0.748: Home, Avatar, UI, Script, Model, Plugins. Test is this
    // editor's own, with Studio's test controls on it.
    assert_eq!(
        labels,
        ["Home", "Avatar", "UI", "Script", "Model", "Test", "Plugins"]
    );
}

#[test]
fn every_command_is_on_some_tab() {
    let shown: HashSet<Item> = Tab::ALL
        .into_iter()
        .flat_map(|tab| tab_items(tab, ALL_ON))
        .collect();
    for item in every_ribbon_command() {
        assert!(shown.contains(&item), "{item:?} has no tab");
    }
}

#[test]
fn nothing_repeats_within_a_tab() {
    for tab in Tab::ALL {
        let items = tab_items(tab, ALL_ON);
        let unique: HashSet<Item> = items.iter().copied().collect();
        assert_eq!(unique.len(), items.len(), "{tab:?} lists a command twice");
    }
}

#[test]
fn only_studios_own_repeats_span_tabs() {
    let mut tabs_of: HashMap<Item, Vec<Tab>> = HashMap::new();
    for tab in Tab::ALL {
        for item in tab_items(tab, ALL_ON) {
            tabs_of.entry(item).or_default().push(tab);
        }
    }
    for (item, tabs) in tabs_of {
        if tabs.len() > 1 {
            assert!(
                STUDIO_REPEATS.contains(&item),
                "{item:?} is on {tabs:?}, Studio has it once"
            );
        }
    }
}

#[test]
fn studios_tabs_open_on_its_first_buttons() {
    assert_eq!(tab_items(Tab::Home, ALL_ON)[..2], [Item::Tools, Item::Snap]);
    assert_eq!(
        groups(Tab::Home, ALL_ON)[1],
        [
            Item::Part,
            Item::Terrain,
            Item::Character,
            Item::Gui,
            Item::Script,
            Item::Import
        ]
    );
    assert_eq!(groups(Tab::Avatar, ALL_ON)[1], [Item::AvatarSettings]);
    assert_eq!(groups(Tab::Ui, ALL_ON)[0], [Item::Gui]);
    assert_eq!(
        groups(Tab::Script, ALL_ON)[0],
        [Item::Script, Item::Format, Item::Find]
    );
    let model = tab_items(Tab::Model, ALL_ON);
    let at = |item| model.iter().position(|i| *i == item).unwrap();
    // Pivot, Part, Material…Anchor, Align, Union…: Studio's order.
    assert!(at(Item::Pivot) < at(Item::Part));
    assert!(at(Item::Anchor) < at(Item::Align));
    assert!(at(Item::Align) < at(Item::SolidModeling));
}

#[test]
fn beta_commands_stay_hidden_with_their_beta_off() {
    let off = Gates::default();
    let all: Vec<Item> = Tab::ALL
        .into_iter()
        .flat_map(|tab| tab_items(tab, off))
        .collect();
    for hidden in [
        Item::UpdatePlanning,
        Item::PseudoLoc,
        Item::Dock(Panel::RemoteInspector),
        Item::Dock(Panel::LeakFinder),
        Item::Dock(Panel::LoadTest),
    ] {
        assert!(!all.contains(&hidden), "{hidden:?} shows with its beta off");
    }
    assert!(all.contains(&Item::Test) && all.contains(&Item::Dock(Panel::Playtests)));
    let only_planning = Gates {
        update_planning: true,
        ..Gates::default()
    };
    assert!(tab_items(Tab::Home, only_planning).contains(&Item::UpdatePlanning));
    assert!(!tab_items(Tab::Ui, only_planning).contains(&Item::PseudoLoc));
}

#[test]
fn no_group_is_left_empty() {
    for gates in [ALL_ON, Gates::default()] {
        for tab in Tab::ALL {
            assert!(groups(tab, gates).iter().all(|group| !group.is_empty()));
        }
    }
}
