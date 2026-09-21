//! Studio's `Show Light Guides` (`Studio.yaml`: "Controls whether light
//! guide visualizations are shown in the viewport"): the lines a selected
//! light draws around itself, worked out by `rbx_viewer::light_guides` and
//! kept in step with the selection and with every edit.

use gpui_kit::Context;

use super::Shell;

impl Shell {
    /// Whether light guides are shown, for the viewport settings item to
    /// render its checked state.
    pub(super) fn light_guides_shown(&self) -> bool {
        self.light_guides
    }

    pub(super) fn toggle_light_guides(&mut self, cx: &mut Context<Self>) {
        self.light_guides = !self.light_guides;
        self.sync_light_guides(cx);
        self.save_settings();
    }

    /// Sends the viewport the guides of whatever lights are selected, as the
    /// DOM stands now — or none, with the setting off.
    ///
    /// Called on every selection change and every edit (see
    /// `Shell::reflect_changes`), since a `Range`, `Angle`, `Face` or
    /// `Color` typed into Properties and a parent part dragged across the
    /// scene both move them. Nothing is sent when they come out as they
    /// were: a drag reflects an edit per mouse move, and a command between
    /// two of its change batches would stop the render thread folding them
    /// into one (see `workspace_view::pump::coalesce`).
    pub(super) fn sync_light_guides(&mut self, cx: &mut Context<Self>) {
        let guides = if self.light_guides {
            rbx_viewer::light_guides(&self.dom, &self.database, self.selection.all())
        } else {
            Vec::new()
        };
        if guides == self.light_guides_sent {
            return;
        }
        self.light_guides_sent = guides.clone();
        self.viewport
            .update(cx, |viewport, _| viewport.set_lines(guides));
    }
}
