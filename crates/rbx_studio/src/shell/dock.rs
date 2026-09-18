//! Wires Shell's three sections into `gpui_component`'s `DockArea`, in place
//! of the old fixed `h_resizable` split: panels can now be dragged to another
//! edge of the layout or stacked as tabs (see the ROADMAP's "Agencement des
//! panneaux").
//!
//! Each [`SectionPanel`] owns no state of its own — it renders by calling
//! back into `Shell`, which still owns the viewport, explorer and properties
//! state exactly as it did under the fixed layout. That keeps this a pure
//! presentation change: nothing about how those three sections work moved.

use gpui_kit::component::dock::{
    panel_handle, BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, InsertTarget, NodeId,
    PaneRef, Panel as ComponentPanel, PanelEvent, PanelId, TitleStyle,
};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{v_flex, ActiveTheme};
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::pacing::UnfocusedFps;

use super::{Shell, EXPLORER_WIDTH, OUTPUT_HEIGHT, PROPERTIES_WIDTH};

/// Which of Shell's three sections a [`SectionPanel`] delegates to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Viewport,
    Explorer,
    Properties,
    Output,
    Scripts,
    StyleEditor,
}

impl Section {
    fn name(self) -> &'static str {
        match self {
            Section::Viewport => "Viewport",
            Section::Explorer => "Explorer",
            Section::Properties => "Properties",
            Section::Output => "Output",
            Section::Scripts => "Script Editor",
            Section::StyleEditor => "Style Editor",
        }
    }
}

/// A dock panel with no state of its own: it renders by delegating into
/// `Shell`'s own section methods, so the dock area is only a container.
struct SectionPanel {
    shell: Entity<Shell>,
    section: Section,
    focus_handle: FocusHandle,
}

impl SectionPanel {
    fn new(shell: Entity<Shell>, section: Section, cx: &mut Context<Self>) -> Self {
        Self {
            shell,
            section,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Focusable for SectionPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for SectionPanel {}

impl Render for SectionPanel {
    /// Viewport/Scripts/StyleEditor each get the ribbon (`shell::ribbon`)
    /// prepended to their own content, rather than Shell rendering it once
    /// above the whole dock area — that's what keeps this trio's shared tab
    /// strip (drawn by the dock itself, above whichever of the three is
    /// active) the first thing under the menu bar, with the ribbon directly
    /// under *that*, matching a maintainer-supplied reference. Duplicating
    /// the one `self.ribbon(cx)` call three times costs nothing real: it
    /// reads the same `Shell` state each time and paints identically
    /// regardless of which of the three tabs is actually showing it.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        self.shell.update(cx, |shell, cx| match section {
            Section::Viewport => v_flex()
                .size_full()
                .child(shell.ribbon(cx))
                .child(div().flex_1().overflow_hidden().child(shell.viewport(cx)))
                .into_any_element(),
            Section::Explorer => shell.explorer(cx).into_any_element(),
            Section::Properties => shell.properties(window, cx).into_any_element(),
            Section::Output => shell.output_panel(cx).into_any_element(),
            Section::Scripts => v_flex()
                .size_full()
                .child(shell.ribbon(cx))
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(shell.script_editor(window, cx)),
                )
                .into_any_element(),
            Section::StyleEditor => v_flex()
                .size_full()
                .child(shell.ribbon(cx))
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(shell.style_editor(window, cx)),
                )
                .into_any_element(),
        })
    }
}

impl BasePanel for SectionPanel {
    fn panel_name(&self) -> &'static str {
        self.section.name()
    }

    /// Nothing in the shell offers a way to reopen a closed panel yet, so
    /// closing one would strand the user with no path back to it.
    fn closable(&self, _: &App) -> bool {
        false
    }
}

impl ComponentPanel for SectionPanel {
    /// Viewport and Properties show live state (the place filename, the
    /// selected instance) instead of the section's static name — see
    /// `Shell::title` and `Shell::properties_title`. Explorer has nothing to
    /// add, so it just echoes `Section::name`.
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match self.section {
            Section::Viewport => self.shell.read(cx).title(),
            Section::Properties => self.shell.read(cx).properties_title(),
            Section::Explorer | Section::Output | Section::Scripts | Section::StyleEditor => {
                SharedString::from(self.section.name())
            }
        };
        // Smaller than the vendored default (see `title_style` below for the
        // matching colour change): the dock's own chrome should read quieter
        // than the panel content it labels.
        div().text_xs().child(label)
    }

    /// A greyish tab-bar foreground instead of the vendored crate's default
    /// near-black, to contrast against the panel interior — the same muted
    /// token the removed inner headers used to use. The background is
    /// repeated unchanged (`tokens.background`, what the title bar already
    /// sits on) so only the text colour actually changes.
    fn title_style(&self, cx: &App) -> Option<TitleStyle> {
        Some(TitleStyle {
            background: *cx.theme().tokens.background,
            foreground: cx.theme().muted_foreground,
        })
    }

    /// The graphics-quality dropdown, now that the Viewport panel no longer
    /// draws its own fake tab bar to hold it (see `Shell::quality_control`).
    /// The transform toolbar lives in the ribbon's "Tools" group instead
    /// (see `shell::ribbon`), not here — it applies to the whole editor's
    /// transform state, not just this one tab.
    fn title_suffix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        match self.section {
            Section::Viewport => Some(self.shell.read(cx).quality_control().into_any_element()),
            // Output's own controls (filter, Clear) need a mutable `Context<Shell>`
            // to wire their click handlers (see `Shell::output_controls`), unlike
            // Viewport's dropdown above which only reads state here.
            Section::Output => Some(
                self.shell
                    .update(cx, |shell, cx| shell.output_controls(cx).into_any_element()),
            ),
            _ => None,
        }
    }

    /// Explorer's "Show all services" and "Light Icons" toggles, moved off
    /// the search row and into the panel's own overflow menu (see
    /// `Shell::show_all_services` / `Shell::set_show_all_services` and
    /// `Shell::icon_pack` / `Shell::set_icon_pack`); Viewport's own
    /// "Orthographic" toggle (see `Shell::orthographic` /
    /// `Shell::set_orthographic`), its Orientation Indicator toggle (see
    /// `Shell::axis_indicator`), its Stats toggle, and its "Cap frame rate at
    /// 25 fps when unfocused" toggle (see `Shell::unfocused_fps`) all live
    /// the same way, next to the quality dropdown already in its title bar.
    /// Output's "Show Timestamp" toggle (`Shell::output_show_timestamps`)
    /// lives here too rather than crowding the level-filter/Clear row
    /// `output_controls` already puts in that panel's title bar.
    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PopupMenu {
        let shell = self.shell.clone();
        match self.section {
            Section::Explorer => {
                let checked = shell.read(cx).show_all_services();
                let show_all_shell = shell.clone();
                let icon_pack_checked = shell.read(cx).icon_pack() == IconPack::Light;
                menu.item(
                    PopupMenuItem::new("Show all services")
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            show_all_shell.update(cx, |shell, cx| {
                                let next = !shell.show_all_services();
                                shell.set_show_all_services(next, cx);
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new("Light Icons")
                        .checked(icon_pack_checked)
                        .on_click(move |_, _, cx| {
                            shell.update(cx, |shell, cx| {
                                let next = if shell.icon_pack() == IconPack::Light {
                                    IconPack::Dark
                                } else {
                                    IconPack::Light
                                };
                                shell.set_icon_pack(next, cx);
                            });
                        }),
                )
            }
            Section::Viewport => {
                let orthographic = shell.read(cx).orthographic();
                let axis_indicator = shell.read(cx).axis_indicator();
                let axis_indicator_shell = shell.clone();
                let stats_checked = shell.read(cx).stats_shown();
                let stats_shell = shell.clone();
                let unfocused_fps_shell = shell.clone();
                let unfocused_fps_checked = shell.read(cx).unfocused_fps() == UnfocusedFps::Fps25;
                menu.item(
                    PopupMenuItem::new("Orthographic")
                        .checked(orthographic)
                        .on_click(move |_, _, cx| {
                            shell.update(cx, |shell, cx| {
                                let next = !shell.orthographic();
                                shell.set_orthographic(next, cx);
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new("Orientation Indicator")
                        .checked(axis_indicator)
                        .on_click(move |_, _, cx| {
                            axis_indicator_shell.update(cx, |shell, cx| {
                                let next = !shell.axis_indicator();
                                shell.set_axis_indicator(next, cx);
                            });
                        }),
                )
                // Real Studio's own toggle is `Window > Performance > Stats`;
                // this editor has no `Window` menu yet, so it sits next to
                // the viewport's other debug affordance instead.
                .item(PopupMenuItem::new("Stats").checked(stats_checked).on_click(
                    move |_, _, cx| {
                        stats_shell.update(cx, |shell, cx| {
                            let next = !shell.stats_shown();
                            shell.set_stats_shown(next, cx);
                        });
                    },
                ))
                .item(
                    PopupMenuItem::new("Cap frame rate at 25 fps when unfocused")
                        .checked(unfocused_fps_checked)
                        .on_click(move |_, _, cx| {
                            unfocused_fps_shell.update(cx, |shell, cx| {
                                let next = if shell.unfocused_fps() == UnfocusedFps::Fps25 {
                                    UnfocusedFps::Fps30
                                } else {
                                    UnfocusedFps::Fps25
                                };
                                shell.set_unfocused_fps(next, cx);
                            });
                        }),
                )
            }
            Section::Output => {
                let checked = shell.read(cx).output_show_timestamps;
                menu.item(
                    PopupMenuItem::new("Show Timestamp")
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            shell.update(cx, |shell, cx| {
                                shell.output_show_timestamps = !shell.output_show_timestamps;
                                cx.notify();
                            });
                        }),
                )
            }
            _ => menu,
        }
    }
}

/// Builds the dock area with the default arrangement — Properties alone on
/// the left, Viewport (tabbed with the Script/Style editors) over Output in
/// the middle, Explorer alone on the right — matching a maintainer-supplied
/// ribbon-style Studio redesign reference (see `UX_GUIDELINES.md` §7).
/// Rearrangeable by dragging like before.
pub(super) fn build(
    shell: Entity<Shell>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Entity<DockArea> {
    let (area, _skin) = DockSkin::dock_area("shell", Some(1), window, cx);

    let viewport = cx.new(|cx| SectionPanel::new(shell.clone(), Section::Viewport, cx));
    let explorer = cx.new(|cx| SectionPanel::new(shell.clone(), Section::Explorer, cx));
    let properties = cx.new(|cx| SectionPanel::new(shell.clone(), Section::Properties, cx));
    let output = cx.new(|cx| SectionPanel::new(shell.clone(), Section::Output, cx));
    let scripts = cx.new(|cx| SectionPanel::new(shell.clone(), Section::Scripts, cx));
    let styles = cx.new(|cx| SectionPanel::new(shell, Section::StyleEditor, cx));

    area.update(cx, |area, cx| {
        area.set_center(
            DockLayout::h_split()
                .child(
                    DockLayout::tabs().panel_view(panel_handle(properties), cx),
                    Some(px(PROPERTIES_WIDTH)),
                )
                .child(
                    // A `v_split` rather than `DockPlacement::Bottom`: that
                    // placement spans the whole area under the Properties
                    // and Explorer columns too (see `gpui_base::dock::
                    // dock_area`'s `center_frame` doc), but Output belongs
                    // only under the viewport, like Roblox Studio's own
                    // Output window.
                    DockLayout::v_split()
                        .child(
                            DockLayout::tabs()
                                .panel_view(panel_handle(viewport), cx)
                                .panel_view(panel_handle(scripts), cx)
                                .panel_view(panel_handle(styles), cx),
                            None,
                        )
                        .child(
                            DockLayout::tabs().panel_view(panel_handle(output), cx),
                            Some(px(OUTPUT_HEIGHT)),
                        ),
                    None,
                )
                .child(
                    DockLayout::tabs().panel_view(panel_handle(explorer), cx),
                    Some(px(EXPLORER_WIDTH)),
                ),
            window,
            cx,
        );
    });

    area
}

/// Register the panel types with the dock so they can be restored from saved state.
/// This must be called before calling `dock_area.load()` for layout restoration to work.
pub(super) fn register_panels(_dock_area: Entity<DockArea>, shell: Entity<Shell>, cx: &mut App) {
    use gpui_kit::component::dock::register_panel;

    // Register every section panel so it can be reconstructed from saved state.
    for section in &[
        Section::Viewport,
        Section::Explorer,
        Section::Properties,
        Section::Output,
        Section::Scripts,
        Section::StyleEditor,
    ] {
        let section = *section;
        let shell_clone = shell.clone();
        register_panel(cx, section.name(), move |_context, _window, cx| {
            let panel = cx.new(|cx| SectionPanel::new(shell_clone.clone(), section, cx));
            // A bare `Arc::new(panel)` compiles here too — `Entity<P>` already
            // satisfies `PanelView` through `gpui_base`'s own blanket impl —
            // but it is not a `PanelHandle`, so `PanelHandle::of` (what the
            // tab bar downcasts through to recover a panel's dropdown menu
            // and zoom control, see `gpui_component::dock::tab_panel`) comes
            // back `None` for every panel rebuilt this way. `panel_handle` is
            // the same helper `build()` above already uses for exactly this
            // reason.
            panel_handle(panel)
        });
    }
}

/// Brings the Script Editor panel's own dock tab to the front, wherever in
/// the layout it currently sits, so opening a script is visible even when the
/// panel is stacked behind the Viewport (which is where the default layout
/// puts it).
///
/// `DockArea` has no "activate this panel" call, but moving a panel into the
/// tab group it is already in, at the index it already has, changes nothing
/// except raising it — `InsertTarget`'s own `activate` flag is what the tab
/// bar sets when a tab is clicked. Looking the panel up by name rather than
/// caching its id is what makes this keep working after a saved layout has
/// been restored, which rebuilds the panels as new entities.
pub(super) fn reveal_scripts(area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
    reveal(area, Section::Scripts.name(), window, cx);
}

/// [`reveal_scripts`] for the Style Editor panel, which the View menu opens
/// the same way (Roblox puts it under `Window` ⟩ UI, per
/// `studio/ui-overview.md`; this editor's menus are File/Edit/Model/View).
pub(super) fn reveal_style_editor(area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
    reveal(area, Section::StyleEditor.name(), window, cx);
}

fn reveal(area: &Entity<DockArea>, name: &str, window: &mut Window, cx: &mut App) {
    let Some((panel, node, ix)) = locate(area, name, cx) else {
        return;
    };
    area.update(cx, |area, cx| {
        area.move_panel(
            panel,
            InsertTarget::Tabs {
                node,
                ix,
                activate: true,
            },
            window,
            cx,
        );
    });
}

/// Where the panel called `name` currently lives: which panel id it has,
/// which tab group holds it, and its index within that group.
fn locate(
    area: &Entity<DockArea>,
    name: &str,
    cx: &App,
) -> Option<(PanelId, NodeId, Option<usize>)> {
    let dock = area.read(cx);
    for placement in [
        DockPlacement::Center,
        DockPlacement::Left,
        DockPlacement::Right,
        DockPlacement::Bottom,
    ] {
        let Some(tree) = dock.layout(placement) else {
            continue;
        };
        let found = tree.panels().find(|panel| {
            dock.panel(*panel)
                .is_some_and(|view| view.panel_name(cx) == name)
        });
        let Some(panel) = found else {
            continue;
        };
        let node = tree.find_panel_node(panel)?;
        // Re-inserting at the index it already has keeps the tab where the
        // user last left it; the move is only a way to raise it.
        let ix = match tree.find_node(node)?.kind() {
            PaneRef::Tabs { panels, .. } => panels.iter().position(|held| *held == panel),
            _ => None,
        };
        return Some((panel, node, ix));
    }
    None
}

#[cfg(test)]
mod tests;
