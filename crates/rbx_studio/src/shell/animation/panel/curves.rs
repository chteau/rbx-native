//! The panel's Curve section, shown with the Curve Editor: converting the
//! clip to curves and back, and the selected curve keys' value, frame,
//! interpolation and tangents, plus Generate Curve. Rotation values and
//! tangents read in degrees (per second), position ones in studs.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::curve::Interpolation;
use rbx_animation::{Direction, Style};

use super::super::widgets::{
    field_row, keyed_button, label, picker, primary_button, section, wide_button,
};
use super::{number, resync, Field, Shell};
use crate::shell::menu::{self, MenuId};

impl Shell {
    pub(super) fn commit_curve_field(&mut self, field: Field, text: &str) {
        let e = &mut self.anim.editor;
        let Some(first) = e.curve_selected.first().cloned() else {
            return;
        };
        let text = text.trim();
        // A rotation channel's numbers are typed in degrees.
        let unit = |v: f32| {
            if first.channel.is_rotation() {
                v.to_radians()
            } else {
                v
            }
        };
        match field {
            Field::CurveValue => {
                if let Ok(v) = text.parse::<f32>() {
                    e.set_curve_value(unit(v));
                }
            }
            Field::CurveTime => {
                if let Ok(frame) = text.parse::<f32>() {
                    e.set_curve_time(frame / e.fps.max(1) as f32);
                }
            }
            Field::Tangent { left } => {
                if text.is_empty() || text.eq_ignore_ascii_case("auto") {
                    if let Some(key) = e.curve_key(&first) {
                        // Clearing one side leaves the other as it was.
                        let (l, r) = if left {
                            (None, key.right_tangent)
                        } else {
                            (key.left_tangent, None)
                        };
                        if l.is_none() && r.is_none() {
                            e.auto_curve_tangents();
                        } else {
                            let picked = e.curve_selected.clone();
                            e.edit_curves(|curves| {
                                for id in &picked {
                                    if let Some(c) = curves
                                        .joints
                                        .get_mut(&id.joint)
                                        .map(|j| j.curve_mut(id.channel))
                                    {
                                        if let Some(i) = c
                                            .keys
                                            .iter()
                                            .position(|k| (k.time - id.time).abs() < 1e-4)
                                        {
                                            c.set_tangents(i, l, r);
                                        }
                                    }
                                }
                            });
                        }
                    }
                } else if let Ok(v) = text.parse::<f32>() {
                    let slope = unit(v);
                    if left {
                        e.set_curve_tangents(Some(slope), None);
                    } else {
                        e.set_curve_tangents(None, Some(slope));
                    }
                }
            }
            _ => {}
        }
    }

    pub(in crate::shell::animation) fn anim_curve_section(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.anim.editor.curves.is_none() {
            return section("Curves")
                .child(label(
                    "Convert the keyframes to curves to edit each channel on its own".into(),
                ))
                .child(h_flex().child(
                    primary_button("anim-convert-curves", "Convert to Curves").on_click(
                        cx.listener(|shell, _, _, cx| {
                            shell.anim.editor.convert_to_curves();
                            cx.notify();
                        }),
                    ),
                ))
                .into_any_element();
        }
        let Some(inputs) = self.anim.fields.as_ref().map(|f| f.curve.clone()) else {
            return div().into_any_element();
        };
        let e = &self.anim.editor;
        let count = e.curve_selected.len();
        let first = e.curve_selected.first().cloned();
        let key = first.as_ref().and_then(|id| e.curve_key(id));
        if let (Some(id), Some(key)) = (&first, key) {
            let shown = |v: f32| {
                if id.channel.is_rotation() {
                    v.to_degrees()
                } else {
                    v
                }
            };
            let frame = key.time * e.fps.max(1) as f32;
            let tangent = |t: Option<f32>| t.map_or("Auto".to_owned(), |t| number(shown(t)));
            let seeds = [
                number(shown(key.value)),
                number(frame),
                tangent(key.left_tangent),
                tangent(key.right_tangent),
            ];
            for (input, seed) in inputs.iter().zip(seeds) {
                resync(input, &seed, window, cx);
            }
        }
        let interpolation = e.curve_interpolation();
        let (style, direction) = (self.anim.curve.style, self.anim.curve.direction);
        let unit = first.as_ref().map_or("", |id| {
            if id.channel.is_rotation() {
                " (°)"
            } else {
                " (studs)"
            }
        });
        let summary = match (&first, count) {
            (None, _) => "No curve key selected".to_owned(),
            (Some(id), 1) => format!("{} · {}{unit}", id.joint, id.channel.label()),
            (Some(_), n) => format!("{n} keys selected"),
        };
        let styles = Style::PICKABLE
            .into_iter()
            .filter(|s| *s != Style::Linear && *s != Style::Constant)
            .map(|s| {
                menu::item(s.label())
                    .checked(s == style)
                    .on_click(move |shell, cx| {
                        shell.anim.curve.style = s;
                        cx.notify();
                    })
            })
            .collect();
        let directions = Direction::ALL
            .into_iter()
            .map(|d| {
                menu::item(d.label())
                    .checked(d == direction)
                    .on_click(move |shell, cx| {
                        shell.anim.curve.direction = d;
                        cx.notify();
                    })
            })
            .collect();
        let any = count > 0;
        let modes = Interpolation::ALL.into_iter().map(|mode| {
            keyed_button(
                ("anim-curve-interp", mode.value() as usize).into(),
                mode.label(),
                any,
            )
            .when(interpolation == Some(mode), |this| {
                this.bg(crate::tokens::accent_soft())
            })
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.anim.editor.set_curve_interpolation(mode);
                cx.notify();
            }))
        });
        let cubic = interpolation == Some(Interpolation::Cubic);
        section("Curve key")
            .child(label(summary))
            .when(count == 1, |this| {
                this.child(field_row("Value", &inputs[0]))
                    .child(field_row("Frame", &inputs[1]))
            })
            .child(h_flex().gap(px(6.)).children(modes))
            .when(any, |this| {
                this.child(field_row("In tan", &inputs[2]))
                    .when(cubic, |this| this.child(field_row("Out tan", &inputs[3])))
                    .child(h_flex().gap(px(6.)).child(
                        wide_button("anim-curve-auto", "Auto tangents", true).on_click(
                            cx.listener(|shell, _, _, cx| {
                                shell.anim.editor.auto_curve_tangents();
                                cx.notify();
                            }),
                        ),
                    ))
            })
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(picker(
                        self,
                        MenuId::AnimCurveStyle,
                        "anim-curve-style",
                        style.label(),
                        styles,
                        cx,
                    ))
                    .child(picker(
                        self,
                        MenuId::AnimCurveDirection,
                        "anim-curve-direction",
                        direction.label(),
                        directions,
                        cx,
                    ))
                    .child(
                        wide_button("anim-curve-generate", "Generate", any).on_click(cx.listener(
                            |shell, _, _, cx| {
                                let (s, d) = (shell.anim.curve.style, shell.anim.curve.direction);
                                if !shell.anim.editor.generate_curve(s, d) {
                                    shell
                                        .anim
                                        .warn("Select a key with one after it to generate");
                                }
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(h_flex().gap(px(6.)).child(
                wide_button("anim-to-keyframes", "Back to keyframes", true).on_click(cx.listener(
                    |shell, _, _, cx| {
                        shell.anim.editor.back_to_keyframes();
                        cx.notify();
                    },
                )),
            ))
            .into_any_element()
    }
}
