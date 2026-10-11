//! The Animation Editor document: a rig in the 3D view, a dope sheet of its
//! joints' keys under it and a side panel beside it (the layout of Blender's
//! Animation workspace), plus an Animation Graph canvas on its second
//! sub-tab.
//!
//! The clip itself lives in `anim_editor::AnimEditor`, a working copy saved
//! to a `KeyframeSequence` on request. The rig is posed by writing its
//! parts' `CFrame`s straight to the DOM every frame. Those writes are kept
//! out of the undo history, and the rest pose is put back before a save and
//! whenever the document is left, so the place never keeps a pose by
//! accident. Pose edits made in the viewport (a part rotated with the
//! Rotate tool, say) are read back as a key on that part's joint at the
//! playhead. That is how Roblox's editor turns a dragged limb into a pose.

mod curves;
mod debug;
mod graph;
mod import;
mod panel;
mod pose;
mod sheet;
mod skeleton;
mod stage;
mod view;
mod widgets;

#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::*;
use rbx_animation::{sequence, Frame, Rig};
use rbx_dom::{CFrameData, Ref};

use super::Shell;
use crate::anim_editor::{place, AnimEditor};
use crate::timeline::Timeline;

pub(super) use graph::GraphDoc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    DopeSheet,
    Curves,
    Graph,
}

impl Mode {
    pub(super) const ALL: [Mode; 3] = [Mode::DopeSheet, Mode::Curves, Mode::Graph];

    fn key(self) -> &'static str {
        match self {
            Mode::DopeSheet => "anim-dope-sheet",
            Mode::Curves => "anim-curves",
            Mode::Graph => "anim-graph",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Mode::DopeSheet => "Dope Sheet",
            Mode::Curves => "Curve Editor",
            Mode::Graph => "Animation Graph",
        }
    }
}

/// The rig the editor is posing, and what it needs to put it back.
pub(super) struct Bound {
    pub(super) model: Ref,
    pub(super) rig: Rig,
    /// Where the root part stands; joints are laid out from it.
    pub(super) root: Frame,
    /// Each rig part's `CFrame` before the editor first posed it.
    pub(super) rest: HashMap<Ref, CFrameData>,
    /// What the editor last wrote, to tell its own writes from an edit.
    pub(super) posed: HashMap<Ref, CFrameData>,
    /// Each part whose `PivotOffset` the editor moved onto its joint, and
    /// what it was (`None`: it had none).
    pub(super) pivots: HashMap<Ref, Option<rbx_dom::Variant>>,
}

/// A pointer gesture on the dope sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Gesture {
    Scrub,
    MoveKeys {
        from_x: f32,
    },
    Pan {
        last_x: f32,
    },
    /// Curve keys dragged in time and value from where the press landed.
    CurveKeys {
        from: (f32, f32),
    },
    /// The selected curve key's left or right tangent handle.
    Tangent {
        left: bool,
    },
}

/// Manage IK: whether a limb dragged with the Move tool is solved as a
/// chain, how much of the body the chain takes, and the parts held still.
#[derive(Default)]
pub(super) struct IkSettings {
    pub(super) enabled: bool,
    pub(super) mode: rbx_animation::ik::Mode,
    pub(super) pinned: std::collections::HashSet<Ref>,
}

pub(super) struct Animation {
    pub(super) editor: AnimEditor,
    pub(super) timeline: Timeline,
    pub(super) mode: Mode,
    pub(super) bound: Option<Bound>,
    pub(super) focus: FocusHandle,
    /// The dope sheet's key area as last laid out.
    pub(super) sheet: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(super) gesture: Option<Gesture>,
    pub(super) last_frame: Option<Instant>,
    pub(super) fields: Option<panel::Fields>,
    /// The last import, save or error, shown under the panel's buttons.
    pub(super) status: Option<SharedString>,
    pub(super) graph: GraphDoc,
    /// Whether the timeline has been fitted to the clip since it loaded.
    pub(super) fitted: bool,
    pub(super) ik: IkSettings,
    pub(super) curve: curves::CurveView,
    /// A mesh being auto-rigged, while the auto-rigger is open.
    pub(super) autorig: Option<crate::shell::rig::AutoRig>,
    /// A scripted run's steps waiting for the auto-rigger (see `debug`).
    debug_rest: Option<Vec<String>>,
    /// The rig the 3D view is staged around, once the grid is sent.
    staged: Option<Ref>,
    /// Whether the rig's bones are drawn over it (Blender shows them in
    /// its animation workspace, so on by default), and what was last sent.
    pub(super) skeleton: bool,
    skeleton_sent: Vec<rbx_viewer::Segment>,
    /// What the local draft last held, and when it was written.
    drafted: Option<(rbx_animation::Clip, rbx_animation::graph::Graph)>,
    drafted_at: Option<Instant>,
}

impl Animation {
    pub(super) fn new(cx: &mut App) -> Animation {
        Animation {
            editor: AnimEditor::default(),
            timeline: Timeline::default(),
            mode: Mode::default(),
            bound: None,
            focus: cx.focus_handle(),
            sheet: Rc::new(Cell::new(None)),
            gesture: None,
            last_frame: None,
            fields: None,
            status: None,
            graph: GraphDoc::default(),
            fitted: false,
            ik: IkSettings::default(),
            curve: curves::CurveView::default(),
            autorig: None,
            debug_rest: None,
            staged: None,
            skeleton: true,
            skeleton_sent: Vec::new(),
            drafted: None,
            drafted_at: None,
        }
    }
}

/// Unsaved work is written to its draft at most this often while it changes.
const DRAFT_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

impl Shell {
    /// Leaves the Animation document: the rig goes back to its rest pose and
    /// playback stops.
    pub(super) fn leave_animation(&mut self, cx: &mut Context<Self>) {
        self.anim.editor.playing = false;
        self.anim.graph.playing = false;
        self.keep_draft(true);
        self.unpose_rig(cx);
        self.unstage(cx);
    }

    /// Stages the bound rig in the 3D view: alone, on a floor grid at its
    /// feet. Sent once per rig, as the document shows.
    pub(super) fn stage_rig(&mut self, cx: &mut Context<Self>) {
        let Some(bound) = &self.anim.bound else {
            self.unstage(cx);
            return;
        };
        if self.anim.staged == Some(bound.model) {
            return;
        }
        let size_of = |part: Ref| match self.dom.get(part)?.properties().get("Size")? {
            rbx_dom::Variant::Vector3(v) => Some(glam::Vec3::new(v.x, v.y, v.z)),
            _ => None,
        };
        let boxes = bound.rest.iter().filter_map(|(&part, c)| {
            let p = glam::Vec3::new(c.position.x, c.position.y, c.position.z);
            Some((p, size_of(part)?))
        });
        let floor = stage::floor_of(boxes).unwrap_or(bound.root.position.y - 3.0);
        let grid = stage::grid(bound.root.position, floor);
        let model = bound.model;
        self.viewport.update(cx, |viewport, _| {
            viewport.set_stage_grid(grid);
            viewport.set_isolation(Some(vec![model]));
        });
        self.anim.staged = Some(model);
    }

    /// Sends the skeleton view for the rig as `world` lays it out (nothing
    /// when it is hidden), only when it changed since the last send.
    pub(super) fn sync_skeleton(&mut self, world: &HashMap<Ref, Frame>, cx: &mut Context<Self>) {
        let segments = match (&self.anim.bound, self.anim.skeleton) {
            (Some(bound), true) => skeleton::segments(&skeleton::bones(
                &bound.rig,
                world,
                self.anim.editor.joint.as_deref(),
            )),
            _ => Vec::new(),
        };
        if segments == self.anim.skeleton_sent {
            return;
        }
        self.anim.skeleton_sent = segments.clone();
        self.viewport
            .update(cx, |viewport, _| viewport.set_skeleton(segments));
    }

    pub(in crate::shell) fn anim_autorig(&self) -> Option<&crate::shell::rig::AutoRig> {
        self.anim.autorig.as_ref()
    }

    pub(in crate::shell) fn anim_autorig_mut(&mut self) -> Option<&mut crate::shell::rig::AutoRig> {
        self.anim.autorig.as_mut()
    }

    pub(in crate::shell) fn anim_set_autorig(&mut self, rig: Option<crate::shell::rig::AutoRig>) {
        self.anim.autorig = rig;
    }

    /// Lets go of `model` before it is replaced: put back at rest, no
    /// longer bound or staged.
    pub(in crate::shell) fn unbind_rig_for_replace(&mut self, model: Ref, cx: &mut Context<Self>) {
        if self.anim.bound.as_ref().is_some_and(|b| b.model == model) {
            self.unpose_rig(cx);
            self.anim.bound = None;
        }
        self.unstage(cx);
    }

    /// Gives the 3D view back to the whole place.
    pub(super) fn unstage(&mut self, cx: &mut Context<Self>) {
        if self.anim.staged.take().is_some() {
            self.anim.skeleton_sent.clear();
            self.viewport.update(cx, |viewport, _| {
                viewport.set_stage_grid(Vec::new());
                viewport.set_skeleton(Vec::new());
                viewport.set_isolation(None);
            });
        }
    }

    fn draft_file(&self) -> Option<std::path::PathBuf> {
        crate::anim_editor::draft::folder().map(|f| crate::anim_editor::draft::path(&f, &self.path))
    }

    /// Writes unsaved work to the local draft (now, or once the last write is
    /// old enough), and deletes the draft once nothing is left unsaved.
    pub(super) fn keep_draft(&mut self, now: bool) {
        use crate::anim_editor::draft;
        let Some(file) = self.draft_file() else {
            return;
        };
        let (clip_dirty, graph_dirty) = (self.anim.editor.dirty, self.anim.graph.dirty);
        if !clip_dirty && !graph_dirty {
            if self.anim.drafted.take().is_some() {
                draft::clear(&file);
            }
            return;
        }
        let content = (self.anim.editor.clip.clone(), self.anim.graph.graph.clone());
        if self.anim.drafted.as_ref() == Some(&content) {
            return;
        }
        if !now
            && self
                .anim
                .drafted_at
                .is_some_and(|t| t.elapsed() < DRAFT_EVERY)
        {
            return;
        }
        let rig = self
            .anim
            .bound
            .as_ref()
            .and_then(|b| self.dom.get(b.model))
            .map(|i| i.name().to_owned())
            .unwrap_or_default();
        let saved = draft::write(
            &file,
            &draft::Draft {
                rig,
                clip: clip_dirty.then(|| content.0.clone()),
                graph: graph_dirty.then(|| content.1.clone()),
            },
        );
        if saved.is_ok() {
            self.anim.drafted = Some(content);
            self.anim.drafted_at = Some(Instant::now());
        }
    }

    /// Brings back the draft left by an earlier session on this rig.
    fn restore_draft(&mut self, model: Ref) {
        use crate::anim_editor::draft;
        if self.anim.editor.dirty || self.anim.graph.dirty {
            return;
        }
        let Some(file) = self.draft_file() else {
            return;
        };
        let Some(found) = draft::read(&file) else {
            return;
        };
        let name = self
            .dom
            .get(model)
            .map(|i| i.name().to_owned())
            .unwrap_or_default();
        if found.rig != name {
            return;
        }
        let mut restored = Vec::new();
        if let Some(clip) = found.clip {
            restored.push(clip.name.clone());
            self.anim.editor.load(clip, None);
            self.anim.editor.dirty = true;
            self.anim.fitted = false;
        }
        if let Some(graph) = found.graph {
            restored.push(graph.name.clone());
            self.anim.graph.load(graph, None);
            self.anim.graph.dirty = true;
        }
        if !restored.is_empty() {
            self.anim.status = Some(
                format!(
                    "Restored unsaved {} from the last session",
                    restored.join(" and ")
                )
                .into(),
            );
        }
    }

    /// Binds the rig the selection is in, if it is a different one.
    pub(super) fn bind_selected_rig(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(model) = self
            .selected_all()
            .first()
            .and_then(|&r| place::rig_of(&self.dom, r))
        else {
            return false;
        };
        if self.anim.bound.as_ref().is_some_and(|b| b.model == model) {
            return true;
        }
        self.bind_rig(model, cx);
        true
    }

    pub(super) fn bind_rig(&mut self, model: Ref, cx: &mut Context<Self>) {
        self.bind_rig_framed(model, true, cx);
    }

    /// [`Shell::bind_rig`], leaving the camera where it is when `frame` is
    /// false: the auto-rigger rebuilding the rig it shows after an edit.
    pub(super) fn bind_rig_framed(&mut self, model: Ref, frame: bool, cx: &mut Context<Self>) {
        self.unpose_rig(cx);
        let rig = Rig::read(&self.dom, model);
        let parts = rig
            .root
            .into_iter()
            .chain(rig.joints.iter().map(|j| j.part1));
        let rest: HashMap<Ref, CFrameData> = parts
            .filter_map(|p| match self.dom.get(p)?.properties().get("CFrame")? {
                rbx_dom::Variant::CFrame(c) => Some((p, *c)),
                _ => None,
            })
            .collect();
        let root = rig
            .root
            .and_then(|r| rest.get(&r))
            .map(Frame::from_data)
            .unwrap_or_default();
        let name = self
            .dom
            .get(model)
            .map(|i| i.name().to_owned())
            .unwrap_or_default();
        let kinds = rig_summary(&rig);
        self.anim.status = Some(format!("Rig {name}: {kinds}").into());
        if frame {
            self.frame_rig(&rig, root, cx);
        }
        self.anim.ik.pinned.clear();
        self.anim.bound = Some(Bound {
            model,
            rig,
            root,
            rest,
            posed: HashMap::new(),
            pivots: HashMap::new(),
        });
        self.restore_draft(model);
        if self.anim.editor.clip.keyframes.is_empty() && self.anim.editor.source.is_none() {
            if let Some((seq, _)) = place::all_of_class(&self.dom, "KeyframeSequence", Some(model))
                .into_iter()
                .next()
            {
                self.load_animation(seq);
            }
        }
        cx.notify();
    }

    pub(super) fn load_animation(&mut self, sequence_ref: Ref) {
        let clip = sequence::read(&self.dom, sequence_ref);
        self.anim.editor.load(clip, Some(sequence_ref));
        self.anim.fitted = false;
    }

    pub(super) fn new_animation(&mut self, cx: &mut Context<Self>) {
        self.anim.editor.load(rbx_animation::Clip::default(), None);
        self.anim.fitted = false;
        self.anim.status = Some("New animation".into());
        cx.notify();
    }

    /// Writes the clip to its `KeyframeSequence` (made in the rig's save
    /// folder the first time) as one undoable edit of the place.
    pub(super) fn save_animation(&mut self, cx: &mut Context<Self>) {
        self.unpose_rig(cx);
        self.push_history();
        let rig = self.anim.bound.as_ref().map(|b| (b.model, b.rig.clone()));
        let clip = self.anim.editor.clip.clone();
        let existing = self.anim.editor.source.filter(|&s| {
            self.dom
                .get(s)
                .is_some_and(|i| i.class() == "KeyframeSequence")
        });
        let target = match existing {
            Some(s) => {
                sequence::overwrite(&mut self.dom, s, &clip, rig.as_ref().map(|r| &r.1));
                s
            }
            None => {
                let folder = rig
                    .as_ref()
                    .map(|(m, _)| place::save_folder(&mut self.dom, *m));
                sequence::write(&mut self.dom, folder, &clip, rig.as_ref().map(|r| &r.1))
            }
        };
        self.anim.editor.source = Some(target);
        self.anim.editor.dirty = false;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.rebuild_explorer(cx);
        self.keep_draft(true);
        self.anim.status = Some(format!("Saved {} as a KeyframeSequence", clip.name).into());
        cx.notify();
    }

    /// Ctrl+Z while the editor is up undoes the editor's own last edit
    /// first; a clip edit never reached the place's history.
    pub(super) fn undo_animation(&mut self, cx: &mut Context<Self>) -> bool {
        let up = self.document == super::chrome::Document::Animation;
        let done = up && self.anim.mode != Mode::Graph && self.anim.editor.undo();
        if done {
            cx.notify();
        }
        done
    }

    pub(super) fn redo_animation(&mut self, cx: &mut Context<Self>) -> bool {
        let up = self.document == super::chrome::Document::Animation;
        let done = up && self.anim.mode != Mode::Graph && self.anim.editor.redo();
        if done {
            cx.notify();
        }
        done
    }
}

impl Shell {
    /// Points the camera at the rig from its front-left, far enough back to
    /// show it whole: what Roblox's editor does when a rig is picked.
    fn frame_rig(&self, rig: &Rig, root: Frame, cx: &mut Context<Self>) {
        let parts = rig.layout(root, &|_| None);
        let (mut low, mut high) = (root.position, root.position);
        for frame in parts.values() {
            low = low.min(frame.position);
            high = high.max(frame.position);
        }
        let centre = (low + high) * 0.5;
        let reach = ((high - low).length() * 0.5).max(2.5);
        let front = root.rotation * glam::Vec3::new(-0.55, 0.25, -1.0).normalize();
        let eye = centre + front * reach * 3.2;
        self.viewport
            .read(cx)
            .look_at(eye.to_array(), centre.to_array());
    }
}

/// "15 joints (AnimationConstraint)" or "12 joints (Motor6D)", or both
/// counts for a rig that mixes them.
pub(super) fn rig_summary(rig: &Rig) -> String {
    use rbx_animation::JointKind;
    let count = |k: JointKind| rig.joints.iter().filter(|j| j.kind == k).count();
    let (motors, constraints) = (
        count(JointKind::Motor6D),
        count(JointKind::AnimationConstraint),
    );
    let total = motors + constraints;
    match (motors, constraints) {
        (_, 0) => format!("{total} joints (Motor6D)"),
        (0, _) => format!("{total} joints (AnimationConstraint)"),
        _ => format!("{total} joints ({motors} Motor6D, {constraints} AnimationConstraint)"),
    }
}
