//! The ribbon: the row of grouped, labelled icon buttons directly under the
//! menu bar, matching the grouped/captioned layout of a maintainer-supplied
//! ribbon-style Studio redesign reference (see `UX_GUIDELINES.md` §8) rather
//! than the plain text toolbar this editor had before.
//!
//! Every group here mirrors a real Studio Home-ribbon group (Clipboard,
//! Tools, Insert, File, Edit, Test, Viewport Settings) by name and rough
//! button set, but only wires a button to a real action when this editor
//! already has one to call — see `menu_bar`'s own doc comment for the same
//! rule applied to the dropdown menus. A button with no real action yet
//! stays visibly `.disabled(true)` with a tooltip saying so, exactly like
//! `menu_bar`'s Cut/Copy/Paste, rather than a click that silently does
//! nothing. Model/View/Test/Plugins ribbon *tabs* (the reference's own
//! category switcher above its ribbon) are not built — nothing in this
//! editor needs a second page of ribbon content yet, and inventing one
//! before a real feature needs it would be exactly the speculative
//! abstraction `AGENTS.md` asks agents to avoid.

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable as _, Icon, Sizable as _};
use gpui_kit::*;

use crate::class_icons;

use super::Shell;

/// The whole strip's height: enough for a tile's icon+caption plus each
/// group's own caption underneath it (see [`group`]).
const RIBBON_HEIGHT: f32 = 84.0;
const TILE_SIZE: f32 = 56.0;
const TILE_ICON_SIZE: f32 = 20.0;

impl Shell {
    pub(super) fn ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Built up front, one group at a time: each of these needs `cx` as
        // `&mut Context<Self>` (for `cx.listener(...)`), which can't overlap
        // with `group()`'s own `&App` borrow of the same `cx` inside one
        // expression.
        let tools = self.tools_group_content(cx);
        let insert = self.insert_tiles(cx);
        let edit = self.edit_tiles(cx);

        h_flex()
            .id("ribbon")
            .w_full()
            .h(px(RIBBON_HEIGHT))
            .flex_none()
            .items_stretch()
            .gap_1()
            .px_2()
            .py_1()
            .overflow_x_scrollbar()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(group(cx, "Clipboard", clipboard_tiles()))
            .child(divider(cx))
            .child(group(cx, "Tools", tools))
            .child(divider(cx))
            .child(group(cx, "Insert", insert))
            .child(divider(cx))
            .child(group(cx, "File", file_tiles()))
            .child(divider(cx))
            .child(group(cx, "Edit", edit))
            .child(divider(cx))
            .child(group(cx, "Test", test_tiles()))
            .child(divider(cx))
            .child(group(cx, "Viewport Settings", viewport_settings_tiles()))
    }

    /// Part/Script/UI (`ScreenGui`) all reuse `Shell::insert_instance` —
    /// the same generic insert `menu_bar`'s Model menu already drives — so
    /// this group's real buttons are three more entry points into that one
    /// path, not new logic. Toolbox stays disabled: it needs Roblox's
    /// Creator Store catalog, which `ROADMAP.md` already marks impossible
    /// without Roblox's own engine.
    fn insert_tiles(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pack = self.icon_pack();
        h_flex()
            .gap_1()
            .child(
                class_tile("ribbon-insert-part", "Part", pack, IconName::Box, "Part")
                    .on_click(cx.listener(|shell, _, _, cx| shell.insert_instance("Part", cx))),
            )
            .child(
                class_tile(
                    "ribbon-insert-script",
                    "Script",
                    pack,
                    IconName::FileCode,
                    "Script",
                )
                .on_click(cx.listener(|shell, _, _, cx| shell.insert_instance("Script", cx))),
            )
            .child(
                class_tile(
                    "ribbon-insert-ui",
                    "ScreenGui",
                    pack,
                    IconName::AppWindow,
                    "UI",
                )
                .on_click(cx.listener(|shell, _, _, cx| shell.insert_instance("ScreenGui", cx))),
            )
            .child(disabled_tile(
                "ribbon-insert-toolbox",
                IconName::Store,
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
    fn edit_tiles(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(disabled_tile(
                "ribbon-edit-material",
                IconName::SwatchBook,
                "Material",
            ))
            .child(disabled_tile(
                "ribbon-edit-color",
                IconName::Palette,
                "Color",
            ))
            .child(
                tile("ribbon-edit-group", IconName::Boxes, "Group")
                    .on_click(cx.listener(|shell, _, _, cx| shell.group_selected(cx))),
            )
            .child(
                tile("ribbon-edit-ungroup", IconName::PackageOpen, "Ungroup")
                    .on_click(cx.listener(|shell, _, _, cx| shell.ungroup_selected(cx))),
            )
            .child(disabled_tile("ribbon-edit-lock", IconName::Lock, "Lock"))
            .child(disabled_tile(
                "ribbon-edit-anchor",
                IconName::Anchor,
                "Anchor",
            ))
    }
}

/// Cut/Copy/Paste match `menu_bar`'s own disabled Edit-menu items exactly —
/// same missing feature, same reason; Duplicate has no menu-bar equivalent
/// yet either.
fn clipboard_tiles() -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(disabled_tile("ribbon-copy", IconName::Copy, "Copy"))
        .child(disabled_tile(
            "ribbon-paste",
            IconName::ClipboardPaste,
            "Paste",
        ))
        .child(disabled_tile("ribbon-cut", IconName::Scissors, "Cut"))
        .child(disabled_tile(
            "ribbon-duplicate",
            IconName::CopyPlus,
            "Duplicate",
        ))
}

/// Asset import (meshes/models from disk) isn't implemented.
fn file_tiles() -> impl IntoElement {
    h_flex().child(disabled_tile("ribbon-import", IconName::Import, "Import"))
}

/// None of Play/Run/Resume/Stop/Team/Exit exist yet — `ROADMAP.md`'s own
/// Play/Test workflow section still lists the sandbox-place design this
/// would need as open.
fn test_tiles() -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(disabled_tile("ribbon-play", IconName::Play, "Play"))
        .child(disabled_tile("ribbon-run", IconName::MonitorPlay, "Run"))
        .child(disabled_tile("ribbon-resume", IconName::Play, "Resume"))
        .child(disabled_tile("ribbon-stop", IconName::Square, "Stop"))
        .child(disabled_tile("ribbon-team", IconName::Users, "Team"))
        .child(disabled_tile("ribbon-exit", IconName::LogOut, "Exit"))
}

/// Game Settings/Device preview/Show UI toggle are all Studio dialogs this
/// editor doesn't have yet. The viewport's own real graphics-quality
/// dropdown stays where it already lived (the Viewport tab's own title row,
/// see `shell::dock`'s `title_suffix`) rather than moving in here — it's
/// this project's own addition, not a Studio ribbon feature, so it doesn't
/// belong under a Studio-named group.
fn viewport_settings_tiles() -> impl IntoElement {
    h_flex()
        .gap_1()
        .child(disabled_tile(
            "ribbon-game-settings",
            IconName::Settings,
            "Game Settings",
        ))
        .child(disabled_tile(
            "ribbon-device",
            IconName::Smartphone,
            "Device",
        ))
        .child(disabled_tile("ribbon-show-ui", IconName::Eye, "Show UI"))
}

/// A group's whole cluster of tiles/controls, captioned underneath —
/// `content` is usually a row of [`tile`]s, but the Tools group mixes those
/// with the existing snap/Align controls, which is faithful to the
/// reference ribbon's own Tools group (big tool tiles beside a small Mode
/// dropdown and two checkboxes).
pub(super) fn group(cx: &App, label: &'static str, content: impl IntoElement) -> impl IntoElement {
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
/// content instead of `.icon()`/`.label()`, since those two lay out
/// side by side rather than stacked (see `gpui_component`'s own `Button`
/// render: `.icon()`/`.label()` join the same horizontal row `.children()`
/// appends to).
pub(super) fn tile(id: impl Into<ElementId>, icon: IconName, label: &'static str) -> Button {
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
                .child(Icon::new(icon).with_size(px(TILE_ICON_SIZE)))
                .child(div().text_size(px(10.)).child(label)),
        )
}

/// [`tile`], but with this project's own class icon (`class_icons::icon_tile`
/// — the same rasterized kit the Explorer's own rows use) instead of a
/// Lucide glyph, falling back to `fallback` on the rare class the kit
/// doesn't cover — see `explorer::resolve_icon`'s own doc for why that
/// fallback exists at all.
fn class_tile(
    id: impl Into<ElementId>,
    class: &str,
    pack: class_icons::IconPack,
    fallback: IconName,
    label: &'static str,
) -> Button {
    let glyph: AnyElement = match class_icons::icon_tile(class, pack) {
        Some(sprite) => sprite_icon(sprite),
        None => Icon::new(fallback)
            .with_size(px(TILE_ICON_SIZE))
            .into_any_element(),
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

fn sprite_icon(image: Arc<RenderImage>) -> AnyElement {
    img(image).size(px(TILE_ICON_SIZE)).into_any_element()
}

/// A tile for a feature this editor doesn't have yet: disabled, with a
/// tooltip that says so rather than repeating the bare label — the same
/// treatment `menu_bar`'s Cut/Copy/Paste already get.
fn disabled_tile(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
) -> impl IntoElement {
    Button::new(id)
        .ghost()
        .disabled(true)
        .w(px(TILE_SIZE))
        .h(px(TILE_SIZE))
        .tooltip(format!("{label} — not implemented yet"))
        .child(
            v_flex()
                .items_center()
                .justify_center()
                .gap_0p5()
                .child(Icon::new(icon).with_size(px(TILE_ICON_SIZE)))
                .child(div().text_size(px(10.)).child(label)),
        )
}
