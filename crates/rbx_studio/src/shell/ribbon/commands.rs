//! The ribbon's live commands as data — what each is called and what it
//! does — read by the ribbon to build its tiles and menus, and by the
//! command palette to list them, so the two cannot drift apart. How a tile
//! looks (its icon, its selected state, why it is greyed) stays in the
//! ribbon; only the name and the effect live here.

use gpui_kit::assets::IconName;
use gpui_kit::Context;
use rbx_viewer::sun::Body;

use crate::sun::Mode;
use crate::transform::{Action, SnapKind, Tool};

use super::super::keys::{PART_TYPE_BALL, PART_TYPE_CYLINDER};
use super::super::layout::Panel;
use super::super::Shell;

/// One insert-menu item.
///
/// `shape` (`Enum.PartType`) is only ever `Some` for the Part menu's Sphere
/// and Cylinder items — they're the only two of the five whose `class`
/// (`Part`) doesn't already say which shape they are. Block shares that same
/// class but needs no override: `Shell::insert_instance`'s own defaults
/// already land on `Enum.PartType.Block`. Wedge/CornerWedge disambiguate
/// through their own class instead and never carry a `Shape` property at
/// all — see `rbx_viewer::scene::shape::resolve`.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::shell) struct Insert {
    pub(in crate::shell) label: &'static str,
    pub(in crate::shell) icon: IconName,
    pub(in crate::shell) class: &'static str,
    pub(in crate::shell) shape: Option<u32>,
}

const fn insert(
    label: &'static str,
    icon: IconName,
    class: &'static str,
    shape: Option<u32>,
) -> Insert {
    Insert {
        label,
        icon,
        class,
        shape,
    }
}

/// Home › Part's menu.
pub(in crate::shell) static PART_INSERTS: [Insert; 5] = [
    insert("Block", IconName::Box, "Part", None),
    insert("Sphere", IconName::Circle, "Part", Some(PART_TYPE_BALL)),
    insert("Wedge", IconName::Triangle, "WedgePart", None),
    insert(
        "Corner Wedge",
        IconName::TriangleRight,
        "CornerWedgePart",
        None,
    ),
    insert(
        "Cylinder",
        IconName::Cylinder,
        "Part",
        Some(PART_TYPE_CYLINDER),
    ),
];

/// Home › Script's three built-in classes.
pub(in crate::shell) static SCRIPT_INSERTS: [Insert; 3] = [
    insert("Script", IconName::FileCode, "Script", None),
    insert("Local Script", IconName::FileCode, "LocalScript", None),
    insert("Module Script", IconName::Package, "ModuleScript", None),
];

/// Home/UI › UI's live inserts. `AdGui` sits between SurfaceGui and
/// BillboardGui in the menu, greyed: it needs a Creator-Dashboard ad unit
/// behind it that this editor cannot provision, so it is no command.
pub(in crate::shell) static GUI_INSERTS: [Insert; 3] = [
    insert("ScreenGui", IconName::AppWindow, "ScreenGui", None),
    insert("SurfaceGui", IconName::Frame, "SurfaceGui", None),
    insert("BillboardGui", IconName::Presentation, "BillboardGui", None),
];

/// Model › Sun's two bodies: which one the Sun tool places.
pub(in crate::shell) const SUN_BODIES: [(Body, &str); 2] =
    [(Body::Sun, "Sun"), (Body::Moon, "Moon")];

/// Model › Sun's four gestures, in the two rows the ribbon stacks them in.
pub(in crate::shell) const SUN_MODES: [[Mode; 2]; 2] =
    [[Mode::Sky, Mode::Face], [Mode::Shadow, Mode::Glint]];

/// One live ribbon command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) enum RibbonCommand {
    Insert(&'static Insert),
    /// The `L` beside the transform tools.
    Local,
    /// The Align popover's Align button, with whatever it is set to.
    Align,
    /// One of the snap popover's two checkboxes.
    Snap(SnapKind),
    EditPivot,
    PivotSnap,
    PivotReset,
    SunBody(Body),
    SunMode(Mode),
    /// Home › Terrain Editor: opens the editor on its last tool, or shuts it.
    TerrainEditor,
    /// Avatar › Rig Builder: opens the dialog that inserts a rig.
    RigBuilder,
}

impl RibbonCommand {
    pub(in crate::shell) fn run(self, shell: &mut Shell, cx: &mut Context<Shell>) {
        match self {
            RibbonCommand::Insert(item) => match item.shape {
                Some(shape) => shell.insert_part(item.class, shape, cx),
                None => shell.insert_instance(item.class, cx),
            },
            RibbonCommand::Local => shell.transform_action(Action::ToggleLocal, cx),
            RibbonCommand::Align => shell.align_selected(cx),
            RibbonCommand::Snap(kind) => shell.transform_action(Action::ToggleSnap(kind), cx),
            RibbonCommand::EditPivot => shell.transform_action(Action::Use(Tool::Pivot), cx),
            RibbonCommand::PivotSnap => shell.transform_action(Action::TogglePivotSnap, cx),
            RibbonCommand::PivotReset => shell.reset_pivot(cx),
            RibbonCommand::SunBody(body) => shell.use_sun(Some(body), None, cx),
            RibbonCommand::SunMode(mode) => shell.use_sun(None, Some(mode), cx),
            RibbonCommand::TerrainEditor => {
                let showing = shell.is_panel_showing(Panel::TerrainEditor);
                shell.toggle_terrain_editor(!showing, cx);
            }
            RibbonCommand::RigBuilder => shell.open_rig_dialog(cx),
        }
    }
}

/// Every live ribbon command as `(category, name, shortcut, command)`, for
/// the palette. Home › Script's built-ins are left out: the Model menu
/// already lists the same three inserts under their own names, and the
/// palette lists that menu.
pub(in crate::shell) fn palette_entries(
) -> Vec<(&'static str, String, Option<&'static str>, RibbonCommand)> {
    let mut entries = Vec::new();
    for item in &PART_INSERTS {
        entries.push((
            "Part",
            item.label.to_owned(),
            None,
            RibbonCommand::Insert(item),
        ));
    }
    for item in &GUI_INSERTS {
        entries.push((
            "UI",
            item.label.to_owned(),
            None,
            RibbonCommand::Insert(item),
        ));
    }
    entries.extend([
        // `transform::action_for`'s own chord, held to it by the palette's
        // hint test.
        (
            "Tool",
            "Local Orientation".to_owned(),
            Some("ctrl-l"),
            RibbonCommand::Local,
        ),
        (
            "Tool",
            "Align Selection".to_owned(),
            None,
            RibbonCommand::Align,
        ),
        (
            "Tool",
            "Toggle Move/Scale Snap".to_owned(),
            None,
            RibbonCommand::Snap(SnapKind::Translate),
        ),
        (
            "Tool",
            "Toggle Rotate Snap".to_owned(),
            None,
            RibbonCommand::Snap(SnapKind::Rotate),
        ),
        (
            "Pivot",
            "Edit Pivot".to_owned(),
            None,
            RibbonCommand::EditPivot,
        ),
        (
            "Pivot",
            "Toggle Snap".to_owned(),
            None,
            RibbonCommand::PivotSnap,
        ),
        ("Pivot", "Reset".to_owned(), None, RibbonCommand::PivotReset),
    ]);
    for (body, label) in SUN_BODIES {
        entries.push((
            "Sun",
            format!("Place {label}"),
            None,
            RibbonCommand::SunBody(body),
        ));
    }
    for mode in SUN_MODES.into_iter().flatten() {
        entries.push((
            "Sun",
            mode.label().to_owned(),
            None,
            RibbonCommand::SunMode(mode),
        ));
    }
    entries.push((
        "Home",
        "Terrain Editor".to_owned(),
        None,
        RibbonCommand::TerrainEditor,
    ));
    entries.push((
        "Avatar",
        "Rig Builder".to_owned(),
        None,
        RibbonCommand::RigBuilder,
    ));
    entries
}
