//! `RBX_STUDIO_ANIMATION=<step>[,<step>...]`: opens the Animation Editor at
//! startup and runs each step as the matching click would, for scripted
//! screenshots and the end-to-end run (see `main.rs`). Steps, in order:
//! `build=r15|r6|r15-motor6d|r15-constraint` (insert the Rig Builder's stock
//! rig, with that joint class), `graph` (the Animation Graph sub-tab), `rig=<name>` (bind the first
//! `Model` of that name; otherwise the selection's rig), `demo` (the example
//! walk cycle), `import=<path>` (an FBX/glTF), `paste=<path>` (a file holding
//! the Blender addon's clipboard text), `time=<seconds>`, `joint=<name>`
//! (select that channel and its key at the playhead), `keyframe` (select
//! every key at the playhead), `save`, `graph-demo` (the example
//! locomotion graph), `speed=<n>` (set the graph's `speed`), `play`,
//! `node=<index>` (select a graph node), `save-graph`, `write=<path>` (File ›
//! Save As to that path) and `quit`.

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
    pub(in crate::shell) fn apply_debug_animation(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(VARIABLE) else {
            return;
        };
        self.set_document(Document::Animation, cx);
        for step in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (verb, arg) = step.split_once('=').unwrap_or((step, ""));
            match verb {
                "graph" => self.anim.mode = Mode::Graph,
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
                "write" => self.save_as(std::path::Path::new(arg), cx),
                "quit" => cx.quit(),
                _ => self.anim.status = Some(format!("{VARIABLE}: unknown step {verb}").into()),
            }
        }
        cx.notify();
    }
}
