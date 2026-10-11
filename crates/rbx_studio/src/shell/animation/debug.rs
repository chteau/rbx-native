//! `RBX_STUDIO_ANIMATION=<step>[,<step>...]`: opens the Animation Editor at
//! startup and runs each step as the matching click would, for scripted
//! screenshots and the end-to-end run (see `main.rs`). Steps, in order:
//! `build=r15|r6|r15-motor6d|r15-constraint` (insert the Rig Builder's stock
//! rig, with that joint class), `graph` (the Animation Graph sub-tab),
//! `skeleton=on|off` (the skeleton view),
//! `curves` (the Curve Editor, the clip converted, the current joint's
//! second Rotation X key selected), `rig=<name>` (bind the first
//! `Model` of that name; otherwise the selection's rig), `demo` (the example
//! walk cycle), `import=<path>` (an FBX/glTF), `import-id=<id>` (download a
//! published animation by AnimationId, asynchronously), `autorig=<path>` (auto-rig an
//! unrigged mesh, in the background; then `rigid`/`smooth`, `front=1|2` and
//! `publish` drive its panel, each waited for),
//! `autorig-drag=<joint>:<dx>/<dy>/<dz>` (drag that joint's marker by that
//! many studs, through the same press, move and release a 3D-view drag
//! sends, then wait for the new cut), `paste=<path>` (a file holding
//! the Blender addon's clipboard text), `time=<seconds>`, `joint=<name>`
//! (select that channel and its key at the playhead), `keyframe` (select
//! every key at the playhead), `save`, `graph-demo` (the example
//! locomotion graph), `speed=<n>` (set the graph's `speed`), `play`,
//! `node=<index>` (select a graph node), `save-graph`, `turn=<part>:<degrees>`
//! (rotate a rig part about its own X axis the way a Rotate-tool drag writes
//! it, then run one editor frame, which reads it back as a key),
//! `select=<part>` (select that part of the bound rig, as a click on it
//! would), `write=<path>` (File › Save As to that path) and `quit`.

use gpui_kit::*;
use rbx_animation::graph::Value;

use super::{Mode, Shell};
use crate::anim_editor::demo;
use crate::shell::chrome::Document;

pub(crate) const VARIABLE: &str = "RBX_STUDIO_ANIMATION";

fn find_model(dom: &rbx_dom::WeakDom, name: &str) -> Option<rbx_dom::Ref> {
    let mut stack: Vec<rbx_dom::Ref> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let instance = dom.get(r)?;
        if instance.class() == "Model" && instance.name() == name {
            return Some(r);
        }
        stack.extend(instance.children().iter().rev());
    }
    None
}

impl Shell {
    /// Picks the scripted run back up once the auto-rigger's rig is placed.
    pub(in crate::shell) fn resume_debug_steps(&mut self, cx: &mut Context<Self>) {
        if let Some(rest) = self.anim.debug_rest.take() {
            self.run_debug_steps(rest, cx);
        }
    }

    /// `autorig-drag=<joint>:<dx>/<dy>/<dz>`: reports whether a cut started,
    /// which the remaining steps then wait for.
    fn debug_autorig_drag(&mut self, arg: &str, cx: &mut Context<Self>) -> bool {
        use crate::workspace_view::JointPhase;
        let parsed = arg.split_once(':').and_then(|(name, by)| {
            let index = rbx_autorig::Joint::ALL
                .iter()
                .position(|j| j.name().eq_ignore_ascii_case(name))?;
            let by: Vec<f32> = by
                .split('/')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            let by = glam::Vec3::from_slice(by.get(..3)?);
            Some((index, by))
        });
        let Some((index, by)) = parsed else {
            self.anim.status = Some(format!("{VARIABLE}: bad autorig-drag={arg}").into());
            return false;
        };
        let Some(from) = self
            .anim_autorig()
            .and_then(|rig| rig.markers().0.get(index).copied())
        else {
            self.anim.status = Some(format!("{VARIABLE}: autorig-drag needs a rig").into());
            return false;
        };
        self.autorig_drag_joint(index, from, JointPhase::Press, cx);
        for step in 1..=4 {
            let to = from + by * (step as f32 / 4.0);
            self.autorig_drag_joint(index, to, JointPhase::Drag, cx);
        }
        self.autorig_drag_joint(index, from + by, JointPhase::Release, cx);
        self.anim_autorig().is_some_and(|rig| rig.busy)
    }

    fn debug_select_curve_key(&mut self) {
        use rbx_animation::curve::Channel;
        let e = &mut self.anim.editor;
        let Some(joint) = e.joint.clone() else { return };
        let first = e
            .curves
            .as_ref()
            .and_then(|c| c.joints.get(&joint))
            .and_then(|j| j.curve(Channel::RotationX).keys.get(1).map(|k| k.time));
        if let Some(time) = first {
            e.select_curve_key(
                crate::anim_editor::CurveKeyId {
                    joint,
                    channel: Channel::RotationX,
                    time,
                },
                false,
            );
        }
    }

    fn debug_turn(&mut self, arg: &str, cx: &mut Context<Self>) {
        let Some((part_name, degrees)) = arg.split_once(':') else {
            return;
        };
        let Ok(degrees) = degrees.parse::<f32>() else {
            return;
        };
        // A frame first, so the rig is posed and what follows is an edit.
        self.pose_rig(cx);
        let Some(part) = self
            .anim
            .bound
            .as_ref()
            .and_then(|b| b.rig.joint_for(part_name))
            .map(|j| j.part1)
        else {
            return;
        };
        let Some(rbx_dom::Variant::CFrame(now)) = self
            .dom
            .get(part)
            .and_then(|i| i.properties().get("CFrame"))
            .cloned()
        else {
            return;
        };
        let turned = rbx_animation::Frame::from_data(&now).mul(&rbx_animation::Frame::angles(
            degrees.to_radians(),
            0.0,
            0.0,
        ));
        let _ = self
            .dom
            .set_property(part, "CFrame", rbx_dom::Variant::CFrame(turned.to_data()));
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.pose_rig(cx);
    }

    pub(in crate::shell) fn apply_debug_animation(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(VARIABLE) else {
            return;
        };
        self.set_document(Document::Animation, cx);
        let steps = spec
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        self.run_debug_steps(steps, cx);
    }

    /// Runs `steps` in order. `autorig=` works in the background, so the
    /// steps after it wait until its rig is in the place (see
    /// [`Shell::resume_debug_steps`]).
    fn run_debug_steps(&mut self, steps: Vec<String>, cx: &mut Context<Self>) {
        for (i, step) in steps.iter().enumerate() {
            let (verb, arg) = step.split_once('=').unwrap_or((step.as_str(), ""));
            match verb {
                "graph" => self.anim.mode = Mode::Graph,
                "skeleton" => self.anim.skeleton = arg != "off",
                // The Curve Editor on the clip converted to curves, with the
                // current joint's second Rotation X key selected.
                "curves" => {
                    self.anim.mode = Mode::Curves;
                    self.anim.editor.convert_to_curves();
                    self.debug_select_curve_key();
                }
                "build" => {
                    use crate::shell::rig::{JointStyle, RigType};
                    let (rig_type, joints) = match arg {
                        "r6" => (RigType::R6, None),
                        "r15-motor6d" => (RigType::R15, Some(JointStyle::Motor6D)),
                        "r15-constraint" => (RigType::R15, Some(JointStyle::AnimationConstraint)),
                        _ => (RigType::R15, None),
                    };
                    self.insert_stock_rig(rig_type, joints, cx);
                    self.bind_selected_rig(cx);
                }
                "rig" => match find_model(&self.dom, arg) {
                    Some(model) => {
                        self.select(model, cx);
                        self.bind_rig(model, cx);
                    }
                    None => self.anim.status = Some(format!("No Model named {arg}").into()),
                },
                "demo" => {
                    if self.anim.bound.is_none() {
                        self.bind_selected_rig(cx);
                    }
                    let clip = demo::walk(self.anim.bound.as_ref().map(|b| &b.rig));
                    self.anim.editor.load(clip, None);
                    self.anim.editor.dirty = true;
                    self.anim.fitted = false;
                }
                "import" => self.import_animation_file(std::path::Path::new(arg), cx),
                "import-id" => self.import_animation_id(arg, cx),
                "autorig" => {
                    self.anim.debug_rest = Some(steps[i + 1..].to_vec());
                    self.start_autorig(std::path::PathBuf::from(arg), cx);
                    return;
                }
                "autorig-drag" => {
                    if self.debug_autorig_drag(arg, cx) {
                        self.anim.debug_rest = Some(steps[i + 1..].to_vec());
                        return;
                    }
                }
                // The auto-rigger's Rigid parts / Smooth bends toggle, then a
                // re-cut the following steps wait for.
                "rigid" | "smooth" => {
                    let smooth = verb == "smooth";
                    if let Some(rig) = self.anim_autorig_mut() {
                        rig.smooth = smooth;
                        self.anim.debug_rest = Some(steps[i + 1..].to_vec());
                        self.recut_autorig(cx);
                        return;
                    }
                }
                // Turn 90° (`front=1`) or Flip front (`front=2`).
                "front" => {
                    if self.anim_autorig().is_some() {
                        self.anim.debug_rest = Some(steps[i + 1..].to_vec());
                        self.autorig_turn(arg.parse().unwrap_or(1), cx);
                        return;
                    }
                }
                // The auto-rigger's Publish; the following steps wait for it.
                "publish" => {
                    if self.anim_autorig().is_some() {
                        self.anim.debug_rest = Some(steps[i + 1..].to_vec());
                        self.publish_autorig(cx);
                        return;
                    }
                }
                "paste" => match std::fs::read_to_string(arg) {
                    Ok(text) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        self.paste_blender_animation(cx);
                    }
                    Err(e) => self.anim.status = Some(format!("Couldn't read {arg}: {e}").into()),
                },
                "time" => {
                    if let Ok(t) = arg.parse::<f32>() {
                        self.anim.editor.set_time(t);
                    }
                }
                "joint" => {
                    let time = self.anim.editor.time;
                    self.focus_channel(arg, cx);
                    if self.anim.editor.clip.pose(time, arg).is_some() {
                        self.anim.editor.select((arg.to_owned(), time), false);
                    }
                }
                "keyframe" => {
                    let time = self.anim.editor.time;
                    self.anim.editor.select_keyframe(time, false);
                }
                "save" => self.save_animation(cx),
                "graph-demo" => {
                    let walk = self.anim.editor.clip.name.clone();
                    self.anim.graph.load(demo::locomotion("Idle", &walk), None);
                    self.anim.graph.dirty = true;
                }
                "speed" => {
                    if let Ok(n) = arg.parse::<f64>() {
                        if let Some(p) = self
                            .anim
                            .graph
                            .graph
                            .params
                            .iter_mut()
                            .find(|p| p.name == "speed")
                        {
                            p.default = Value::Number(n);
                        }
                    }
                }
                "play" => self.toggle_animation_playback(cx),
                "node" => self.anim.graph.selected = arg.parse().ok(),
                "save-graph" => self.anim_save_graph(cx),
                "turn" => self.debug_turn(arg, cx),
                "select" => {
                    let part = self
                        .anim
                        .bound
                        .as_ref()
                        .and_then(|b| b.rig.joint_for(arg))
                        .map(|j| j.part1);
                    match part {
                        Some(part) => self.reselect(vec![part], cx),
                        None => self.anim.status = Some(format!("No rig part named {arg}").into()),
                    }
                }
                "write" => self.save_as(std::path::Path::new(arg), cx),
                "quit" => cx.quit(),
                _ => self.anim.status = Some(format!("{VARIABLE}: unknown step {verb}").into()),
            }
        }
        cx.notify();
    }
}
