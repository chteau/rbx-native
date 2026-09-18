//! The ribbon: the row of grouped, labelled icon buttons directly under the
//! Viewport/Script Editor/Style Editor tab strip (see `shell::dock`'s
//! `SectionPanel::render`, which is what actually places it there — this
//! module only builds the element), matching the grouped/captioned layout
//! of a maintainer-supplied ribbon-style Studio redesign reference (see
//! `UX_GUIDELINES.md` §8).
//!
//! Every group mirrors a real Studio Home-ribbon group (Clipboard, Tools,
//! Insert, File, Edit, Test, Viewport Settings) by name and rough button
//! set, but only wires a button to a real action when this editor already
//! has one to call — see `menu_bar`'s own doc comment for the same rule
//! applied to the dropdown menus. A button with no real action yet stays
//! visibly `.disabled(true)` with a tooltip saying so, exactly like
//! `menu_bar`'s Cut/Copy/Paste, rather than a click that silently does
//! nothing.
//!
//! Split across three category [`Tab`]s (Home, Model, Test) rather than one
//! long scrolling row: seven groups' worth of big tiles don't fit an
//! ordinary window width at once — and once the ribbon moved to living
//! inside the (narrower) tab content area instead of spanning the whole
//! window (see `shell::dock`'s doc comment above), even two tabs still
//! needed a scrollbar in an ordinary window. A real Studio ribbon solves
//! this by paging, not by shrinking, so this one pages too: Home carries
//! Clipboard+Tools (the two groups used constantly), Model carries
//! Insert+File+Edit (object-editing, used less often), Test carries
//! Test+Viewport Settings. Real Studio's further tabs (Avatar/UI/Script/
//! Plugins) aren't built — this editor has no distinct content for them
//! yet, and an empty tab page is worse than no tab, not better; add one
//! when a real feature needs it, the same rule `AGENTS.md` asks for
//! everywhere else.
//!
//! Every icon comes from this project's own action icon kit
//! (`action_icons::action_icon`, `assets/icons/actions/` — sibling to the
//! `ClassName`-keyed kit in `class_icons`) or, for the three Insert tiles
//! that literally insert a Roblox class, that class's own icon
//! (`class_tile`). No Lucide glyph is used anywhere in this module — this
//! editor's own icon kit is its visual identity, not a generic one shared
//! with every other app the underlying GUI toolkit renders. Adding a new
//! tile means adding its SVG to `assets/icons/actions/{dark,light}` first
//! (16x16 canvas, flat two-tone fills — see `assets/icons/README.md`'s
//! design system, and the existing files for the shapes it already
//! defines), not reaching for `IconName`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable as _, Selectable as _};
use gpui_kit::*;

use crate::action_icons::action_icon;
use crate::class_icons::{self, IconPack};

use super::Shell;

const TILE_SIZE: f32 = 56.0;
const TILE_ICON_SIZE: f32 = 20.0;

/// The ribbon's own category tabs — see this module's doc comment for why
/// there are only these three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Home,
    Model,
    Test,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Home, Tab::Model, Tab::Test];

    fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Model => "Model",
            Tab::Test => "Test",
        }
    }
}

impl Shell {
    pub(super) fn ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_tab = self.ribbon_tab;
        let pack = self.icon_pack();

        // Built up front, one group at a time: each of these needs `cx` as
        // `&mut Context<Self>` (for `cx.listener(...)`), which can't overlap
        // with `group()`'s own `&App` borrow of the same `cx` inside one
        // expression.
        let page: AnyElement = match active_tab {
            // Clipboard + Tools alone: Tools' own tile row plus the snap
            // fields and Align popover already run wide, so Home stops
            // there rather than also carrying Insert/Edit — three tabs
            // this narrow beats two tabs that still need a scrollbar in an
            // ordinary window.
            Tab::Home => {
                let tools = self.tools_group_content(cx);
                h_flex()
                    .id("ribbon-home")
                    .flex_none()
                    .items_stretch()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .overflow_x_scrollbar()
                    .child(group(cx, "Clipboard", clipboard_tiles(pack)))
                    .child(divider(cx))
                    .child(group(cx, "Tools", tools))
                    .into_any_element()
            }
            Tab::Model => {
                let insert = self.insert_tiles(cx, pack);
                let edit = self.edit_tiles(cx, pack);
                h_flex()
                    .id("ribbon-model")
                    .flex_none()
                    .items_stretch()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .overflow_x_scrollbar()
                    .child(group(cx, "Insert", insert))
                    .child(divider(cx))
                    .child(group(cx, "File", file_tiles(pack)))
                    .child(divider(cx))
                    .child(group(cx, "Edit", edit))
                    .into_any_element()
            }
            Tab::Test => h_flex()
                .id("ribbon-test")
                .flex_none()
                .items_stretch()
                .gap_1()
                .px_2()
                .py_1()
                .overflow_x_scrollbar()
                .child(group(cx, "Test", test_tiles(pack)))
                .child(divider(cx))
                .child(group(
                    cx,
                    "Viewport Settings",
                    viewport_settings_tiles(pack),
                ))
                .into_any_element(),
        };

        v_flex()
            .id("ribbon")
            .w_full()
            .flex_none()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(h_flex().gap_1().px_2().pt_1().children(Tab::ALL.map(|tab| {
                Button::new(("ribbon-tab", tab as usize))
                    .ghost()
                    .label(tab.label())
                    .selected(tab == active_tab)
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.ribbon_tab = tab;
                        cx.notify();
                    }))
            })))
            .child(page)
    }

    /// Part/Script/UI (`ScreenGui`) all reuse `Shell::insert_instance` —
    /// the same generic insert `menu_bar`'s Model menu already drives — so
    /// this group's real buttons are three more entry points into that one
    /// path, not new logic. Toolbox stays disabled: it needs Roblox's
    /// Creator Store catalog, which `ROADMAP.md` already marks impossible
    /// without Roblox's own engine.
    fn insert_tiles(&self, cx: &mut Context<Self>, pack: IconPack) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(
                class_tile("ribbon-insert-part", "Part", pack, "Part")
                    .on_click(cx.listener(|shell, _, _, cx| shell.insert_instance("Part", cx))),
            )
            .child(
                class_tile("ribbon-insert-script", "Script", pack, "Script")
                    .on_click(cx.listener(|shell, _, _, cx| shell.insert_instance("Script", cx))),
            )
            .child(
                class_tile("ribbon-insert-ui", "ScreenGui", pack, "UI").on_click(
                    cx.listener(|shell, _, _, cx| shell.insert_instance("ScreenGui", cx)),
                ),
            )
            .child(disabled_tile(
                "ribbon-insert-toolbox",
                "toolbox",
                pack,
                "Toolbox",
            ))
    }

    /// Group/Ungroup reuse `shell::group`'s existing Ctrl+G/Ctrl+Shift+G
    /// handlers — the same ones `menu_bar`'s Model menu already calls.
    /// Material/Color/Lock/Anchor stay disabled: each is already reachable
    /// through the Properties panel (`Material`, `Color`, `Locked`,
    /// `Anchored`), and a one-click ribbon shortcut for them is real new
    /// plumbing (apply to a whole selection, not just the Properties
    /// panel's single edited instance) this pass doesn't add.
    fn edit_tiles(&self, cx: &mut Context<Self>, pack: IconPack) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(disabled_tile(
                "ribbon-edit-material",
                "material",
                pack,
                "Material",
            ))
            .child(disabled_tile("ribbon-edit-color", "color", pack, "Color"))
            .child(
                tile("ribbon-edit-group", "duplicate", pack, "Group")
                    .on_click(cx.listener(|shell, _, _, cx| shell.group_selected(cx))),
            )
            .child(
                tile("ribbon-edit-ungroup", "copy", pack, "Ungroup")
                    .on_click(cx.listener(|shell, _, _, cx| shell.ungroup_selected(cx))),
            )
            .child(disabled_tile("ribbon-edit-lock", "lock", pack, "Lock"))
            .child(disabled_tile(
                "ribbon-edit-anchor",
                "anchor",
                pack,
                "Anchor",
            ))
    }
}

/// Cut/Copy/Paste match `menu_bar`'s own disabled Edit-menu items exactly —
/// same missing feature, same reason; Duplicate has no menu-bar equivalent
/// yet either.
fn clipboard_tiles(pack: IconPack) -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(disabled_tile("ribbon-copy", "copy", pack, "Copy"))
        .child(disabled_tile("ribbon-paste", "paste", pack, "Paste"))
        .child(disabled_tile("ribbon-cut", "cut", pack, "Cut"))
        .child(disabled_tile(
            "ribbon-duplicate",
            "duplicate",
            pack,
            "Duplicate",
        ))
}

/// Asset import (meshes/models from disk) isn't implemented.
fn file_tiles(pack: IconPack) -> impl IntoElement {
    h_flex().child(disabled_tile("ribbon-import", "import", pack, "Import"))
}

/// None of Play/Run/Resume/Stop/Team/Exit exist yet — `ROADMAP.md`'s own
/// Play/Test workflow section still lists the sandbox-place design this
/// would need as open.
fn test_tiles(pack: IconPack) -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(disabled_tile("ribbon-play", "play", pack, "Play"))
        .child(disabled_tile("ribbon-run", "run", pack, "Run"))
        .child(disabled_tile("ribbon-resume", "resume", pack, "Resume"))
        .child(disabled_tile("ribbon-stop", "stop", pack, "Stop"))
        .child(disabled_tile("ribbon-team", "team", pack, "Team"))
        .child(disabled_tile("ribbon-exit", "exit", pack, "Exit"))
}

/// Game Settings/Device preview/Show UI toggle are all Studio dialogs this
/// editor doesn't have yet. The viewport's own real graphics-quality
/// dropdown stays where it already lived (the Viewport tab's own title row,
/// see `shell::dock`'s `title_suffix`) rather than moving in here — it's
/// this project's own addition, not a Studio ribbon feature, so it doesn't
/// belong under a Studio-named group.
fn viewport_settings_tiles(pack: IconPack) -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(disabled_tile(
            "ribbon-game-settings",
            "game-settings",
            pack,
            "Game Settings",
        ))
        .child(disabled_tile("ribbon-device", "device", pack, "Device"))
        .child(disabled_tile("ribbon-show-ui", "show-ui", pack, "Show UI"))
}

/// A group's whole cluster of tiles/controls, captioned underneath —
/// `content` is usually a row of [`tile`]s, but the Tools group mixes those
/// with the existing snap/Align controls, which is faithful to the
/// reference ribbon's own Tools group (big tool tiles beside a small Mode
/// dropdown and two checkboxes).
fn group(cx: &App, label: &'static str, content: impl IntoElement) -> impl IntoElement {
    v_flex()
        .h_full()
        .flex_none()
        .items_center()
        .justify_between()
        .gap_1()
        .child(h_flex().flex_1().items_center().gap_1().child(content))
        .child(
            div()
                .text_size(px(10.))
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
}

fn divider(cx: &App) -> impl IntoElement {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(TILE_SIZE))
        .bg(cx.theme().border)
}

/// A big square tile: an icon over a caption, the reference ribbon's own
/// button shape — built from a plain [`Button`] with custom `.child(...)`
/// content instead of `.icon()`/`.label()`, since those two lay out side by
/// side rather than stacked (see `gpui_component`'s own `Button` render:
/// `.icon()`/`.label()` join the same horizontal row `.children()`
/// appends to). `action` is a bare filename stem under
/// `assets/icons/actions/{dark,light}` (see `action_icons`), not a Lucide
/// name.
pub(super) fn tile(
    id: impl Into<ElementId>,
    action: &str,
    pack: IconPack,
    label: &'static str,
) -> Button {
    glyph_tile(id, action_icon(action, pack), label)
}

/// [`tile`], but with this project's own class icon (`class_icons::icon_tile`
/// — the same rasterized kit the Explorer's own rows use) for a tile that
/// inserts a specific Roblox class, instead of an action-kit icon.
fn class_tile(
    id: impl Into<ElementId>,
    class: &str,
    pack: IconPack,
    label: &'static str,
) -> Button {
    glyph_tile(id, class_icons::icon_tile(class, pack), label)
}

fn glyph_tile(
    id: impl Into<ElementId>,
    sprite: Option<std::sync::Arc<RenderImage>>,
    label: &'static str,
) -> Button {
    let glyph: AnyElement = match sprite {
        Some(sprite) => img(sprite).size(px(TILE_ICON_SIZE)).into_any_element(),
        // Reached only if an SVG this module names is missing or fails to
        // parse — a build-time invariant `action_icons`'/`class_icons`' own
        // tests already check, so a blank tile here would mean one of those
        // tests should have failed first.
        None => div().size(px(TILE_ICON_SIZE)).into_any_element(),
    };
    Button::new(id)
        .ghost()
        .w(px(TILE_SIZE))
        .h(px(TILE_SIZE))
        .tooltip(label)
        .child(
            v_flex()
                .items_center()
                .justify_center()
                .gap_0p5()
                .child(glyph)
                .child(div().text_size(px(10.)).child(label)),
        )
}

/// A tile for a feature this editor doesn't have yet: disabled, with a
/// tooltip that says so rather than repeating the bare label — the same
/// treatment `menu_bar`'s Cut/Copy/Paste already get.
fn disabled_tile(
    id: impl Into<ElementId>,
    action: &str,
    pack: IconPack,
    label: &'static str,
) -> impl IntoElement {
    tile(id, action, pack, label)
        .disabled(true)
        .tooltip(format!("{label} — not implemented yet"))
}
