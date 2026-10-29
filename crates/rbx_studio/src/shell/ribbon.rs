//! **Row C**: the ribbon: grouped commands for whichever category tab Row
//! B has selected (Row B itself lives in `shell::chrome`, since it is a tab
//! strip like Row A's, not ribbon content).
//!
//! Two button shapes, both the frame's:
//!
//! - a **tile**, 42px wide, icon over label, filling the ribbon's height,
//!   for the commands worth aiming at;
//! - a **stack**, 78px wide, three rows of icon-beside-label, for the
//!   ones that belong to the tile beside them (Copy's paste/cut/duplicate)
//!   or that are really a readout (the snap increments).
//!
//! A tile is only clickable when this editor has a real handler behind it.
//! Everything Studio's ribbon offers that this editor doesn't do yet stays
//! visibly disabled with a tooltip saying so, rather than vanishing,
//! seeing the shape of what belongs here is worth more than a shorter
//! ribbon, and it's the same rule `menu_bar`'s greyed items already follow.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::chrome::Document;
use super::layout::Panel;
use super::roving::Roving;

use crate::script_editor::FinderMode;
use crate::script_templates::ScriptTemplates;
use crate::tokens;
use crate::transform::Tool;

use super::menu::{self, MenuId};
use super::play::{session::StopReason, Phase};
use super::Shell;

mod commands;
mod model_tiles;

pub(super) use commands::{
    palette_entries, RibbonCommand, GUI_INSERTS, PART_INSERTS, SUN_BODIES, SUN_MODES,
};
use commands::{Insert, SCRIPT_INSERTS};
pub(super) use model_tiles::is_locked;

/// The ribbon's category tabs. The frame names six; this editor has real
/// commands for Home, UI and Model, a page of its own for running a place
/// (Test, which the frame has no tab for). A tile with nothing behind it
/// stays on the page, greyed, with the reason on hover, the same answer the
/// menu bar's greyed items give.
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
    pub(super) fn ribbon(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        // The ribbon is one Tab stop whose control count depends on the
        // open page, so the group is opened here and closed once the page
        // has actually been built (see `shell::roving`).
        self.ribbon_nav.begin(&self.tab_order, None, cx);
        let groups: Vec<Vec<AnyElement>> = match self.ribbon_tab {
            Tab::Home => vec![
                self.clipboard_tiles(cx),
                self.transform_tools(cx),
                self.insert_tiles(cx),
                self.terrain_tiles(cx),
                self.settings_tiles(cx),
                self.panel_tiles(cx),
            ],
            Tab::Avatar => {
                let settings = vec![self.avatar_settings_tile(cx)];
                let mut tiles = self.rig_tiles(cx);
                tiles.push(self.animation_tile(cx));
                tiles.push(
                    tile(
                        &self.ribbon_nav,
                        "ribbon-accessory",
                        IconName::Package,
                        "Accessory",
                        cx,
                    )
                    .tooltip(|window, cx| {
                        super::tooltip::text(
                            "Accessory \u{2014} fit a mesh to a body as an accessory or layered clothing, test it, and generate it",
                            window,
                            cx,
                        )
                    })
                    .on_click(cx.listener(|shell, _, _, cx| shell.open_ugc_fitting(None, None, cx)))
                    .into_any_element(),
                );
                tiles.push(self.avatar_setup_tile(cx));
                let items = vec![
                    tile(
                        &self.ribbon_nav,
                        "ribbon-check-item",
                        IconName::ShieldCheck,
                        "Check Item",
                        cx,
                    )
                    .tooltip(|window, cx| {
                        super::tooltip::text(
                            "Check Item \u{2014} check an avatar item against Roblox\u{2019}s Marketplace limits",
                            window,
                            cx,
                        )
                    })
                    .on_click(cx.listener(|shell, _, _, cx| shell.open_ugc_validator(None, None, cx)))
                    .into_any_element(),
                    tile(
                        &self.ribbon_nav,
                        "ribbon-upload-item",
                        IconName::CloudUpload,
                        "Upload Item",
                        cx,
                    )
                    .tooltip(|window, cx| {
                        super::tooltip::text(
                            "Upload Item \u{2014} upload an avatar item and finish it on the Creator Hub",
                            window,
                            cx,
                        )
                    })
                    .on_click(cx.listener(|shell, _, _, cx| shell.open_ugc_upload(None, None, cx)))
                    .into_any_element(),
                    tile(
                        &self.ribbon_nav,
                        "ribbon-face-preview",
                        IconName::ScanFace,
                        "Face Preview",
                        cx,
                    )
                    .tooltip(|window, cx| {
                        super::tooltip::text(
                            "Face Preview \u{2014} pose a dynamic head\u{2019}s FACS controls and see its landmarks",
                            window,
                            cx,
                        )
                    })
                    .on_click(cx.listener(|shell, _, _, cx| shell.open_face_preview(cx)))
                    .into_any_element(),
                ];
                vec![settings, tiles, items]
            }
            Tab::Ui => vec![
                vec![self.insert_gui(cx).into_any_element()],
                vec![
                    unavailable_tile(
                        "ribbon-ui-anchor",
                        IconName::Anchor,
                        "Anchor",
                        "setting AnchorPoint from the ribbon isn't built; edit it in Properties",
                    )
                    .into_any_element(),
                    // The Align tool moves parts by `CFrame`; a GuiObject has
                    // `Position` and `Size` instead, which it doesn't write.
                    unavailable_tile(
                        "ribbon-ui-align",
                        IconName::AlignHorizontalJustifyCenter,
                        "Align",
                        "the Align tool moves parts, not GUI objects",
                    )
                    .into_any_element(),
                    unavailable_tile(
                        "ribbon-ui-auto-scale",
                        IconName::Scaling,
                        "Auto Scale",
                        "converting Offset sizes to Scale isn't built",
                    )
                    .into_any_element(),
                ],
            ],
            Tab::Script => vec![self.script_tiles(cx)],
            Tab::Model => vec![
                self.edit_tiles(cx),
                self.pivot_tiles(cx),
                self.sun_tiles(cx),
                self.import_tiles(cx),
            ],
            Tab::Test => vec![self.test_tiles(cx), self.viewport_tiles(cx)],
            Tab::Plugins => self.plugin_ribbon_groups(cx),
        };

        // One separator between groups, none at either end, the frame's own
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
            // last buttons is WCAG 1.4.4's "loss of content", the precise
            // thing the scale exists to avoid.
            .overflow_x_scroll()
            .pt(px(9.))
            .px(px(16.))
            .pb(px(11.))
            .gap(px(14.))
            .bg(tokens::chrome())
            .border_b(px(1.))
            .border_color(tokens::border())
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
    ///
    /// The tools, Local and Align sit inside one bordered cluster, icon
    /// only, each still has its own tooltip, so nothing here loses its
    /// accessible name by dropping a caption nobody needs to read twice.
    /// The snap readouts stay their own thing beside it: they are values,
    /// not tools.
    fn transform_tools(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let active = self.transform.effective();
        let cluster: Vec<AnyElement> = Tool::TRANSFORM
            .map(|tool| {
                let refusal = self.transform.refusal(tool);
                let id = ("tool", tool as usize);
                match refusal {
                    Some(_) => refused_icon(&self.ribbon_nav, id, tool_icon(tool), cx),
                    None => cluster_icon(&self.ribbon_nav, id, tool_icon(tool), cx),
                }
                .when(active == tool, |this| selected(this, tool_accent(tool)))
                .tooltip(move |window, cx| {
                    let text = match (refusal, tool.shortcut()) {
                        (Some(reason), _) => format!("{} — {reason}", tool.label()),
                        (None, Some(key)) => format!("{} ({key})", tool.label()),
                        (None, None) => tool.label().to_owned(),
                    };
                    super::tooltip::text(text, window, cx)
                })
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.transform_action(crate::transform::Action::Use(tool), cx);
                }))
                .into_any_element()
            })
            .into_iter()
            .chain([
                self.local_tile(cx).into_any_element(),
                self.align_control(cx).into_any_element(),
            ])
            .collect();

        vec![
            v_flex()
                .flex_none()
                .h_full()
                .justify_center()
                .child(
                    h_flex()
                        .flex_none()
                        .items_center()
                        .gap(px(4.))
                        .p(px(5.))
                        .bg(tokens::field_select())
                        .rounded(tokens::radius_container())
                        .border_1()
                        .border_color(tokens::border())
                        .children(cluster),
                )
                .into_any_element(),
            self.snap_stack(cx).into_any_element(),
        ]
    }

    /// §5.7, the three insert menus. Every item routes through
    /// `Shell::insert_instance` (or, for the Part menu's Block/Sphere/
    /// Cylinder, `Shell::insert_part`: see `insert_item`), the same paths
    /// the Model menu already uses, so a ribbon click and a menu-bar click
    /// are the same code path.
    fn insert_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let part = menu::dropdown_in_group(
            self,
            MenuId::InsertPart,
            super::chrome::Trigger::new(tile(
                &self.ribbon_nav,
                "ribbon-insert-part",
                IconName::Box,
                "Part",
                cx,
            )),
            PART_INSERTS.iter().map(insert_item).collect(),
            cx,
        );
        let script = menu::dropdown_in_group(
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
            // The Creator Store catalog: other creators' assets can't be
            // inserted through Roblox's public APIs (see `ROADMAP.md`), and
            // nothing here lists the user's own assets to insert instead.
            unavailable_tile(
                "ribbon-toolbox",
                IconName::Package,
                "Toolbox",
                "inserting other creators' assets needs Roblox's Toolbox",
            )
            .into_any_element(),
        ]
    }

    /// Home's Settings group, Studio's own: Game Settings.
    /// The viewport's own real controls, quality, orthographic, the axis
    /// indicator, are mostly in the viewport dock's settings menu, because
    /// they belong to the open document rather than to a Studio-named ribbon
    /// group; Settings opens the same page, and Show UI is the one switch
    /// Studio keeps on its ribbon.
    fn viewport_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let shown = self.show_screen_gui;
        let mut tiles =
            vec![
                tile(
                    &self.ribbon_nav,
                    "ribbon-viewport-settings",
                    IconName::Settings,
                    "Settings",
                    cx,
                )
                .tooltip(|window, cx| {
                    super::tooltip::text("Settings \u{2014} the viewport's settings", window, cx)
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.open_settings_on("viewport", cx)))
                .into_any_element(),
                tile(
                    &self.ribbon_nav,
                    "ribbon-show-ui",
                    IconName::Eye,
                    "Show UI",
                    cx,
                )
                .when(shown, |this| {
                    this.bg(tokens::accent_soft())
                        .text_color(tokens::check_on())
                })
                .tooltip(move |window, cx| super::tooltip::text(show_ui_tooltip(shown), window, cx))
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.set_screen_gui_shown(!shell.show_screen_gui, cx)
                }))
                .into_any_element(),
            ];
        if crate::beta::enabled(super::PSEUDO_LOC_BETA_ID) {
            let on = self.pseudo_locale().is_some();
            tiles.push(
                tile(
                    &self.ribbon_nav,
                    "ribbon-pseudo-loc",
                    IconName::Globe,
                    "Pseudo-loc",
                    cx,
                )
                .when(on, |this| {
                    this.bg(tokens::accent_soft())
                        .text_color(tokens::check_on())
                })
                .tooltip(|window, cx| {
                    super::tooltip::text(
                        "Pseudo-localize \u{2014} draw every GUI text longer and accented, and mark what no longer fits",
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(move |shell, _, _, cx| {
                    let locale = (!on).then(rbx_viewer::PseudoLocale::default);
                    shell.set_pseudo_locale(locale, cx);
                    if locale.is_some() {
                        shell.set_panel_open(Panel::Localization, true, cx);
                    }
                }))
                .into_any_element(),
            );
        }
        tiles
    }

    /// Avatar's Avatar Setup tile: Studio's tool turns a character mesh into
    /// a rig, which is what the auto-rigger does.
    /// Studio's first Avatar button, in a group of its own: the orange
    /// figure with a gear that opens the Avatar Settings dock.
    fn avatar_settings_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let size = px(20.);
        let badge = div()
            .absolute()
            .right(px(-2.))
            .bottom(px(-1.))
            .size(px(10.))
            .rounded_full()
            .bg(tokens::tile())
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(IconName::Cog)
                    .size(px(9.))
                    .text_color(tokens::text_full()),
            );
        let icon = div()
            .relative()
            .flex_none()
            .size(size)
            .child(
                Icon::new(IconName::PersonStanding)
                    .size_full()
                    .text_color(tokens::warning()),
            )
            .child(badge);
        let tile = tile_frame("ribbon-avatar-settings", icon, "Settings", true);
        self.ribbon_nav
            .claim(tile, cx)
            .tooltip(|window, cx| {
                super::tooltip::text(
                    "Avatar Settings \u{2014} the place's avatar rules",
                    window,
                    cx,
                )
            })
            .on_click(cx.listener(|shell, _, _, cx| shell.toggle_avatar_settings(cx)))
            .into_any_element()
    }

    fn avatar_setup_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        tile(
            &self.ribbon_nav,
            "ribbon-avatar-setup",
            IconName::Settings,
            "Avatar Setup",
            cx,
        )
        .tooltip(|window, cx| {
            super::tooltip::text(
                "Avatar Setup \u{2014} auto-rig a character mesh into an R15 rig",
                window,
                cx,
            )
        })
        .on_click(cx.listener(|shell, _, _, cx| shell.choose_autorig(cx)))
        .into_any_element()
    }

    /// The Script tab: Find and Replace open the script editor's finder
    /// overlay (its Find All / Replace All), Analysis toggles the Script
    /// Analysis panel. Format has nothing behind it.
    fn script_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let can_find = self.document == Document::Scripts && self.scripts.tabs.active().is_some();
        let finder = |id: &'static str,
                      icon,
                      label: &'static str,
                      tip: &'static str,
                      cx: &mut Context<Self>| {
            if can_find {
                tile(&self.ribbon_nav, id, icon, label, cx)
                    .tooltip(move |window, cx| {
                        super::tooltip::text(format!("{label} \u{2014} {tip}"), window, cx)
                    })
                    .on_click(cx.listener(|shell, _, window, cx| {
                        shell.open_finder(FinderMode::FindAll, window, cx)
                    }))
                    .into_any_element()
            } else {
                unavailable_tile(id, icon, label, "open a script first").into_any_element()
            }
        };
        let analysis = self.is_panel_showing(Panel::ScriptAnalysis);
        vec![
            finder(
                "ribbon-script-find",
                IconName::Search,
                "Find",
                "search every open script",
                cx,
            ),
            finder(
                "ribbon-script-replace",
                IconName::Type,
                "Replace",
                "replace across every open script",
                cx,
            ),
            unavailable_tile(
                "ribbon-script-format",
                IconName::Code,
                "Format",
                "a Luau formatter isn't bundled",
            )
            .into_any_element(),
            tile(
                &self.ribbon_nav,
                "ribbon-script-analysis",
                IconName::Bug,
                "Analysis",
                cx,
            )
            .when(analysis, |this| {
                this.bg(tokens::accent_soft())
                    .text_color(tokens::check_on())
            })
            .tooltip(|window, cx| {
                super::tooltip::text("Analysis \u{2014} the Script Analysis panel", window, cx)
            })
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.set_panel_open(Panel::ScriptAnalysis, !analysis, cx);
            }))
            .into_any_element(),
        ]
    }

    fn settings_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![
            tile(
                &self.ribbon_nav,
                "ribbon-game-settings",
                IconName::Settings,
                "Game Settings",
                cx,
            )
            .tooltip(|window, cx| {
                super::tooltip::text(
                    "Game Settings — the experience's settings and this place's own",
                    window,
                    cx,
                )
            })
            .on_click(cx.listener(|shell, _, _, cx| RibbonCommand::GameSettings.run(shell, cx)))
            .into_any_element(),
            tile(
                &self.ribbon_nav,
                "ribbon-data-stores",
                IconName::Database,
                "Data Stores",
                cx,
            )
            .tooltip(|window, cx| {
                super::tooltip::text(
                    "Data Stores — browse and edit the experience's data stores",
                    window,
                    cx,
                )
            })
            .on_click(cx.listener(|shell, _, _, cx| RibbonCommand::DataStores.run(shell, cx)))
            .into_any_element(),
        ]
    }

    /// One toggle per dock, and the reason they exist: a dock closed from
    /// its own tab has no tab left to bring it back with. The View menu
    /// carries the same toggles, through the same call, a window you can
    /// shut and not reopen is a window you have lost.
    fn panel_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut tiles = vec![
            (Panel::Explorer, IconName::ListTree, "Explorer"),
            (Panel::Properties, IconName::SlidersHorizontal, "Properties"),
            (Panel::Output, IconName::Terminal, "Output"),
            (Panel::Viewport, IconName::MonitorCog, "Viewport"),
            (Panel::InputActions, IconName::Gamepad2, "Input Actions"),
            (
                Panel::SecurityAudit,
                IconName::ShieldAlert,
                "Security Audit",
            ),
            (Panel::Playtests, IconName::Film, "Playtests"),
            (Panel::Layers, IconName::Layers, "Layers"),
            (Panel::Loren, IconName::Layers, "Loren"),
        ];
        if crate::beta::enabled(super::PSEUDO_LOC_BETA_ID) {
            tiles.push((Panel::Localization, IconName::Globe, "Localization"));
        }
        // Argon and Wally have no tab of their own to reopen from outside
        // the Script Editor, because they're nowhere to be seen outside it
        // either, see `Shell::hidden_panels`.
        if self.document == Document::Scripts {
            tiles.push((Panel::Argon, IconName::RefreshCw, "Argon"));
            tiles.push((Panel::Wally, IconName::Package, "Wally"));
            tiles.push((
                Panel::ScriptAnalysis,
                IconName::ListChecks,
                "Script Analysis",
            ));
            tiles.push((Panel::Watch, IconName::Eye, "Watch"));
            tiles.push((Panel::CallStack, IconName::Layers, "Call Stack"));
            tiles.push((Panel::Tests, IconName::CircleCheck, "Tests"));
        }
        tiles
            .into_iter()
            .map(|(panel, icon, label)| {
                let showing = self.is_panel_showing(panel);
                tile(&self.ribbon_nav, label, icon, label, cx)
                    .when(showing, |this| {
                        this.bg(tokens::accent_soft())
                            .text_color(tokens::check_on())
                    })
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.set_panel_open(panel, !showing, cx);
                    }))
                    .into_any_element()
            })
            .collect()
    }

    fn insert_gui(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        menu::dropdown_in_group(
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
                insert_item(&GUI_INSERTS[0]),
                insert_item(&GUI_INSERTS[1]),
                // Roblox's own ad surface: it needs a Creator-Dashboard ad
                // unit behind it to mean anything, which this editor has no
                // way to provision.
                menu::item("AdGui").icon(IconName::Megaphone).disabled(),
                insert_item(&GUI_INSERTS[2]),
            ],
            cx,
        )
    }

    /// Group/Ungroup are live (`shell::group`, the same Ctrl+G/Ctrl+Shift+G
    /// handlers the Model menu calls). Material, Color, Lock and Anchor live
    /// in `model_tiles`, and act on the whole selection's parts.
    /// Cut/Copy/Paste/Duplicate run the same `shell::clipboard` entry points
    /// as `menu_bar`'s Edit menu and `Ctrl+X`/`C`/`V`/`D`.
    ///
    /// Each is greyed exactly while its own handler would return early
    /// without doing anything, `clipboard::has_copyable` is that handler's
    /// own guard, so the button and the command cannot disagree about when
    /// there is something to act on. A control that is greyed is also out
    /// of the roving group (see `unavailable_tile`), so arrowing along the
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
    fn edit_tiles(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
            self.material_tile(cx),
            self.color_tile(cx),
            self.flag_tile(model_tiles::Flag::Locked, cx),
            self.flag_tile(model_tiles::Flag::Anchored, cx),
        ]
    }
}

/// A primary or destructive tile's 44x44 floor (WCAG 2.5.5), whatever the
/// UI scale or Large Click Targets say.
pub(super) fn primary(id: &'static str, tile: Stateful<Div>) -> Stateful<Div> {
    tile.debug_selector(|| id.into())
        .min_w(tokens::primary_target())
        .min_h(tokens::primary_target())
}

/// `ROADMAP.md`'s Play/Test section still lists the sandbox-place design
/// all of these would need as open.
impl Shell {
    /// Studio's "Current: Client / Server" switch: which game tree the
    /// Explorer and Properties show while a test runs.
    fn view_tile(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        use super::play::live::View;
        let view = self.play.live.view;
        let (icon, label) = match view {
            View::Client => (IconName::Globe, "Client"),
            View::Server => (IconName::Server, "Server"),
        };
        if self.play.phase == Phase::Running {
            let next = match view {
                View::Client => View::Server,
                View::Server => View::Client,
            };
            tile(&self.ribbon_nav, "ribbon-view", icon, label, cx)
                .on_click(cx.listener(move |shell, _, _, cx| shell.live_set_view(next, cx)))
        } else {
            unavailable_tile("ribbon-view", icon, label, "no test is running")
        }
    }

    /// The running game's numbers beside the test tiles: one row per side
    /// (`play::session::stats_line`), every number in the row's tooltip.
    /// Nothing while no test runs.
    fn stats_readout(&self) -> Option<Div> {
        use super::play::session::{stats_details, stats_line};
        use rbx_play::protocol::Side;
        if self.play.phase != Phase::Running {
            return None;
        }
        let row = |id: &'static str, icon: IconName, side: Side, metrics| {
            let (line, details) = match metrics {
                Some(metrics) => (stats_line(side, metrics), stats_details(metrics)),
                None => (
                    "\u{2014}".to_owned(),
                    "Waiting for the first report.".to_owned(),
                ),
            };
            h_flex()
                .id(id)
                .w_full()
                .flex_1()
                .items_center()
                .gap(px(6.))
                .px(px(7.))
                .rounded(tokens::radius())
                .bg(tokens::tile())
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .text_color(tokens::text_label())
                .child(Icon::new(icon).size(tokens::text_xs()))
                .child(div().flex_1().truncate().child(line))
                .tooltip(move |window, cx| super::tooltip::text(details.clone(), window, cx))
        };
        Some(
            v_flex()
                .flex_none()
                .w(px(190.))
                .h_full()
                .gap(px(4.))
                .child(row(
                    "ribbon-stats-client",
                    IconName::Globe,
                    Side::Client,
                    self.play.stats.client.as_ref(),
                ))
                .child(row(
                    "ribbon-stats-server",
                    IconName::Server,
                    Side::Server,
                    self.play.stats.server.as_ref(),
                )),
        )
    }

    fn test_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let nav = &self.ribbon_nav;
        let idle = self.play.phase == Phase::Idle;
        let play = if idle {
            tile(nav, "ribbon-play", IconName::Play, "Play", cx)
                .on_click(cx.listener(|shell, _, window, cx| shell.play_start(window, cx)))
        } else {
            unavailable_tile(
                "ribbon-play",
                IconName::Play,
                "Play",
                "a test is already running",
            )
        };
        let stop = if idle {
            unavailable_tile(
                "ribbon-stop",
                IconName::Square,
                "Stop",
                "nothing is running",
            )
        } else {
            tile(nav, "ribbon-stop", IconName::Square, "Stop", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.play_stop(StopReason::Stopped, cx)))
        };
        let invite = if self.play.job_id.is_some() {
            tile(nav, "ribbon-invite", IconName::Users, "Invite", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.play_copy_join_link(cx)))
        } else {
            unavailable_tile(
                "ribbon-invite",
                IconName::Users,
                "Invite",
                "no test server yet",
            )
        };
        let mut tiles = vec![
            primary("ribbon-play", play).into_any_element(),
            // Stop is a tile of its own rather than a third stack row: three
            // rows in an 80px ribbon are ~25px each, and Stop is the control a
            // person reaches for in a hurry.
            stack(vec![
                unavailable_row(
                    "ribbon-run",
                    IconName::SquarePlay,
                    "Run",
                    "a server-only run needs Roblox's engine; Play runs the place on Roblox",
                ),
                unavailable_row(
                    "ribbon-pause",
                    IconName::Pause,
                    "Pause",
                    "there is no local run to pause",
                ),
            ])
            .into_any_element(),
            primary("ribbon-stop", stop).into_any_element(),
            invite.into_any_element(),
            self.view_tile(cx).into_any_element(),
            self.network_tile(cx),
            self.device_tile(cx),
            unavailable_tile(
                "ribbon-team",
                IconName::Users,
                "Team Test",
                "it needs Roblox's Team Create servers",
            )
            .into_any_element(),
            self.controller_tile(cx).into_any_element(),
        ];
        tiles.extend(self.stats_readout().map(IntoElement::into_any_element));
        tiles
    }
}

impl Shell {
    /// Test › Controller: the Controller Emulator window.
    fn controller_tile(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        tile(
            &self.ribbon_nav,
            "ribbon-controller",
            IconName::Gamepad2,
            "Controller",
            cx,
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.open_controller_emulator(cx)))
    }
}

/// The Play and Stop tiles in isolation, for the 44x44 layout check.
#[cfg(test)]
fn test_tiles() -> Vec<AnyElement> {
    vec![
        primary(
            "ribbon-play",
            unavailable_tile("ribbon-play", IconName::Play, "Play", "test"),
        )
        .into_any_element(),
        primary(
            "ribbon-stop",
            unavailable_tile("ribbon-stop", IconName::Square, "Stop", "test"),
        )
        .into_any_element(),
    ]
}

/// The tooltip of the Show UI tile for the overlay's current state.
fn show_ui_tooltip(shown: bool) -> &'static str {
    if shown {
        "Show UI \u{2014} hide the ScreenGuis drawn over the 3D view"
    } else {
        "Show UI \u{2014} draw the ScreenGuis over the 3D view again"
    }
}

/// One insert-menu row, running the same [`RibbonCommand`] the command
/// palette lists for it.
fn insert_item(item: &'static Insert) -> menu::Item {
    menu::item(item.label)
        .icon(item.icon)
        .on_click(move |shell, cx| RibbonCommand::Insert(item).run(shell, cx))
}

/// The three built-in script classes, the way into the templates window,
/// then whatever the user has put in their templates directory, labelled
/// `"<name> (<class>)"` so two templates of different classes can share a
/// name without being ambiguous. Rebuilt every frame, so a template added on
/// disk shows up as soon as `Shell` reloads the list.
fn script_items(templates: &ScriptTemplates) -> Vec<menu::Item> {
    let mut items: Vec<menu::Item> = SCRIPT_INSERTS.iter().map(insert_item).collect();
    items.extend([
        menu::separator(),
        menu::item("Manage templates\u{2026}")
            .icon(IconName::Pencil)
            .on_click(|shell, cx| shell.open_script_templates(cx)),
    ]);
    if !templates.extras().is_empty() {
        items.push(menu::separator());
    }
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
        // Roblox's own glyph is a bounding box with a handle at each corner
        // this project ships no Roblox icon assets (see `CONTRIBUTING.md`), so a
        // generic Lucide glyph standing for the same idea, a frame with its
        // own corner handles, is this editor's equivalent rather than a copy.
        Tool::Transform => IconName::SquareDashed,
        Tool::Sun => IconName::Sun,
        Tool::Pivot => IconName::LocateFixed,
        Tool::Terrain => IconName::Mountain,
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
        Tool::Transform => tokens::tool_transform(),
        Tool::Sun => tokens::tool_sun(),
        Tool::Pivot => tokens::tool_pivot(),
        // Scale's green: the closest of the existing tool hues to terrain.
        Tool::Terrain => tokens::tool_scale(),
    }
}

/// Marks a tool button as the active one.
///
/// Three cues, not one: a wash of the tool's pastel, a 1.5px border in the
/// same pastel, and the icon itself tinted. WCAG 1.4.1 forbids colour as
/// the *only* means of conveying state, and the border is the part that
/// survives grayscale and every kind of colour blindness, the tools
/// already differ by icon shape, so between the two an active tool is
/// identifiable with no hue perception at all.
pub(super) fn selected(tile: Stateful<Div>, accent: Rgba) -> Stateful<Div> {
    tile.bg(tokens::tool_wash(accent))
        .border(tokens::tool_border())
        .border_color(accent)
        .text_color(accent)
}

/// A bare icon inside the transform-tools cluster: no fill or radius of its
/// own at rest, so the icons read as members of one shared card rather than
/// six tiles glued together, the border and the wash [`selected`] paints
/// are what a hover or an active tool adds on top of this.
/// A cluster icon whose tool can't be used right now: greyed, and a hover
/// or press lights nothing. Its tooltip says why, and a click on it does
/// nothing (`Shell::transform_action` refuses the tool too). Built on
/// [`cluster_frame`] rather than over [`cluster_icon`]: an element takes
/// one hover style, and a second one panics in a debug build.
pub(super) fn refused_icon(
    nav: &Roving,
    id: impl Into<ElementId>,
    icon: IconName,
    cx: &mut App,
) -> Stateful<Div> {
    let el = cluster_frame(id, icon)
        .cursor_not_allowed()
        .text_color(tokens::text_disabled());
    nav.claim(el, cx)
}

pub(super) fn cluster_icon(
    nav: &Roving,
    id: impl Into<ElementId>,
    icon: IconName,
    cx: &mut App,
) -> Stateful<Div> {
    let el = cluster_frame(id, icon)
        .cursor_pointer()
        .text_color(tokens::text_muted())
        .hover(|this| {
            tokens::hover_fx(this)
                .bg(tokens::hover())
                .text_color(tokens::text_full())
        })
        .active(|this| this.bg(tokens::ribbon_tab_active()));
    nav.claim(el, cx)
}

/// What [`cluster_icon`] and [`refused_icon`] share: the tile and its
/// glyph, with no pointer styles yet.
fn cluster_frame(id: impl Into<ElementId>, icon: IconName) -> Stateful<Div> {
    // 38x38, well over the WCAG floor: the icons this size sits next to
    // (Part, Script, UI, Toolbox) are drawn at a similar weight, and a
    // cluster of small glyphs beside them read as an afterthought.
    div()
        .id(id.into())
        .flex_none()
        .size(tokens::scaled_width(38.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::radius_tile())
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .child(Icon::new(icon).size(tokens::scaled_width(17.)))
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

/// A disabled control is deliberately **not** claimed into the roving
/// group: the APG's toolbar pattern focuses the first non-disabled control
/// on entry and arrows skip the rest, and a keyboard user landing on a
/// button that cannot do anything is a dead end with no way to tell why.
/// The tooltip still explains it on hover.
///
/// The same greyed, unclaimed tile with a reason on hover: either a command
/// that cannot act right now (nothing selected, an empty clipboard), which
/// the person at the keyboard can fix in a second, or one this editor can't
/// offer at all, where the reason names what is missing.
///
/// Offering the click and doing nothing is the worst of the options, which
/// is what this exists to stop.
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
    tile_frame(id, icon_zone(icon, px(20.)), label, enabled)
}

/// [`tile`] with a badge beside its icon (the Network tile's connection
/// pill). The tile grows past the usual width to fit it, so the badge never
/// covers the icon whatever its text.
pub(super) fn tile_with_badge(
    nav: &Roving,
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    badge: impl IntoElement,
    cx: &mut App,
) -> Stateful<Div> {
    let row = h_flex()
        .flex_none()
        .gap(px(3.))
        .items_center()
        .child(icon_zone(icon, px(20.)))
        .child(badge);
    let tile = tile_frame(id, row, label, true)
        .w_auto()
        .min_w(tokens::tile_width())
        .px(px(6.));
    nav.claim(tile, cx)
}

/// A tile's box, states and label, under `top` (its icon).
fn tile_frame(
    id: impl Into<ElementId>,
    top: impl IntoElement,
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
        .rounded(tokens::radius())
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
                    .hover(|this| {
                        tokens::hover_fx(this)
                            .bg(tokens::hover())
                            .text_color(tokens::text_full())
                    })
                    .active(|this| this.bg(tokens::ribbon_tab_active()))
            } else {
                this.cursor_not_allowed()
                    .text_color(tokens::text_disabled())
            }
        })
        .group("ribbon-tile")
        .child(top)
        .child(div().w_full().text_center().truncate().child(label))
}

/// A ribbon button's icon, and its hover lift.
///
/// The spec asks for a 120ms 1.06x scale. GPUI has no element transform, so
/// the lift is a size change on the icon's own fixed box rather than a
/// transform, the label below it doesn't move, because the box the icon
/// fills is what grows. The vendored GPUI eases a hover's pixel sizes over
/// `tokens::DURATION_HOVER` (the spec's 120ms), given an id to keep the blend
/// in, and drops the easing under reduced motion; the lift itself is dropped
/// there too, since a jump is still a change and the background hover
/// carries the whole affordance without it.
fn icon_zone(icon: IconName, size: Pixels) -> Stateful<Div> {
    let lifted = px(f32::from(size) * 1.06);

    div()
        .id("icon-zone")
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

/// [`unavailable_tile`]'s stack row: the same reason, the same reasoning.
pub(super) fn unavailable_row(
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

/// A stack row that does something: the same geometry as [`unavailable_row`] with
/// [`base_tile`]'s enabled styling (hover wash, pressed wash, keyboard focus
/// ring) and a place in the ribbon's roving group.
pub(super) fn live_stack_row(
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
        .hover(|this| {
            tokens::hover_fx(this)
                .bg(tokens::hover())
                .text_color(tokens::text_full())
        })
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
        .rounded(tokens::radius())
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
        .bg(tokens::border())
        .into_any_element()
}

#[cfg(test)]
#[path = "ribbon_tests.rs"]
mod tests;
