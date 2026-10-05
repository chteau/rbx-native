use gpui_kit::{Action, OwnedMenuItem};

use super::{
    menus, MenuExportGltf, MenuInsertTemplate, MenuLinkRobloxPlace, MenuOpenAutoSaves,
    MenuPlaceholder, MenuPublishToRoblox, MenuSave, MenuSaveToFile, MenuSaveToRoblox,
    MenuVersionHistory,
};
use crate::script_templates::Template;

/// Every item in the bar either does something or is visibly greyed out.
/// The two halves are one invariant, not two: an enabled item wired to
/// [`MenuPlaceholder`] would look live and do nothing, and a disabled item
/// wired to a real command would hide a feature the editor actually has.
#[test]
fn a_placeholder_item_is_exactly_the_set_of_disabled_items() {
    let placeholder = MenuPlaceholder.name();
    for menu in menus(&[]) {
        for item in menu.items {
            let OwnedMenuItem::Action {
                name,
                action,
                disabled,
                ..
            } = item
            else {
                continue;
            };
            assert_eq!(
                action.name() == placeholder,
                disabled,
                "{} ⟩ {name}",
                menu.name
            );
        }
    }
}

/// The four titles the rest of this module's doc comment describes, in the
/// order Studio itself puts them — a reordering here changes what F10 lands
/// on, so it is worth being deliberate about.
#[test]
fn the_bar_holds_the_four_titles_in_order() {
    let names: Vec<String> = menus(&[])
        .iter()
        .map(|menu| menu.name.to_string())
        .collect();
    assert_eq!(names, ["File", "Edit", "Model", "View"]);
}

/// A menu whose first entry is a separator draws a stray rule at the top,
/// and `PopupMenu::separator` silently drops a leading one, so a menu that
/// starts with one would quietly lose it rather than fail.
#[test]
fn no_menu_opens_or_closes_on_a_separator() {
    for menu in menus(&[]) {
        let separator =
            |item: Option<&OwnedMenuItem>| matches!(item, Some(OwnedMenuItem::Separator));
        assert!(
            !separator(menu.items.first()),
            "{} starts on a rule",
            menu.name
        );
        assert!(
            !separator(menu.items.last()),
            "{} ends on a rule",
            menu.name
        );
    }
}

/// Auto-Recovery's copies are reachable from File, as Studio's are, and
/// the item is live rather than one more placeholder.
#[test]
fn file_opens_the_auto_saves_folder() {
    let file = menus(&[])
        .into_iter()
        .find(|menu| menu.name == "File")
        .expect("a File menu");
    let found = file.items.iter().any(|item| {
        matches!(
            item,
            OwnedMenuItem::Action { name, action, disabled: false, .. }
                if name == "Open Auto Saves" && action.partial_eq(&MenuOpenAutoSaves)
        )
    });
    assert!(found);
}

/// Save and Publish to Roblox are two live, distinct commands — not one
/// placeholder, and not the local Save under another name.
#[test]
fn file_saves_and_publishes_to_roblox_as_two_live_commands() {
    let file = menus(&[])
        .into_iter()
        .find(|menu| menu.name == "File")
        .expect("a File menu");
    let live = |label: &str, wanted: &dyn Action| {
        file.items.iter().any(|item| {
            matches!(
                item,
                OwnedMenuItem::Action { name, action, disabled: false, .. }
                    if name == label && action.partial_eq(wanted)
            )
        })
    };
    assert!(live("Save to Roblox", &MenuSaveToRoblox));
    assert!(live("Publish to Roblox", &MenuPublishToRoblox));
    assert!(live("Link to Roblox Place\u{2026}", &MenuLinkRobloxPlace));
    assert!(live("Version History\u{2026}", &MenuVersionHistory));
    // The whole place to a local file of the user's naming, and as glTF:
    // live, and neither one Ctrl+S's own Save.
    assert!(live("Save to File", &MenuSave));
    assert!(live("Save to File As\u{2026}", &MenuSaveToFile));
    assert!(live("Export as glTF\u{2026}", &MenuExportGltf));
}

/// The templates window opens from File, last, right under Studio
/// Settings.
#[test]
fn file_ends_with_script_templates_under_studio_settings() {
    let file = menus(&[])
        .into_iter()
        .find(|menu| menu.name == "File")
        .expect("a File menu");
    let names: Vec<String> = file
        .items
        .iter()
        .filter_map(|item| match item {
            OwnedMenuItem::Action { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names[names.len() - 2..],
        ["Studio Settings\u{2026}", "Script Templates\u{2026}"]
    );
}

/// The user's templates close Model after a separator (a long list must
/// not push Group/Ungroup off the screen), labelled the way the ribbon's
/// Script menu labels them, each running the template at its own index.
#[test]
fn the_users_templates_close_the_model_menu() {
    let templates = [
        Template {
            class: "Script",
            name: "Enemy AI".into(),
            source: String::new(),
        },
        Template {
            class: "ModuleScript",
            name: "Signal".into(),
            source: String::new(),
        },
    ];
    let model = menus(&templates)
        .into_iter()
        .find(|menu| menu.name == "Model")
        .expect("a Model menu");
    let names: Vec<String> = model
        .items
        .iter()
        .filter_map(|item| match item {
            OwnedMenuItem::Action { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names[names.len() - 3..],
        [
            "Ungroup",
            "Insert Enemy AI (Script)",
            "Insert Signal (ModuleScript)"
        ]
    );
    let n = model.items.len();
    assert!(matches!(model.items[n - 3], OwnedMenuItem::Separator));

    let second = model
        .items
        .iter()
        .find_map(|item| match item {
            OwnedMenuItem::Action { name, action, .. }
                if name == "Insert Signal (ModuleScript)" =>
            {
                Some(action.boxed_clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(second.partial_eq(&MenuInsertTemplate { index: 1 }));
}

/// WCAG 2.5.5: Save to File, Save to Roblox, Publish to Roblox and Delete
/// are drawn as 44px element rows (see `popup::PRIMARY_ROWS`), measured
/// through the toolkit's own popup layout.
#[gpui_kit::test]
fn the_primary_menu_rows_lay_out_at_least_44_tall(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::{point, px, size, AvailableSpace, ParentElement as _};
    let cx = cx.add_empty_window();
    let mut found = Vec::new();
    for menu in menus(&[]) {
        let items = menu.items.clone();
        let entity = cx.update(|window, cx| super::popup::dropdown(&items, None, window, cx));
        cx.draw(
            point(px(0.), px(0.)),
            size(
                AvailableSpace::Definite(px(400.)),
                AvailableSpace::Definite(px(900.)),
            ),
            move |_, _| gpui_kit::div().child(entity),
        );
        for name in super::popup::PRIMARY_ROWS {
            let selector: &'static str = Box::leak(format!("menu-row-{name}").into_boxed_str());
            if let Some(bounds) = cx.debug_bounds(selector) {
                assert!(
                    f32::from(bounds.size.height) >= 44.,
                    "{name} is {:?} tall",
                    bounds.size.height
                );
                found.push(name);
            }
        }
    }
    found.sort_unstable();
    let mut expected = super::popup::PRIMARY_ROWS.to_vec();
    expected.sort_unstable();
    assert_eq!(found, expected, "every primary row exists in some menu");
}
