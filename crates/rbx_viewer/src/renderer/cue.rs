//! The editor's own cues — what the Explorer has selected, and what the
//! cursor is about to click — drawn as the silhouette of the geometry they
//! cover rather than as the box around it.
//!
//! A part keeps its shape here: a `Ball` outlines as a circle, a wedge as a
//! wedge and a downloaded `MeshPart` as its own polygon, because this is
//! the very mask-and-composite pass `renderer::highlight` draws a
//! `Highlight` with — pointed at the editor's selection instead of at an
//! instance in the file. Roblox publishes no shape-conformance spec for
//! Studio's own selection visual (its docs give it as "a light-blue
//! outline" and no more), so this is not a claim of parity: it is the same
//! answer this renderer already gives for the one outline effect that *is*
//! specified as a silhouette.
//!
//! A container — a `Model`, a `Folder` — still outlines as one box around
//! everything beneath it, which `renderer::selection` and
//! `renderer::hover` go on drawing: a model has no silhouette of its own to
//! trace, and the box around it is what Studio calls its bounding box.

use rbx_dom::Ref;

use super::geometry::Meshes;
use super::highlight::{Highlights, Source};
use super::pipeline::Target;
use super::post::Targets;
use crate::pick::Selected;
use crate::scene::{DepthMode, Highlight, Part, PartId, Resolved};

/// Studio's own selection blue, linearized — the same colour
/// `selection.wgsl` fills the container box with, kept in step by eye
/// because a WGSL constant cannot be shared with Rust.
const SELECTION: [f32; 3] = [0.106, 0.462, 0.911];
/// The hover cue's amber, from `hover.wgsl`, at the same reduced alpha that
/// shader blends its own box with.
const HOVER: [f32; 3] = [0.955, 0.380, 0.020];
const HOVER_ALPHA: f32 = 0.55;
/// What stands in `Highlight::referent` for a cue: no instance asked for
/// one, and nothing reads the field back — it exists so a re-planned list
/// can be matched to the DOM, which the editor's own cues never are.
const NO_INSTANCE: Ref = Ref::new(0);

/// The selection and hover silhouettes, as the one pass that draws both.
pub(super) struct Cues {
    pass: Highlights,
    selection: Vec<Ref>,
    hover: Vec<Ref>,
    /// Whether scene geometry in front of the selection hides it — the same
    /// setting `renderer::selection` carries for the container box, applied
    /// here as the `Highlight` depth mode that means the same thing.
    occluded: bool,
}

impl Cues {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::BindGroupLayout,
        target: Target,
        scene: Source<'_>,
    ) -> Self {
        Cues {
            pass: Highlights::new(
                device,
                queue,
                frame,
                target,
                Source {
                    highlights: &[],
                    ..scene
                },
            ),
            selection: Vec::new(),
            hover: Vec::new(),
            occluded: false,
        }
    }

    /// The parts of `selected` that have a silhouette of their own: a part
    /// stands for itself, a container for nothing here (its box is drawn by
    /// `renderer::selection`).
    pub(super) fn parts_of(selected: &[Selected]) -> Vec<Ref> {
        selected
            .iter()
            .filter(|entry| entry.is_part())
            .flat_map(|entry| entry.parts())
            .copied()
            .collect()
    }

    pub(super) fn set_selection(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::BindGroupLayout,
        selection: Vec<Ref>,
        scene: Source<'_>,
    ) {
        self.selection = selection;
        self.rebuild(device, queue, frame, scene);
    }

    pub(super) fn set_hover(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::BindGroupLayout,
        hover: Vec<Ref>,
        scene: Source<'_>,
    ) {
        self.hover = hover;
        self.rebuild(device, queue, frame, scene);
    }

    pub(super) fn set_occluded(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::BindGroupLayout,
        occluded: bool,
        scene: Source<'_>,
    ) {
        if self.occluded == occluded {
            return;
        }
        self.occluded = occluded;
        self.rebuild(device, queue, frame, scene);
    }

    /// Rebuilds both cues against a scene that has just been replaced — see
    /// `renderer::rebuild`, where the parts they cover are new objects even
    /// where they are the same instances.
    pub(super) fn rebuild(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::BindGroupLayout,
        scene: Source<'_>,
    ) {
        let cues = self.cues();
        self.pass.replace(
            device,
            queue,
            frame,
            Source {
                highlights: &cues,
                ..scene
            },
        );
    }

    fn cues(&self) -> Vec<Highlight> {
        cue_highlights(&self.selection, &self.hover, self.occluded)
    }

    pub(super) fn sync_part(&mut self, device: &wgpu::Device, part: &Part) {
        self.pass.sync_part(device, part);
    }

    pub(super) fn remove_part(&mut self, id: PartId) {
        self.pass.remove_part(id);
    }

    pub(super) fn flush(&mut self, queue: &wgpu::Queue) {
        self.pass.flush(queue);
    }

    pub(super) fn sync_mesh(
        &mut self,
        device: &wgpu::Device,
        resolved: &Resolved,
        instance: &crate::scene::ResolvedInstance,
    ) -> bool {
        self.pass.sync_mesh(device, resolved, instance)
    }

    pub(super) fn remove_mesh(&mut self, referent: Ref) {
        self.pass.remove_mesh(referent);
    }

    pub(super) fn set_target(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        frame: &wgpu::BindGroupLayout,
    ) {
        self.pass.set_target(device, target, frame);
    }

    pub(super) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        frame: &wgpu::BindGroup,
        meshes: &Meshes,
        size: (u32, u32),
    ) {
        self.pass
            .draw(device, encoder, targets, frame, meshes, size);
    }
}

/// The two synthetic highlights the pass draws: the selection first, so a
/// part that is both selected and hovered shows the selection's own colour
/// (the editor never sends a hover for something already selected, but the
/// mask has to answer either way).
fn cue_highlights(selection: &[Ref], hover: &[Ref], occluded: bool) -> Vec<Highlight> {
    let depth_mode = if occluded {
        DepthMode::Occluded
    } else {
        DepthMode::AlwaysOnTop
    };
    [(selection, SELECTION, 1.0), (hover, HOVER, HOVER_ALPHA)]
        .into_iter()
        .filter(|(parts, _, _)| !parts.is_empty())
        .map(|(parts, color, alpha)| Highlight {
            parts: parts.to_vec(),
            // No interior: a cue says what is selected, it does not paint
            // over it.
            fill: color,
            fill_alpha: 0.0,
            outline: color,
            outline_alpha: alpha,
            depth_mode,
            referent: NO_INSTANCE,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rbx_reflection::ReflectionDatabase;

    use super::*;

    #[test]
    fn a_cue_outlines_without_filling() {
        let cues = cue_highlights(&[Ref::new(1)], &[], false);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].fill_alpha, 0.0);
        assert_eq!(cues[0].outline, SELECTION);
        assert_eq!(cues[0].outline_alpha, 1.0);
        assert_eq!(cues[0].depth_mode, DepthMode::AlwaysOnTop);
    }

    /// The same setting the container box carries: geometry in front of the
    /// selection hides it.
    #[test]
    fn the_occluded_setting_is_the_highlights_own_depth_mode() {
        let cues = cue_highlights(&[Ref::new(1)], &[], true);
        assert_eq!(cues[0].depth_mode, DepthMode::Occluded);
    }

    #[test]
    fn hover_is_its_own_dimmer_cue_after_the_selection() {
        let cues = cue_highlights(&[Ref::new(1)], &[Ref::new(2)], false);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].parts, vec![Ref::new(1)]);
        assert_eq!(cues[1].parts, vec![Ref::new(2)]);
        assert_eq!(cues[1].outline, HOVER);
        assert!(cues[1].outline_alpha < cues[0].outline_alpha);
    }

    #[test]
    fn nothing_selected_or_hovered_is_no_pass_at_all() {
        assert!(cue_highlights(&[], &[], false).is_empty());
    }

    /// A container has no silhouette to trace — it keeps the box
    /// `renderer::selection` draws — so only the parts of a selection reach
    /// the mask.
    #[test]
    fn only_parts_reach_the_silhouette() {
        let mut dom = rbx_dom::WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        let inside = dom.new_instance("Part", "Part", Some(model));
        let loose = dom.new_instance("Part", "Loose", None);
        let database = ReflectionDatabase::embedded();

        let selected = vec![
            Selected::read(&dom, &database, model),
            Selected::read(&dom, &database, loose),
        ];
        assert_eq!(Cues::parts_of(&selected), vec![loose]);
        assert!(!Cues::parts_of(&selected).contains(&inside));
    }
}
