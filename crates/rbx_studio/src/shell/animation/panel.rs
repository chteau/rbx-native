//! The side panel beside the 3D view, in the place Blender's sidebar (and
//! the "Rbx Animations" addon's panel) sits: the rig, the clip's settings,
//! the selected joint's pose at the playhead, the selected keys' easing,
//! animation events, and the Blender import and export buttons.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::{Direction, Frame, Priority, Style};

use super::Shell;
use crate::shell::chrome;
use crate::shell::menu::{self, MenuId};
use crate::shell::ribbon::RibbonCommand;
use crate::tokens;

pub(super) const WIDTH: f32 = 276.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Name,
    Rotation(usize),
    Position(usize),
    Marker,
    AnimationId,
}

pub(in crate::shell) struct Fields {
    name: Entity<InputState>,
    rotation: [Entity<InputState>; 3],
    position: [Entity<InputState>; 3],
    marker: Entity<InputState>,
    animation_id: Entity<InputState>,
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
        let animation_id = make(Field::AnimationId, "rbxassetid://…", window, cx);
        self.anim.fields = Some(Fields {
            name,
            rotation,
            position,
            marker,
            animation_id,
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
            Field::AnimationId => {
                if !text.trim().is_empty() {
                    self.import_animation_id(text, cx);
                }
            }
            Field::Rotation(axis) | Field::Position(axis) => {
                let Ok(value) = text.trim().parse::<f32>() else {
                    cx.notify();
                    return;
                };
                let Some(joint) = self.anim.editor.joint.clone() else {
                    self.anim.status = Some("Pick a joint in the channel list first".into());
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
        let clip = self.anim_clip_section(&name, cx);
        let key = self.anim_key_section(joint, &rotation, &position, cx);
        let events = self.anim_events_section(&marker, cx);
        let import = self.anim_import_section(cx);
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
            .child(rig)
            .child(clip)
            .child(key)
            .child(events)
            .child(import)
            .when_some(self.anim.status.clone(), |this, status| {
                this.child(
                    div()
                        .id("anim-status")
                        .debug_selector(|| "anim-status".into())
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text_muted())
                        .child(status),
                )
            })
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
        section("Rigging").child(label(summary)).child(
            h_flex()
                .gap(px(6.))
                .child(
                    wide_button("anim-use-rig", "Use selected rig", true).on_click(cx.listener(
                        |shell, _, _, cx| {
                            if !shell.bind_selected_rig(cx) {
                                shell.anim.status =
                                    Some("The selection is not inside a rig".into());
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(
                    wide_button("anim-rig-builder", "Rig Builder", true).on_click(
                        cx.listener(|shell, _, _, cx| RibbonCommand::RigBuilder.run(shell, cx)),
                    ),
                ),
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
        let saved = crate::anim_editor::place::all_of_class(
            &self.dom,
            "KeyframeSequence",
            self.anim.bound.as_ref().map(|b| b.model),
        );
        let loads: Vec<menu::Item> = if saved.is_empty() {
            vec![menu::item("No KeyframeSequence in this place").disabled()]
        } else {
            saved
                .into_iter()
                .map(|(r, n)| {
                    menu::item(n).on_click(move |shell, cx| {
                        shell.load_animation(r);
                        cx.notify();
                    })
                })
                .collect()
        };
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
                    .child(
                        wide_button("anim-new", "New", true)
                            .on_click(cx.listener(|shell, _, _, cx| shell.new_animation(cx))),
                    )
                    .child(picker(
                        self,
                        MenuId::AnimLoad,
                        "anim-load",
                        "Load",
                        loads,
                        cx,
                    ))
                    .child(
                        primary_button("anim-save", "Save")
                            .on_click(cx.listener(|shell, _, _, cx| shell.save_animation(cx))),
                    ),
            )
    }

    fn anim_key_section(
        &mut self,
        joint: Option<String>,
        rotation: &[Entity<InputState>; 3],
        position: &[Entity<InputState>; 3],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let easing = self.anim.editor.easing();
        let any = !self.anim.editor.selected.is_empty();
        let styles = Style::PICKABLE
            .into_iter()
            .map(|s| {
                menu::item(s.label())
                    .checked(easing.is_some_and(|e| e.0 == s))
                    .on_click(move |shell, cx| {
                        shell.anim.editor.set_easing(Some(s), None);
                        cx.notify();
                    })
            })
            .collect();
        let directions = Direction::ALL
            .into_iter()
            .map(|d| {
                menu::item(d.label())
                    .checked(easing.is_some_and(|e| e.1 == d))
                    .on_click(move |shell, cx| {
                        shell.anim.editor.set_easing(None, Some(d));
                        cx.notify();
                    })
            })
            .collect();
        let style_label = easing.map_or(if any { "Mixed" } else { "Easing" }, |e| e.0.label());
        let direction_label = easing.map_or("Direction", |e| e.1.label());
        let has_joint = joint.is_some();
        let joint_label = joint.unwrap_or_else(|| "No joint selected".into());
        section("Keyframe")
            .child(label(format!(
                "Joint: {joint_label} · frame {}",
                self.anim.editor.frame()
            )))
            .child(xyz_row("Rotation °", rotation))
            .child(xyz_row("Position", position))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        wide_button("anim-key-joint", "Key joint (I)", has_joint).on_click(
                            cx.listener(|shell, _, _, cx| {
                                let joint = shell.anim.editor.joint.clone();
                                if joint.is_some() {
                                    shell.key_joints(joint.as_deref(), cx);
                                }
                            }),
                        ),
                    )
                    .child(
                        wide_button("anim-key-all", "Key all", true)
                            .on_click(cx.listener(|shell, _, _, cx| shell.key_joints(None, cx))),
                    )
                    .child(
                        wide_button("anim-reset-joint", "Reset", has_joint).on_click(cx.listener(
                            |shell, _, _, cx| {
                                if let Some(j) = shell.anim.editor.joint.clone() {
                                    shell.anim.editor.key(&j, Frame::IDENTITY);
                                    cx.notify();
                                }
                            },
                        )),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .when(any, |this| {
                        this.child(picker(
                            self,
                            MenuId::AnimEasingStyle,
                            "anim-ease-style",
                            style_label,
                            styles,
                            cx,
                        ))
                        .child(picker(
                            self,
                            MenuId::AnimEasingDirection,
                            "anim-ease-direction",
                            direction_label,
                            directions,
                            cx,
                        ))
                    })
                    .when(!any, |this| {
                        this.child(label("Select keys to set their easing".into()))
                    }),
            )
    }

    fn anim_events_section(
        &mut self,
        marker: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let time = self.anim.editor.snap(self.anim.editor.time);
        let here: Vec<String> = self
            .anim
            .editor
            .clip
            .keyframes
            .iter()
            .filter(|k| (k.time - time).abs() < 1e-4)
            .flat_map(|k| k.markers.iter().map(|m| m.name.clone()))
            .collect();
        let listed = if here.is_empty() {
            "No event at the playhead".to_owned()
        } else {
            here.join(", ")
        };
        section("Events")
            .child(label(listed))
            .child(field_row("Add", marker))
            .child(
                wide_button("anim-remove-events", "Remove events here", !here.is_empty()).on_click(
                    cx.listener(|shell, _, _, cx| {
                        shell.anim.editor.remove_markers_here();
                        cx.notify();
                    }),
                ),
            )
    }

    fn anim_import_section(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.anim.fields.as_ref().map(|f| f.animation_id.clone());
        section("Animation import")
            .children(id.map(|id| {
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(div().flex_1().child(field_row("Id", &id)))
                    .child(
                        wide_button("anim-import-id", "Fetch", true)
                            .flex_none()
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                let text = id.read(cx).value().to_string();
                                shell.import_animation_id(&text, cx);
                            })),
                    )
            }))
            .child(
                wide_button("anim-import-file", "Import FBX / glTF…", true)
                    .on_click(cx.listener(|shell, _, _, cx| shell.choose_animation_file(cx))),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        wide_button("anim-paste-blender", "Paste from Blender", true).on_click(
                            cx.listener(|shell, _, _, cx| shell.paste_blender_animation(cx)),
                        ),
                    )
                    .child(
                        wide_button("anim-copy-blender", "Copy for Blender", true).on_click(
                            cx.listener(|shell, _, _, cx| shell.copy_blender_animation(cx)),
                        ),
                    ),
            )
    }
}

fn title(text: &'static str) -> impl IntoElement {
    div()
        .text_size(tokens::text_sm())
        .text_color(tokens::text_strong())
        .child(text)
}

pub(super) fn section(name: &'static str) -> Div {
    v_flex().gap(px(6.)).child(
        div()
            .text_size(tokens::text_xs())
            .text_color(tokens::text_muted())
            .child(name),
    )
}

pub(super) fn label(text: String) -> impl IntoElement {
    div()
        .text_size(tokens::text_sm())
        .text_color(tokens::text_label())
        .child(text)
}

pub(super) fn field_row(name: &'static str, input: &Entity<InputState>) -> impl IntoElement {
    h_flex()
        .gap(tokens::label_gap())
        .items_center()
        .child(
            div()
                .w(px(44.))
                .flex_none()
                .text_size(tokens::text_sm())
                .text_color(tokens::text_label())
                .child(name),
        )
        .child(field(input).flex_1())
}

/// [`field_row`] for a label known only at run time (a node property).
pub(super) fn field_row_owned(name: &str, input: &Entity<InputState>) -> impl IntoElement {
    h_flex()
        .gap(tokens::label_gap())
        .items_center()
        .child(
            div()
                .w(px(72.))
                .flex_none()
                .truncate()
                .text_size(tokens::text_sm())
                .text_color(tokens::text_label())
                .child(name.to_owned()),
        )
        .child(field(input).flex_1())
}

fn field(input: &Entity<InputState>) -> Div {
    div()
        .h(tokens::input_height())
        .px(tokens::input_padding())
        .bg(tokens::field_select())
        .rounded(px(4.))
        .flex()
        .items_center()
        .child(
            Input::new(input)
                .appearance(false)
                .with_size(tokens::field_size())
                .h_full(),
        )
}

fn xyz_row(name: &'static str, inputs: &[Entity<InputState>; 3]) -> impl IntoElement {
    v_flex()
        .gap(px(3.))
        .child(
            div()
                .text_size(tokens::text_xs())
                .text_color(tokens::text_muted())
                .child(name),
        )
        .child(
            h_flex()
                .gap(px(4.))
                .children(inputs.iter().zip(["X", "Y", "Z"]).map(|(input, axis)| {
                    h_flex()
                        .flex_1()
                        .gap(px(3.))
                        .items_center()
                        .child(
                            div()
                                .text_size(tokens::text_xs())
                                .text_color(tokens::text_muted())
                                .child(axis),
                        )
                        .child(field(input).flex_1())
                })),
        )
}

pub(super) fn wide_button(id: &'static str, text: &'static str, enabled: bool) -> Stateful<Div> {
    chrome::button(id, text, false)
        .flex_1()
        .debug_selector(move || id.into())
        .when(!enabled, |this| this.opacity(0.4).cursor_not_allowed())
}

/// [`wide_button`] for a button repeated in a list, keyed per row.
pub(super) fn keyed_button(
    id: ElementId,
    text: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    chrome::button(id, text.into(), false)
        .flex_1()
        .when(!enabled, |this| this.opacity(0.4).cursor_not_allowed())
}

/// The panel's primary action, at the 44px a primary control takes.
pub(super) fn primary_button(id: &'static str, text: &'static str) -> Stateful<Div> {
    chrome::button(id, text, true)
        .flex_1()
        .h(tokens::primary_target())
        .debug_selector(move || id.into())
}

pub(super) fn picker(
    shell: &Shell,
    menu_id: MenuId,
    id: &'static str,
    text: &str,
    items: Vec<menu::Item>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    menu::dropdown(
        shell,
        menu_id,
        chrome::Trigger::new(
            chrome::button(id, text.to_owned(), false)
                .gap(px(4.))
                .child(component::Icon::new(IconName::ChevronDown).size(px(10.))),
        ),
        items,
        cx,
    )
}
