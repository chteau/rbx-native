//! What an editor tells the renderer to *show*, as opposed to what the place
//! holds: the outlined selection, the "about to click" hover cue, the ghost
//! boxes a tool is previewing and the transform gizmo's draggers.
//!
//! Split out of [`Renderer`] because they are one family — none of them is
//! rebuilt from the DOM, all of them survive a reload through
//! [`crate::view::View`], and every one is a setter the embedder calls
//! between frames.

use super::cue::Cues;
use super::highlight;
use super::{Renderer, Selected};
use crate::gizmo::Gizmo;
use crate::scene::Scene;

impl Renderer {
    /// Replaces the outlined selection, rebuilding its tiny vertex buffer right
    /// away rather than waiting for the next `draw`.
    ///
    /// Two cues, not one: a container gets the box `renderer::selection`
    /// draws, and anything with a shape of its own gets the silhouette
    /// `renderer::cue` traces — which is why this needs the scene the
    /// silhouette is masked from.
    pub(crate) fn set_selection(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        selected: &[Selected],
        scene: &Scene,
    ) {
        self.selection.set(device, selected);
        self.cues.set_selection(
            device,
            queue,
            &self.frame_layout,
            Cues::parts_of(selected),
            Self::cue_source(scene),
        );
    }

    /// Whether scene geometry in front of the selection hides its outline —
    /// see `renderer::selection`, which draws it through everything by
    /// default.
    pub(crate) fn set_selection_occluded(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        occluded: bool,
        scene: &Scene,
    ) {
        self.selection.set_occluded(occluded);
        self.cues.set_occluded(
            device,
            queue,
            &self.frame_layout,
            occluded,
            Self::cue_source(scene),
        );
    }

    /// Replaces the hover outline, rebuilding its tiny vertex buffer right
    /// away rather than waiting for the next `draw`. An empty list clears it.
    pub(crate) fn set_hover(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        selected: Vec<Selected>,
        scene: &Scene,
    ) {
        let parts = Cues::parts_of(&selected);
        self.hover.set(device, selected);
        self.cues.set_hover(
            device,
            queue,
            &self.frame_layout,
            parts,
            Self::cue_source(scene),
        );
    }

    /// Replaces the ghost boxes a tool is previewing — see
    /// `renderer::preview`. An empty list clears them.
    pub(crate) fn set_preview(&mut self, device: &wgpu::Device, boxes: &[glam::Mat4]) {
        self.preview.set(device, boxes);
    }

    /// Shows or hides the transform tool's draggers over whatever is
    /// selected. Their geometry is rebuilt inside [`Renderer::draw`] rather
    /// than here: the arms are scaled to hold a constant size on screen, so
    /// they change with every camera move, not only when the tool does.
    pub(crate) fn set_gizmo(&mut self, gizmo: Option<Gizmo>) {
        self.gizmo = gizmo;
    }

    /// What the cue pass masks a silhouette out of: the same parts and
    /// resolved meshes the place's own highlights are drawn from, with no
    /// highlight of its own — `renderer::cue` supplies those.
    fn cue_source(scene: &Scene) -> highlight::Source<'_> {
        highlight::Source {
            highlights: &[],
            parts: scene.parts(),
            resolved: scene.resolved_file_meshes(),
        }
    }
}
