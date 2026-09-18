//! **Row C** — the ribbon strip: grouped, captioned commands for whichever
//! category tab Row B has selected (Row B itself lives in `shell::chrome`,
//! since it is a tab row like Row A's, not ribbon content).
//!
//! Two button shapes, and the difference is deliberate:
//!
//! - **Tools** are 32x32 icon buttons packed tight (§2). They are modes you
//!   flip between constantly and already know by shape, so a label under
//!   each one would be noise you read past forever.
//! - **Everything else** is a 56x64 tile: icon over label (§5.2). These are
//!   commands you reach for occasionally, where the word is what you're
//!   actually scanning for.
//!
//! A tile is only clickable when this editor has a real handler behind it.
//! Everything Studio's ribbon offers that this editor doesn't do yet stays
//! visibly disabled with a tooltip saying so, rather than vanishing —
//! seeing the shape of what belongs here is worth more than a shorter
//! ribbon, and it's the same rule `menu_bar`'s greyed items already follow.
//!
//! Every icon comes from [`crate::ui_icons`], never from the toolkit's
//! stock glyph set: the state matrices below re-tint icons per state, which
//! only works on `currentColor` line art.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;
use crate::ui_icons;

use super::menu::{self, MenuId};
use super::Shell;

/// Ribbon height (§4.1 Row C). Enough for a 64px tile plus its group
/// caption underneath, and no more.
const RIBBON_HEIGHT: Pixels = px(88.);
const TILE_WIDTH: Pixels = px(56.);
const TILE_HEIGHT: Pixels = px(64.);
const TILE_ICON: Pixels = px(24.);
/// §2.1: the tool buttons' own, tighter geometry.
pub(super) const TOOL_BUTTON: Pixels = px(32.);
pub(super) const TOOL_ICON: Pixels = px(18.);

/// The ribbon's category tabs. Home carries what you touch constantly,
/// Model what edits objects, Test what runs them — the split exists because
/// all seven groups at once do not fit the centre column at an ordinary
/// window width, and paging is how a real ribbon solves that rather than
/// shrinking everything until it stops being legible.
///
/// Real Studio's further tabs (Avatar/UI/Script/Plugins) aren't here: this
/// editor has no distinct content for them, and an empty page is worse than
/// an absent one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tab {
    #[default]
    Home,
    Model,
    Test,
}

impl Tab {
    pub(super) const ALL: [Tab; 3] = [Tab::Home, Tab::Model, Tab::Test];

    pub(super) fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Model => "Model",
            Tab::Test => "Test",
        }
    }
}

impl Shell {
    pub(super) fn ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let page: Vec<AnyElement> = match self.ribbon_tab {
            Tab::Home => vec![
                group("Clipboard", clipboard_tiles()),
                group("Tools", self.tools(cx)),
            ],
            Tab::Model => vec![
                group("Insert", self.insert_tiles(cx)),
                group("File", file_tiles()),
                group("Edit", self.edit_tiles(cx)),
            ],
            Tab::Test => vec![
                group("Test", test_tiles()),
                group("Viewport Settings", viewport_settings_tiles()),
            ],
        };

        h_flex()
            .w_full()
            .h(RIBBON_HEIGHT)
            .flex_none()
            .items_stretch()
            .gap(tokens::SPACE_4)
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_1)
            .bg(tokens::bg_1())
            .shadow(tokens::elevation_1())
            .children(page)
    }

    /// §2 — the transform tools, their snap popover, and Align.
    fn tools(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut tools = self.tool_buttons(cx);
        tools.push(self.snap_popover(cx).into_any_element());
        tools.push(self.align_control(cx).into_any_element());
        tools
    }

    /// §5.7 — the three insert menus. Every item routes through the same
    /// `Shell::insert_instance` the Model menu already uses, so a ribbon
    /// click and a menu-bar click are the same code path.
    fn insert_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let part = menu::dropdown(
            self,
            MenuId::InsertPart,
            super::chrome::Trigger::new(dropdown_tile("ribbon-insert-part", "part", "Part")),
            vec![
                insert_item("Block", "block", "Part"),
                insert_item("Sphere", "sphere", "Part"),
                insert_item("Wedge", "wedge", "WedgePart"),
                insert_item("Corner Wedge", "corner-wedge", "CornerWedgePart"),
                insert_item("Cylinder", "cylinder", "Part"),
            ],
            cx,
        );
        let script = menu::dropdown(
            self,
            MenuId::InsertScript,
            super::chrome::Trigger::new(dropdown_tile("ribbon-insert-script", "script", "Script")),
            vec![
                insert_item("Script", "script", "Script"),
                insert_item("Local Script", "local-script", "LocalScript"),
                insert_item("Module Script", "module-script", "ModuleScript"),
            ],
            cx,
        );
        let gui = menu::dropdown(
            self,
            MenuId::InsertGui,
            super::chrome::Trigger::new(dropdown_tile("ribbon-insert-gui", "gui", "UI")),
            vec![
                insert_item("ScreenGui", "screen-gui", "ScreenGui"),
                insert_item("SurfaceGui", "surface-gui", "SurfaceGui"),
                // Roblox's own ad surface: it needs a Creator-Dashboard ad
                // unit behind it to mean anything, which this editor has no
                // way to provision.
                menu::item("AdGui").icon("ad-gui").disabled(),
                insert_item("BillboardGui", "billboard-gui", "BillboardGui"),
            ],
            cx,
        );

        vec![
            part.into_any_element(),
            script.into_any_element(),
            gui.into_any_element(),
            // Roblox's Creator Store catalog — out of reach without Roblox's
            // own engine (see `ROADMAP.md`).
            disabled_tile("ribbon-toolbox", "toolbox", "Toolbox").into_any_element(),
        ]
    }

    /// Group/Ungroup are live (`shell::group`, the same Ctrl+G/Ctrl+Shift+G
    /// handlers the Model menu calls). Material/Colour/Lock/Anchor are all
    /// reachable through the Properties panel today; a one-click ribbon
    /// version has to apply across a whole selection, which is real new
    /// plumbing rather than a second button on an existing path.
    fn edit_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![
            disabled_tile("ribbon-material", "material", "Material").into_any_element(),
            disabled_tile("ribbon-color", "color", "Color").into_any_element(),
            tile("ribbon-group", "group", "Group")
                .on_click(cx.listener(|shell, _, _, cx| shell.group_selected(cx)))
                .into_any_element(),
            tile("ribbon-ungroup", "ungroup", "Ungroup")
                .on_click(cx.listener(|shell, _, _, cx| shell.ungroup_selected(cx)))
                .into_any_element(),
            disabled_tile("ribbon-lock", "lock", "Lock").into_any_element(),
            disabled_tile("ribbon-anchor", "anchor", "Anchor").into_any_element(),
        ]
    }
}

/// Cut/Copy/Paste match `menu_bar`'s own disabled Edit-menu items exactly —
/// same missing feature, same reason. Duplicate has no menu-bar twin yet.
fn clipboard_tiles() -> Vec<AnyElement> {
    [
        ("copy", "Copy"),
        ("paste", "Paste"),
        ("cut", "Cut"),
        ("duplicate", "Duplicate"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (icon, label))| {
        disabled_tile(("ribbon-clipboard", index), icon, label).into_any_element()
    })
    .collect()
}

/// Importing meshes and models off disk isn't implemented.
fn file_tiles() -> Vec<AnyElement> {
    vec![disabled_tile("ribbon-import", "import", "Import").into_any_element()]
}

/// `ROADMAP.md`'s Play/Test section still lists the sandbox-place design
/// all of these would need as open.
fn test_tiles() -> Vec<AnyElement> {
    [
        ("play", "Play"),
        ("run", "Run"),
        ("resume", "Resume"),
        ("stop", "Stop"),
        ("team", "Team"),
        ("exit", "Exit"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (icon, label))| {
        disabled_tile(("ribbon-test", index), icon, label).into_any_element()
    })
    .collect()
}

/// The viewport's own real control — the graphics-quality dropdown — is not
/// here: it lives beside the document tabs (Row A), because it belongs to
/// the open document rather than to a Studio-named ribbon group.
fn viewport_settings_tiles() -> Vec<AnyElement> {
    [
        ("game-settings", "Game Settings"),
        ("device", "Device"),
        ("show-ui", "Show UI"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (icon, label))| {
        disabled_tile(("ribbon-viewport", index), icon, label).into_any_element()
    })
    .collect()
}

fn insert_item(label: &'static str, icon: &'static str, class: &'static str) -> menu::Item {
    menu::item(label)
        .icon(icon)
        .on_click(move |shell, cx| shell.insert_instance(class, cx))
}

/// A group: its controls, then its caption underneath.
fn group(label: &'static str, content: Vec<AnyElement>) -> AnyElement {
    v_flex()
        .flex_none()
        .h_full()
        .items_center()
        .justify_between()
        .child(
            h_flex()
                .flex_1()
                .items_center()
                .gap(tokens::SPACE_1)
                .children(content),
        )
        .child(
            div()
                .text_size(tokens::SECTION_HEADER_SIZE)
                .line_height(tokens::SECTION_HEADER_LINE_HEIGHT)
                .font_weight(tokens::SECTION_HEADER_WEIGHT)
                .text_color(tokens::text_secondary())
                .child(label.to_uppercase()),
        )
        .into_any_element()
}

/// §5.2's tile: icon over label, one hit target.
pub(super) fn tile(id: impl Into<ElementId>, icon: &str, label: &'static str) -> Stateful<Div> {
    base_tile(id, icon, label, true)
}

/// The same tile for something this editor can't do yet: greyed, not
/// clickable, and honest about why on hover.
pub(super) fn disabled_tile(
    id: impl Into<ElementId>,
    icon: &str,
    label: &'static str,
) -> Stateful<Div> {
    base_tile(id, icon, label, false)
}

/// A tile that opens a menu: §5.2's chevron sits at the icon zone's
/// bottom-right, and the whole tile is one hit target — no split click
/// zones, so there's no way to miss the arrow and get the wrong action.
fn dropdown_tile(id: impl Into<ElementId>, icon: &str, label: &'static str) -> Stateful<Div> {
    base_tile(id, icon, label, true).child(
        div()
            .absolute()
            .right(px(2.))
            .top(px(22.))
            .child(ui_icons::icon("chevron-down").size(px(10.))),
    )
}

fn base_tile(
    id: impl Into<ElementId>,
    icon: &str,
    label: &'static str,
    enabled: bool,
) -> Stateful<Div> {
    v_flex()
        .id(id.into())
        .relative()
        .flex_none()
        // §7.2: the ribbon's own controls follow the two tab rows, left to
        // right, sharing one band so the order survives a page switch.
        .when(enabled, |this| {
            this.tab_index(super::chrome::RIBBON_CONTROL_INDEX)
                .focus(|this| this.shadow(tokens::focus_ring(tokens::bg_1())))
        })
        .w(TILE_WIDTH)
        .h(TILE_HEIGHT)
        .pt(tokens::SPACE_2)
        .gap(tokens::SPACE_1)
        .items_center()
        .rounded(tokens::RADIUS_SM)
        .text_size(tokens::UI_LABEL_SIZE)
        .line_height(tokens::UI_LABEL_LINE_HEIGHT)
        .map(|this| {
            if enabled {
                this.cursor_pointer()
                    .text_color(tokens::text_secondary())
                    .hover(|this| this.bg(tokens::bg_2()).text_color(tokens::text_primary()))
                    .active(|this| this.bg(tokens::bg_3()))
            } else {
                this.cursor_not_allowed()
                    .text_color(tokens::text_disabled())
            }
        })
        .child(ui_icons::icon(icon).size(TILE_ICON))
        .child(div().truncate().child(label))
}
