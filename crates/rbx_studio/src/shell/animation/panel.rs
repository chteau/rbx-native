//! The side panel beside the 3D view, in the place Blender's sidebar (and
//! the "Rbx Animations" addon's panel) sits: the rig, the clip's settings,
//! the selected joint's pose at the playhead, the selected keys' easing,
//! animation events, and the Blender import and export buttons.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::{Frame, Priority};

use super::widgets::{field_row, label, picker, publish_button, section, title, wide_button};
use super::Shell;
use crate::shell::chrome;
use crate::shell::menu::{self, MenuId};
use crate::shell::ribbon::RibbonCommand;
use crate::tokens;

mod autorig;
mod curves;
mod face;
mod ik;
mod sections;

pub(super) const WIDTH: f32 = 276.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Name,
    Rotation(usize),
    Position(usize),
    Marker,
    CurveValue,
    CurveTime,
    AutorigAxis(usize),
    Tangent { left: bool },
}

pub(in crate::shell) struct Fields {
    name: Entity<InputState>,
    rotation: [Entity<InputState>; 3],
    position: [Entity<InputState>; 3],
    marker: Entity<InputState>,
    /// The Curve section's value, time, and left and right tangents.
    curve: [Entity<InputState>; 4],
    /// The Auto-rig section's joint position.
    autorig: [Entity<InputState>; 3],
    /// The Face section's region (an index into `facs::REGIONS`) and one
    /// slider per channel, made the first time the channel shows.
    face_region: usize,
    face_sliders:
        std::collections::HashMap<String, Entity<gpui_kit::component::slider::SliderState>>,
    /// Set once an event is added from the field: it is emptied on the next
    /// render, which has the `Window` that takes.
    clear_marker: bool,
    _subscriptions: Vec<Subscription>,
}

fn number(value: f32) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        "0".into()
    } else {
        format!("{rounded}")
    }
}

/// Writes `seed` unless the field is being typed in or already shows it.
fn resync(input: &Entity<InputState>, seed: &str, window: &mut Window, cx: &mut App) {
    if input.focus_handle(cx).is_focused(window) || input.read(cx).value().as_ref() == seed {
        return;
    }
    input.update(cx, |state, cx| state.set_value(seed.to_owned(), window, cx));
}

impl Shell {
    fn ensure_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.anim.fields.is_some() {
            return;
        }
        let mut subscriptions = Vec::new();
        let mut make = |field: Field,
                        placeholder: &'static str,
                        window: &mut Window,
                        cx: &mut Context<Self>| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            subscriptions.push(cx.subscribe(
                &input,
                move |shell, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        let text = input.read(cx).value().to_string();
                        shell.commit_field(field, &text, cx);
                    }
                },
            ));
            input
        };
        let name = make(Field::Name, "Animation name", window, cx);
        let rotation = [0, 1, 2].map(|i| make(Field::Rotation(i), "0", window, cx));
        let position = [0, 1, 2].map(|i| make(Field::Position(i), "0", window, cx));
        let marker = make(Field::Marker, "Event name", window, cx);
        let curve = [
            make(Field::CurveValue, "Value", window, cx),
            make(Field::CurveTime, "Frame", window, cx),
            make(Field::Tangent { left: true }, "Auto", window, cx),
            make(Field::Tangent { left: false }, "Auto", window, cx),
        ];
        let autorig = [0, 1, 2].map(|i| make(Field::AutorigAxis(i), "0", window, cx));
        self.anim.fields = Some(Fields {
            name,
            rotation,
            position,
            marker,
            curve,
            autorig,
            face_region: 0,
            face_sliders: std::collections::HashMap::new(),
            clear_marker: false,
            _subscriptions: subscriptions,
        });
    }

    fn commit_field(&mut self, field: Field, text: &str, cx: &mut Context<Self>) {
        match field {
            Field::Name => {
                let name = text.trim().to_owned();
                if !name.is_empty() {
                    self.anim.editor.edit(|c| c.name = name);
                }
            }
            Field::Marker => {
                let name = text.trim();
                if !name.is_empty() {
                    self.anim.editor.add_marker(name, "");
                    if let Some(f) = &mut self.anim.fields {
                        f.clear_marker = true;
                    }
                }
            }
            Field::AutorigAxis(axis) => self.commit_autorig_axis(axis, text, cx),
            Field::CurveValue | Field::CurveTime | Field::Tangent { .. } => {
                self.commit_curve_field(field, text);
            }
            Field::Rotation(axis) | Field::Position(axis) => {
                let Ok(value) = text.trim().parse::<f32>() else {
                    cx.notify();
                    return;
                };
                let Some(joint) = self.anim.editor.joint.clone() else {
                    self.anim.warn("Pick a joint in the channel list first");
                    cx.notify();
                    return;
                };
                let now = self
                    .current_transforms()
                    .get(&joint)
                    .copied()
                    .unwrap_or_default();
                let next = if matches!(field, Field::Rotation(_)) {
                    let mut angles = now.to_angles();
                    angles[axis] = value.to_radians();
                    Frame::new(
                        now.position,
                        Frame::angles(angles.x, angles.y, angles.z).rotation,
                    )
                } else {
                    let mut position = now.position;
                    position[axis] = value;
                    Frame::new(position, now.rotation)
                };
                self.anim.editor.key(&joint, next);
            }
        }
        cx.notify();
    }

    pub(super) fn animation_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_fields(window, cx);
        let joint = self.anim.editor.joint.clone();
        let pose = joint
            .as_ref()
            .and_then(|j| self.current_transforms().get(j).copied())
            .unwrap_or_default();
        let fields = self.anim.fields.as_ref().map(|f| {
            (
                f.name.clone(),
                f.rotation.clone(),
                f.position.clone(),
                f.marker.clone(),
            )
        });
        let Some((name, rotation, position, marker)) = fields else {
            return div().into_any_element();
        };
        resync(&name, &self.anim.editor.clip.name, window, cx);
        if let Some(f) = &mut self.anim.fields {
            if std::mem::take(&mut f.clear_marker) {
                marker.update(cx, |state, cx| state.set_value("", window, cx));
            }
        }
        let angles = pose.to_angles();
        for (i, input) in rotation.iter().enumerate() {
            resync(input, &number(angles[i].to_degrees()), window, cx);
        }
        for (i, input) in position.iter().enumerate() {
            resync(input, &number(pose.position[i]), window, cx);
        }

        let rig = self.anim_rig_section(cx);
        let autorig_inputs = self.anim.fields.as_ref().map(|f| f.autorig.clone());
        let autorig =
            autorig_inputs.and_then(|inputs| self.anim_autorig_section(&inputs, window, cx));
        let clip = self.anim_clip_section(&name, cx);
        let key = self.anim_key_section(joint, &rotation, &position, cx);
        let events = self.anim_events_section(&marker, cx);
        let import = self.anim_import_section(cx);
        let curve =
            (self.anim.mode == super::Mode::Curves).then(|| self.anim_curve_section(window, cx));
        let face = self.anim_face_section(window, cx);
        let ik = self.anim_ik_section(cx);
        v_flex()
            .id("anim-panel")
            .flex_none()
            .w(px(WIDTH))
            .h_full()
            .overflow_y_scroll()
            .p(px(8.))
            .gap(px(10.))
            .bg(tokens::dock())
            .border_l(px(1.))
            .border_color(tokens::border())
            .child(title("Animation"))
            // A mesh being rigged comes first: the stage leaves the panel
            // little height, and the auto-rigger's controls are what the
            // panel is open for.
            .children(autorig)
            .child(rig)
            .child(clip)
            .children(curve)
            .child(key)
            .child(ik)
            .child(events)
            .child(face)
            .child(import)
            .into_any_element()
    }

    fn anim_rig_section(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = match &self.anim.bound {
            Some(b) => {
                let name = self
                    .dom
                    .get(b.model)
                    .map(|i| i.name().to_owned())
                    .unwrap_or_default();
                format!("{name} · {}", super::rig_summary(&b.rig))
            }
            None => "No rig: select a character in the Explorer or the 3D view".into(),
        };
        // The Auto-rig section carries its own Publish while it is open.
        let publishable = self.anim_autorig().is_none() && self.publishable_rig().is_some();
        let joints = crate::shell::rig::JointStyle::of_place(&self.dom);
        let r15_label = match joints {
            crate::shell::rig::JointStyle::AnimationConstraint => "R15 · AnimationConstraint",
            crate::shell::rig::JointStyle::Motor6D => "R15 · Motor6D",
        };
        section("Rigging")
            .child(label(summary))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        wide_button("anim-insert-r15", "Insert R15", true)
                            .tooltip(move |window, cx| {
                                crate::shell::tooltip::text(r15_label, window, cx)
                            })
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.insert_stock_rig(crate::shell::rig::RigType::R15, None, cx);
                            })),
                    )
                    .child(
                        wide_button("anim-insert-r6", "Insert R6", true).on_click(cx.listener(
                            |shell, _, _, cx| {
                                shell.insert_stock_rig(crate::shell::rig::RigType::R6, None, cx);
                            },
                        )),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        wide_button("anim-use-rig", "Use selected rig", true).on_click(
                            cx.listener(|shell, _, _, cx| {
                                if !shell.bind_selected_rig(cx) {
                                    shell.anim.warn("The selection is not inside a rig");
                                }
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        wide_button("anim-rig-builder", "Rig Builder…", true).on_click(
                            cx.listener(|shell, _, _, cx| RibbonCommand::RigBuilder.run(shell, cx)),
                        ),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        wide_button("anim-autorig", "Auto-rig a mesh…", true)
                            .tooltip(|window, cx| {
                                crate::shell::tooltip::text(
                                    "Fit an R15 skeleton to an unrigged character (FBX, OBJ or glTF) and cut it into its 15 body parts",
                                    window,
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|shell, _, _, cx| shell.choose_autorig(cx))),
                    )
                    // An auto-rigged body still on local meshes publishes
                    // from here once the auto-rigger is done with it.
                    .when(publishable, |row| {
                        row.child(
                            publish_button("anim-publish-rig", self.anim.publishing)
                                .tooltip(|window, cx| {
                                    crate::shell::tooltip::text(
                                        "Upload this rig's body parts to Roblox and point the rig at them",
                                        window,
                                        cx,
                                    )
                                })
                                .on_click(cx.listener(|shell, _, _, cx| shell.publish_autorig(cx))),
                        )
                    }),
            )
    }

    fn anim_clip_section(
        &mut self,
        name: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let e = &self.anim.editor;
        let (looped, priority, fps) = (e.clip.looped, e.clip.priority, e.fps);
        let frames = (e.clip.length() * fps as f32).round() as i64;
        let length = format!(
            "{frames} frames · {:.2} s{}",
            e.clip.length(),
            if e.dirty { " · unsaved" } else { "" }
        );
        let priorities = Priority::ALL
            .into_iter()
            .map(|p| {
                menu::item(p.label())
                    .checked(p == priority)
                    .on_click(move |shell, cx| {
                        shell.anim.editor.edit(|c| c.priority = p);
                        cx.notify();
                    })
            })
            .collect();
        let rates = [24, 30, 60]
            .into_iter()
            .map(|r| {
                menu::item(format!("{r} fps"))
                    .checked(r == fps)
                    .on_click(move |shell, cx| {
                        shell.anim.editor.fps = r;
                        shell.anim.timeline.fps = r;
                        cx.notify();
                    })
            })
            .collect();
        section("Clip")
            .child(field_row("Name", name))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        chrome::button(
                            "anim-loop",
                            if looped { "Looping" } else { "Not looping" },
                            looped,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.anim.editor.edit(|c| c.looped = !c.looped);
                            cx.notify();
                        })),
                    )
                    .child(picker(
                        self,
                        MenuId::AnimPriority,
                        "anim-priority",
                        priority.label(),
                        priorities,
                        cx,
                    ))
                    .child(picker(
                        self,
                        MenuId::AnimFps,
                        "anim-fps",
                        &format!("{fps} fps"),
                        rates,
                        cx,
                    )),
            )
            .child(label(length))
            .child(
                h_flex()
                    .gap(px(6.))
                    // Opening, saving and publishing live in the top bar
                    // (Import ▾ / Export ▾ / Publish); only a fresh clip
                    // stays here, beside the clip's own settings.
                    .child(
                        wide_button("anim-new", "New clip", true)
                            .on_click(cx.listener(|shell, _, _, cx| shell.new_animation(cx))),
                    ),
            )
    }
}
