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
    panel_handle, BasePanel, DockArea, DockLayout, DockSkin, Panel as ComponentPanel, PanelEvent,
    TitleStyle,
};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use super::{Shell, EXPLORER_WIDTH, OUTPUT_HEIGHT, PROPERTIES_HEIGHT};

/// Which of Shell's three sections a [`SectionPanel`] delegates to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Viewport,
    Explorer,
    Properties,
    Output,
}

impl Section {
    fn name(self) -> &'static str {
        match self {
            Section::Viewport => "Viewport",
            Section::Explorer => "Explorer",
            Section::Properties => "Properties",
            Section::Output => "Output",
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        self.shell.update(cx, |shell, cx| match section {
            Section::Viewport => shell.viewport(cx).into_any_element(),
            Section::Explorer => shell.explorer(cx).into_any_element(),
            Section::Properties => shell.properties(window, cx).into_any_element(),
            Section::Output => shell.output_panel(cx).into_any_element(),
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
            Section::Explorer | Section::Output => SharedString::from(self.section.name()),
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

    /// Explorer's "Show all services" toggle, moved off the search row and
    /// into the panel's own overflow menu (see `Shell::show_all_services` /
    /// `Shell::set_show_all_services`); Viewport's own "Orthographic" toggle
    /// (see `Shell::orthographic` / `Shell::set_orthographic`) lives the same
    /// way, next to the quality dropdown already in its title bar.
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
                menu.item(
                    PopupMenuItem::new("Show all services")
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            shell.update(cx, |shell, cx| {
                                let next = !shell.show_all_services();
                                shell.set_show_all_services(next, cx);
                            });
                        }),
                )
            }
            Section::Viewport => {
                let checked = shell.read(cx).orthographic();
                menu.item(
                    PopupMenuItem::new("Orthographic")
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            shell.update(cx, |shell, cx| {
                                let next = !shell.orthographic();
                                shell.set_orthographic(next, cx);
                            });
                        }),
                )
            }
            _ => menu,
        }
    }
}

/// Builds the dock area with the default arrangement — viewport over Output
/// on the left, Explorer stacked over Properties on the right — exactly what
/// the old `h_resizable` split drew (plus Output's own bottom slot), now
/// rearrangeable by dragging.
pub(super) fn build(
    shell: Entity<Shell>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Entity<DockArea> {
    let (area, _skin) = DockSkin::dock_area("shell", Some(1), window, cx);

    let viewport = cx.new(|cx| SectionPanel::new(shell.clone(), Section::Viewport, cx));
    let explorer = cx.new(|cx| SectionPanel::new(shell.clone(), Section::Explorer, cx));
    let properties = cx.new(|cx| SectionPanel::new(shell.clone(), Section::Properties, cx));
    let output = cx.new(|cx| SectionPanel::new(shell, Section::Output, cx));

    area.update(cx, |area, cx| {
        area.set_center(
            DockLayout::h_split()
                .child(
                    // A `v_split` rather than `DockPlacement::Bottom`: that
                    // placement spans the whole area under the Explorer and
                    // Properties column too (see `gpui_base::dock::dock_area`'s
                    // `center_frame` doc), but Output belongs only under the
                    // viewport, like Roblox Studio's own Output window.
                    DockLayout::v_split()
                        .child(
                            DockLayout::tabs().panel_view(panel_handle(viewport), cx),
                            None,
                        )
                        .child(
                            DockLayout::tabs().panel_view(panel_handle(output), cx),
                            Some(px(OUTPUT_HEIGHT)),
                        ),
                    None,
                )
                .child(
                    DockLayout::v_split()
                        .child(
                            DockLayout::tabs().panel_view(panel_handle(explorer), cx),
                            None,
                        )
                        .child(
                            DockLayout::tabs().panel_view(panel_handle(properties), cx),
                            Some(px(PROPERTIES_HEIGHT)),
                        ),
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
    use std::sync::Arc;

    // Register all four section panels so they can be reconstructed from saved state.
    for section in &[
        Section::Viewport,
        Section::Explorer,
        Section::Properties,
        Section::Output,
    ] {
        let section = *section;
        let shell_clone = shell.clone();
        register_panel(cx, section.name(), move |_context, _window, cx| {
            let panel = cx.new(|cx| SectionPanel::new(shell_clone.clone(), section, cx));
            Arc::new(panel)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::Section;

    // `panel_name` documents that its value must never change once chosen
    // (it will be the persisted layout's panel key once settings land), so
    // this is worth locking down even though the rest of this module needs a
    // live GPUI window to exercise.
    #[test]
    fn every_section_has_a_distinct_stable_name() {
        let names = [
            Section::Viewport.name(),
            Section::Explorer.name(),
            Section::Properties.name(),
            Section::Output.name(),
        ];
        assert_eq!(names, ["Viewport", "Explorer", "Properties", "Output"]);
    }
}
