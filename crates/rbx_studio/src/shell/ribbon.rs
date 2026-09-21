//! **Row C** — the ribbon: grouped commands for whichever category tab Row
//! B has selected (Row B itself lives in `shell::chrome`, since it is a tab
//! strip like Row A's, not ribbon content).
//!
//! Two button shapes, both the frame's:
//!
//! - a **tile** — 42px wide, icon over label, filling the ribbon's height —
//!   for the commands worth aiming at;
//! - a **stack** — 78px wide, three rows of icon-beside-label — for the
//!   ones that belong to the tile beside them (Copy's paste/cut/duplicate)
//!   or that are really a readout (the snap increments).
//!
//! A tile is only clickable when this editor has a real handler behind it.
//! Everything Studio's ribbon offers that this editor doesn't do yet stays
//! visibly disabled with a tooltip saying so, rather than vanishing —
//! seeing the shape of what belongs here is worth more than a shorter
//! ribbon, and it's the same rule `menu_bar`'s greyed items already follow.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::keys::{PART_TYPE_BALL, PART_TYPE_CYLINDER};
use super::layout::Panel;
use super::roving::Roving;

use crate::script_templates::ScriptTemplates;
use crate::tokens;
use crate::transform::Tool;

use super::menu::{self, MenuId};
use super::Shell;

/// The ribbon's category tabs. The frame names six; this editor has real
/// commands for Home, UI and Model, a page of its own for running a place
/// (Test, which the frame has no tab for), and placeholders for the rest —
/// an empty-looking page is how a ribbon says "this exists, not yet", and
/// it is the same answer the menu bar's greyed items give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tab {
    #[default]
    Home,
    Avatar,
    Ui,
    Script,
    Model,
    Test,
    Plugins,
}

impl Tab {
    pub(super) const ALL: [Tab; 7] = [
        Tab::Home,
        Tab::Avatar,
        Tab::Ui,
        Tab::Script,
        Tab::Model,
        Tab::Test,
        Tab::Plugins,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Avatar => "Avatar",
            Tab::Ui => "UI",
            Tab::Script => "Script",
            Tab::Model => "Model",
            Tab::Test => "Test",
            Tab::Plugins => "Plugins",
        }
    }
}

impl Shell {
    pub(super) fn ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The ribbon is one Tab stop whose control count depends on the
        // open page, so the group is opened here and closed once the page
        // has actually been built (see `shell::roving`).
        self.ribbon_nav.begin(&self.tab_order, None, cx);
        let groups: Vec<Vec<AnyElement>> = match self.ribbon_tab {
            Tab::Home => vec![
                self.clipboard_tiles(cx),
                self.transform_tools(cx),
                self.insert_tiles(cx),
                self.panel_tiles(cx),
            ],
            Tab::Avatar => vec![placeholders(
                "avatar",
                &[
                    (IconName::Users, "Rig Builder"),
                    (IconName::Play, "Animation"),
                    (IconName::Package, "Accessory"),
                    (IconName::Settings, "Avatar Setup"),
                ],
            )],
            Tab::Ui => vec![
                vec![self.insert_gui(cx).into_any_element()],
                placeholders(
                    "ui",
                    &[
                        (IconName::Anchor, "Anchor"),
                        (IconName::AlignHorizontalJustifyCenter, "Align"),
                        (IconName::Scaling, "Auto Scale"),
                    ],
                ),
            ],
            Tab::Script => vec![placeholders(
                "script",
                &[
                    (IconName::Search, "Find"),
                    (IconName::Type, "Replace"),
                    (IconName::Code, "Format"),
                    (IconName::Bug, "Analysis"),
                ],
            )],
            Tab::Model => vec![self.edit_tiles(cx), file_tiles()],
            Tab::Test => vec![test_tiles(), viewport_tiles()],
            Tab::Plugins => vec![placeholders(
                "plugins",
                &[
                    (IconName::Wrench, "Manage"),
                    (IconName::Package, "Folder"),
                    (IconName::FilePlus, "Create"),
                ],
            )],
        };

        // One separator between groups, none at either end — the frame's own
        // rhythm, and the reason the groups are built as a list of lists
        // rather than one flat row.
        let mut children: Vec<AnyElement> = Vec::new();
        for group in groups.into_iter().filter(|group| !group.is_empty()) {
            if !children.is_empty() {
                children.push(separator());
            }
            children.push(
                h_flex()
                    .flex_none()
                    .h_full()
                    .items_stretch()
                    .gap(px(5.))
                    .children(group)
                    .into_any_element(),
            );
        }

        self.ribbon_nav.finish();

        h_flex()
            .id("ribbon")
            .w_full()
            .h(tokens::ribbon_height())
            .flex_none()
            .items_stretch()
            // Scrolls rather than clips. At 2x the UI scale the Home page
            // is wider than the window, and a ribbon that simply loses its
            // last buttons is WCAG 1.4.4's "loss of content" — the precise
            // thing the scale exists to avoid.
            .overflow_x_scroll()
            .p(px(5.))
            .gap(px(10.))
            .bg(tokens::chrome())
            // Arrow keys move between the ribbon's buttons rather than
            // leaving it; Enter and Space are left alone, because GPUI
            // already turns those into a click on whichever button holds
            // focus.
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.ribbon_nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .children(children)
    }

    /// The four transform tools, the local-axis toggle, Align, and the snap
    /// increments they all obey.
    fn transform_tools(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let active = self.transform.tool;
        let mut group: Vec<AnyElement> = Tool::ALL
            .map(|tool| {
                tile(
                    &self.ribbon_nav,
                    ("tool", tool as usize),
                    tool_icon(tool),
                    tool.label(),
                    cx,
                )
                .when(active == tool, |this| selected(this, tool_accent(tool)))
                .tooltip(move |window, cx| {
                    super::tooltip::text(
                        format!("{} ({})", tool.label(), tool.shortcut()),
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.transform_action(crate::transform::Action::Use(tool), cx);
                }))
                .into_any_element()
            })
            .into_iter()
            .collect();

        group.push(self.local_tile(cx).into_any_element());
        group.push(self.align_control(cx).into_any_element());
        group.push(self.snap_stack(cx).into_any_element());
        group
    }

    /// §5.7 — the three insert menus. Every item routes through
    /// `Shell::insert_instance` (or, for the Part menu's Block/Sphere/
    /// Cylinder, `Shell::insert_part` — see `insert_item`), the same paths
    /// the Model menu already uses, so a ribbon click and a menu-bar click
    /// are the same code path.
    fn insert_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let part = menu::dropdown(
            self,
            MenuId::InsertPart,
            super::chrome::Trigger::new(tile(
                &self.ribbon_nav,
                "ribbon-insert-part",
                IconName::Box,
                "Part",
                cx,
            )),
            vec![
                insert_item("Block", IconName::Box, "Part", None),
                insert_item("Sphere", IconName::Circle, "Part", Some(PART_TYPE_BALL)),
                insert_item("Wedge", IconName::Triangle, "WedgePart", None),
                insert_item(
                    "Corner Wedge",
                    IconName::TriangleRight,
                    "CornerWedgePart",
                    None,
                ),
                insert_item(
                    "Cylinder",
                    IconName::Cylinder,
                    "Part",
                    Some(PART_TYPE_CYLINDER),
                ),
            ],
            cx,
        );
        let script = menu::dropdown(
            self,
            MenuId::InsertScript,
            super::chrome::Trigger::new(tile(
                &self.ribbon_nav,
                "ribbon-insert-script",
                IconName::FileCode,
                "Script",
                cx,
            )),
            script_items(&self.script_templates),
            cx,
        );

        vec![
            part.into_any_element(),
            script.into_any_element(),
            self.insert_gui(cx).into_any_element(),
            // Roblox's Creator Store catalog — out of reach without Roblox's
            // own engine (see `ROADMAP.md`).
            disabled_tile("ribbon-toolbox", IconName::Package, "Toolbox").into_any_element(),
        ]
    }

    /// One toggle per dock, and the reason they exist: a dock closed from
    /// its own tab has no tab left to bring it back with. The View menu
    /// carries the same three, through the same call — a window you can
    /// shut and not reopen is a window you have lost.
    fn panel_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        [
            (Panel::Explorer, IconName::ListTree, "Explorer"),
            (Panel::Properties, IconName::SlidersHorizontal, "Properties"),
            (Panel::Output, IconName::Terminal, "Output"),
        ]
        .into_iter()
        .map(|(panel, icon, label)| {
            let open = self.is_panel_open(panel);
            tile(&self.ribbon_nav, label, icon, label, cx)
                .when(open, |this| this.bg(tokens::ribbon_tab_active()))
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.set_panel_open(panel, !open, cx);
                }))
                .into_any_element()
        })
        .collect()
    }

    fn insert_gui(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        menu::dropdown(
            self,
            MenuId::InsertGui,
            super::chrome::Trigger::new(tile(
                &self.ribbon_nav,
                "ribbon-insert-gui",
                IconName::AppWindow,
                "UI",
                cx,
            )),
            vec![
                insert_item("ScreenGui", IconName::AppWindow, "ScreenGui", None),
                insert_item("SurfaceGui", IconName::Frame, "SurfaceGui", None),
                // Roblox's own ad surface: it needs a Creator-Dashboard ad
                // unit behind it to mean anything, which this editor has no
                // way to provision.
                menu::item("AdGui").icon(IconName::Megaphone).disabled(),
                insert_item("BillboardGui", IconName::Presentation, "BillboardGui", None),
            ],
            cx,
        )
    }

    /// Group/Ungroup are live (`shell::group`, the same Ctrl+G/Ctrl+Shift+G
    /// handlers the Model menu calls). Material/Colour/Lock/Anchor are all
    /// reachable through the Properties panel today; a one-click ribbon
    /// version has to apply across a whole selection, which is real new
    /// plumbing rather than a second button on an existing path.
    /// Cut/Copy/Paste/Duplicate run the same `shell::clipboard` entry points
    /// as `menu_bar`'s Edit menu and `Ctrl+X`/`C`/`V`/`D`.
    ///
    /// Each is greyed exactly while its own handler would return early
    /// without doing anything — `clipboard::has_copyable` is that handler's
    /// own guard, so the button and the command cannot disagree about when
    /// there is something to act on. A control that is greyed is also out
    /// of the roving group (see `disabled_tile`), so arrowing along the
    /// ribbon skips it rather than stopping on a dead end; the ribbon
    /// already rebuilds that group every render, which is why its count is
    /// allowed to change with the clipboard.
    fn clipboard_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        const NOTHING_SELECTED: &str = "nothing is selected";
        const CLIPBOARD_EMPTY: &str = "the clipboard is empty";

        let nav = &self.ribbon_nav;
        let copyable =
            super::clipboard::has_copyable(&self.dom, &self.database, self.selected_all());

        let copy = if copyable {
            tile(nav, "ribbon-copy", IconName::Copy, "Copy", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.copy_selected(cx)))
        } else {
            unavailable_tile("ribbon-copy", IconName::Copy, "Copy", NOTHING_SELECTED)
        };
        let paste = if self.clipboard.is_empty() {
            unavailable_row(
                "ribbon-paste",
                IconName::ClipboardPaste,
                "Paste",
                CLIPBOARD_EMPTY,
            )
        } else {
            live_stack_row(nav, "ribbon-paste", IconName::ClipboardPaste, "Paste", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.paste_clipboard(cx)))
        };
        let cut = if copyable {
            live_stack_row(nav, "ribbon-cut", IconName::Scissors, "Cut", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.cut_selected(cx)))
        } else {
            unavailable_row("ribbon-cut", IconName::Scissors, "Cut", NOTHING_SELECTED)
        };
        let duplicate = if copyable {
            live_stack_row(nav, "ribbon-duplicate", IconName::CopyPlus, "Duplicate", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.duplicate_selected(cx)))
        } else {
            unavailable_row(
                "ribbon-duplicate",
                IconName::CopyPlus,
                "Duplicate",
                NOTHING_SELECTED,
            )
        };

        vec![
            copy.into_any_element(),
            stack(vec![paste, cut, duplicate]).into_any_element(),
        ]
    }

    /// Group and Ungroup grey on the same rule the clipboard tiles follow
    /// (see [`Shell::clipboard_tiles`]): `group::has_groupable` and
    /// `has_ungroupable` are the guards `group_selected` and
    /// `ungroup_selected` return early on, so a live tile always has work to
    /// do. Group needs one common, reparentable parent; Ungroup needs one
    /// `Model` among the selection and ignores whatever else is there.
    fn edit_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let nav = &self.ribbon_nav;
        let selected = self.selected_all();
        let group = if super::group::has_groupable(&self.dom, &self.database, selected) {
            tile(nav, "ribbon-group", IconName::Group, "Group", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.group_selected(cx)))
        } else {
            unavailable_tile(
                "ribbon-group",
                IconName::Group,
                "Group",
                "nothing groupable is selected",
            )
        };
        let ungroup = if super::group::has_ungroupable(&self.dom, &self.database, selected) {
            tile(nav, "ribbon-ungroup", IconName::Ungroup, "Ungroup", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.ungroup_selected(cx)))
        } else {
            unavailable_tile(
                "ribbon-ungroup",
                IconName::Ungroup,
                "Ungroup",
                "no Model is selected",
            )
        };

        vec![
            group.into_any_element(),
            ungroup.into_any_element(),
            disabled_tile("ribbon-material", IconName::Layers, "Material").into_any_element(),
            disabled_tile("ribbon-color", IconName::Droplet, "Color").into_any_element(),
            disabled_tile("ribbon-lock", IconName::Lock, "Lock").into_any_element(),
            disabled_tile("ribbon-anchor", IconName::Anchor, "Anchor").into_any_element(),
        ]
    }
}

/// Importing meshes and models off disk isn't implemented.
fn file_tiles() -> Vec<AnyElement> {
    vec![disabled_tile("ribbon-import", IconName::Import, "Import").into_any_element()]
}

/// `ROADMAP.md`'s Play/Test section still lists the sandbox-place design
/// all of these would need as open.
fn test_tiles() -> Vec<AnyElement> {
    vec![
        disabled_tile("ribbon-play", IconName::Play, "Play").into_any_element(),
        stack(vec![
            stack_row("ribbon-run", IconName::SquarePlay, "Run"),
            stack_row("ribbon-pause", IconName::Pause, "Pause"),
            stack_row("ribbon-stop", IconName::Square, "Stop"),
        ])
        .into_any_element(),
        disabled_tile("ribbon-team", IconName::Users, "Team Test").into_any_element(),
    ]
}

/// The viewport's own real controls — quality, orthographic, the axis
/// indicator — are not here: they live in the viewport dock's own settings
/// menu, because they belong to the open document rather than to a
/// Studio-named ribbon group.
fn viewport_tiles() -> Vec<AnyElement> {
    placeholders(
        "viewport",
        &[
            (IconName::Settings, "Settings"),
            (IconName::Smartphone, "Device"),
            (IconName::Eye, "Show UI"),
        ],
    )
}

/// A whole group this editor has nothing behind yet.
fn placeholders(page: &'static str, tiles: &[(IconName, &'static str)]) -> Vec<AnyElement> {
    tiles
        .iter()
        .enumerate()
        .map(|(index, &(icon, label))| disabled_tile((page, index), icon, label).into_any_element())
        .collect()
}

/// `shape` (`Enum.PartType`) is only ever `Some` for the Part menu's Sphere
/// and Cylinder items — they're the only two of the five whose `class`
/// (`Part`) doesn't already say which shape they are. Block shares that same
/// class but needs no override: `Shell::insert_instance`'s own defaults
/// already land on `Enum.PartType.Block`. Wedge/CornerWedge disambiguate
/// through their own class instead and never carry a `Shape` property at
/// all — see `rbx_viewer::scene::shape::resolve`.
fn insert_item(
    label: &'static str,
    icon: IconName,
    class: &'static str,
    shape: Option<u32>,
) -> menu::Item {
    menu::item(label)
        .icon(icon)
        .on_click(move |shell, cx| match shape {
            Some(shape) => shell.insert_part(class, shape, cx),
            None => shell.insert_instance(class, cx),
        })
}

/// The three built-in script classes, then whatever the user has put in
/// their templates directory, labelled `"<name> (<class>)"` so two templates
/// of different classes can share a name without being ambiguous.
fn script_items(templates: &ScriptTemplates) -> Vec<menu::Item> {
    let mut items = vec![
        insert_item("Script", IconName::FileCode, "Script", None),
        insert_item("Local Script", IconName::FileCode, "LocalScript", None),
        insert_item("Module Script", IconName::Package, "ModuleScript", None),
    ];
    items.extend(
        templates
            .extras()
            .iter()
            .enumerate()
            .map(|(index, template)| {
                let icon = if template.class == "ModuleScript" {
                    IconName::Package
                } else {
                    IconName::FileCode
                };
                menu::item(format!("{} ({})", template.name, template.class))
                    .icon(icon)
                    .on_click(move |shell, cx| shell.insert_user_template(index, cx))
            }),
    );
    items
}

fn tool_icon(tool: Tool) -> IconName {
    match tool {
        Tool::Select => IconName::MousePointer2,
        Tool::Move => IconName::Move3d,
        Tool::Scale => IconName::Scale3d,
        Tool::Rotate => IconName::Rotate3d,
    }
}

/// Each transform tool's own pastel, spent only on that tool's button while
/// it is the active one.
pub(super) fn tool_accent(tool: Tool) -> Rgba {
    match tool {
        Tool::Select => tokens::tool_select(),
        Tool::Move => tokens::tool_move(),
        Tool::Scale => tokens::tool_scale(),
        Tool::Rotate => tokens::tool_rotate(),
    }
}

/// Marks a tool button as the active one.
///
/// Three cues, not one: a wash of the tool's pastel, a 1.5px border in the
/// same pastel, and the icon itself tinted. WCAG 1.4.1 forbids colour as
/// the *only* means of conveying state, and the border is the part that
/// survives grayscale and every kind of colour blindness — the tools
/// already differ by icon shape, so between the two an active tool is
/// identifiable with no hue perception at all.
pub(super) fn selected(tile: Stateful<Div>, accent: Rgba) -> Stateful<Div> {
    tile.bg(tokens::tool_wash(accent))
        .border(tokens::tool_border())
        .border_color(accent)
        .text_color(accent)
}

/// The frame's tile: icon over label, one hit target, full ribbon height.
pub(super) fn tile(
    nav: &Roving,
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    cx: &mut App,
) -> Stateful<Div> {
    nav.claim(base_tile(id, icon, label, true), cx)
}

/// The same tile for something this editor can't do yet: greyed, not
/// clickable, and honest about why on hover.
/// A disabled control is deliberately **not** claimed into the roving
/// group: the APG's toolbar pattern focuses the first non-disabled control
/// on entry and arrows skip the rest, and a keyboard user landing on a
/// button that cannot do anything is a dead end with no way to tell why.
/// The tooltip still explains it on hover.
pub(super) fn disabled_tile(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
) -> Stateful<Div> {
    unavailable_tile(id, icon, label, "not implemented")
}

/// The same greyed, unclaimed tile for a command this editor *does* have
/// that cannot act right now — nothing selected, an empty clipboard.
///
/// The reason replaces "not implemented" because the two are different
/// answers: one is a gap in this editor, the other is something the person
/// at the keyboard can fix in a second. Offering the click instead and
/// doing nothing is the worst of the three, which is what this exists to
/// stop.
pub(super) fn unavailable_tile(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    reason: &'static str,
) -> Stateful<Div> {
    base_tile(id, icon, label, false)
        .tooltip(move |window, cx| super::tooltip::text(format!("{label} — {reason}"), window, cx))
}

fn base_tile(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    enabled: bool,
) -> Stateful<Div> {
    v_flex()
        .id(id.into())
        .relative()
        .flex_none()
        .w(tokens::tile_width())
        .h_full()
        .gap(px(6.))
        .px(px(1.))
        .py(px(5.))
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS)
        .bg(tokens::tile())
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .map(|this| {
            if enabled {
                // No `tab_index` here: the ribbon is one Tab stop and
                // `shell::roving` decides which of its controls currently
                // holds it.
                this.focus_visible(|this| this.shadow(tokens::focus_ring(tokens::chrome())))
                    .cursor_pointer()
                    .text_color(tokens::text_label())
                    .hover(|this| this.bg(tokens::hover()).text_color(tokens::text_full()))
                    .active(|this| this.bg(tokens::ribbon_tab_active()))
            } else {
                this.cursor_not_allowed()
                    .text_color(tokens::text_disabled())
            }
        })
        .group("ribbon-tile")
        .child(icon_zone(icon, px(20.)))
        .child(div().w_full().text_center().truncate().child(label))
}

/// A ribbon button's icon, and its hover lift.
///
/// The spec asks for a 120ms 1.06x scale. GPUI has no property transitions
/// and no element transform, so the lift is instant and is a size change on
/// the icon's own fixed box rather than a transform — the label below it
/// doesn't move, because the box the icon fills is what grows. Being
/// instant, it is not motion in WCAG 2.3.3's sense at all; it still checks
/// `reduced_motion`, because a jump is a change and costs one line to
/// honour, and the background hover carries the whole affordance without it.
fn icon_zone(icon: IconName, size: Pixels) -> Div {
    let lifted = px(f32::from(size) * 1.06);

    div()
        .flex_none()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .when(!tokens::reduced_motion(), |this| {
            this.group_hover("ribbon-tile", move |this| this.size(lifted))
        })
        .child(Icon::new(icon).size_full())
}

/// The column of narrow rows that sits beside a tile.
pub(super) fn stack(rows: Vec<Stateful<Div>>) -> Div {
    v_flex()
        .flex_none()
        .w(tokens::stack_width())
        .h_full()
        .gap(px(4.))
        .children(rows)
}

/// One of its rows, for a command this editor doesn't have yet: greyed, not
/// clickable and left out of the roving group, exactly as [`disabled_tile`].
pub(super) fn stack_row(id: &'static str, icon: IconName, label: &'static str) -> Stateful<Div> {
    unavailable_row(id, icon, label, "not implemented")
}

/// [`unavailable_tile`]'s stack row: the same reason, the same reasoning.
fn unavailable_row(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    reason: &'static str,
) -> Stateful<Div> {
    base_row(id, icon, label)
        .cursor_not_allowed()
        .text_color(tokens::text_disabled())
        .tooltip(move |window, cx| super::tooltip::text(format!("{label} — {reason}"), window, cx))
}

/// A stack row that does something: the same geometry as [`stack_row`] with
/// [`base_tile`]'s enabled styling (hover wash, pressed wash, keyboard focus
/// ring) and a place in the ribbon's roving group.
fn live_stack_row(
    nav: &Roving,
    id: &'static str,
    icon: IconName,
    label: &'static str,
    cx: &mut App,
) -> Stateful<Div> {
    let row = base_row(id, icon, label)
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::chrome())))
        .cursor_pointer()
        .text_color(tokens::text_label())
        .hover(|this| this.bg(tokens::hover()).text_color(tokens::text_full()))
        .active(|this| this.bg(tokens::ribbon_tab_active()));
    nav.claim(row, cx)
}

fn base_row(id: &'static str, icon: IconName, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .flex_1()
        .items_center()
        .gap(px(6.))
        .px(px(7.))
        .rounded(tokens::RADIUS)
        .bg(tokens::tile())
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .child(Icon::new(icon).size(tokens::text_xs()))
        .child(div().flex_1().truncate().child(label))
}

/// Between two groups: the frame's hairline, inset from the ribbon's own
/// padding so it stops short of both edges.
fn separator() -> AnyElement {
    div()
        .flex_none()
        .self_center()
        .w(px(1.))
        .h(tokens::separator_height())
        .bg(tokens::divider())
        .into_any_element()
}
