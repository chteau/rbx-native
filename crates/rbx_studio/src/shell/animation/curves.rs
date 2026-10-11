//! The Curve Editor, Roblox's second view of a clip: each joint's Position
//! and Rotation X/Y/Z as curves over time, keys as dots, a Cubic key's
//! tangents as handles. The channel list picks the joint (and, optionally,
//! one channel); the plot fits that joint's curves to its height with studs
//! on the left ruler and degrees on the right. Click a key to select it
//! (Shift adds), drag to move it in time and value, drag a handle to bend
//! its tangent, double-click a curve to add a key there. The clip has to be
//! converted to curves first, as in Roblox; the side panel's Curve section
//! does that and holds the selected keys' settings.

use gpui_kit::assets::IconName;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::curve::{Channel, FloatCurve, Interpolation, JointCurves, Ranges};
use rbx_animation::{Direction, Style};

use super::sheet::NAMES_WIDTH;
use super::widgets::primary_button;
use super::{Gesture, Shell};
use crate::anim_editor::{rows, CurveKeyId};
use crate::timeline::paint;
use crate::tokens;

mod plot;

use plot::{Plot, HIT};

const ROW: f32 = 20.0;
const RULER: f32 = 26.0;
/// Horizontal lines across the plot, each labelled on both rulers.
const GRID_LINES: usize = 5;

/// What the Curve Editor shows, beyond the editor's own state.
pub(in crate::shell) struct CurveView {
    /// One channel on its own, or (`None`) all six of the joint's.
    pub(super) channel: Option<Channel>,
    /// The value ranges last fitted, held still while a key is dragged so
    /// the plot does not rescale under the pointer.
    fit: Ranges,
    /// The easing Generate Curve bakes.
    pub(super) style: Style,
    pub(super) direction: Direction,
}

impl Default for CurveView {
    fn default() -> Self {
        CurveView {
            channel: None,
            fit: Ranges::default(),
            style: Style::Bounce,
            direction: Direction::Out,
        }
    }
}

/// One curve as the paint closure draws it.
#[derive(Clone)]
struct Line {
    channel: Channel,
    curve: FloatCurve,
    selected: Vec<bool>,
}

impl Shell {
    fn curve_channels(&self) -> Vec<Channel> {
        match self.anim.curve.channel {
            Some(c) => vec![c],
            None => Channel::ALL.to_vec(),
        }
    }

    fn shown_curves(&self) -> Option<(&str, &JointCurves)> {
        let joint = self.anim.editor.joint.as_deref()?;
        let curves = self.anim.editor.curves.as_ref()?.joints.get(joint)?;
        Some((joint, curves))
    }

    /// The plot as it is laid out now.
    fn curve_plot(&self) -> Option<Plot> {
        let b = self.anim.sheet.get()?;
        Some(Plot::new(
            self.anim.timeline,
            f32::from(b.size.height),
            self.anim.curve.fit,
        ))
    }

    fn refit_curves(&mut self) {
        if self.anim.gesture.is_some() {
            return;
        }
        let channels = self.curve_channels();
        self.anim.curve.fit = plot::fit(self.shown_curves().map(|(_, c)| c), &channels);
    }

    pub(super) fn curve_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.anim.editor.curves.is_none() {
            return self.curve_editor_empty(cx);
        }
        self.refit_curves();
        let plot = self.curve_plot();
        let channels = self.curve_channels();
        let joint = self.anim.editor.joint.clone();
        let lines: Vec<Line> = self
            .shown_curves()
            .map(|(name, curves)| {
                channels
                    .iter()
                    .map(|&channel| {
                        let curve = curves.curve(channel).clone();
                        let selected = curve
                            .keys
                            .iter()
                            .map(|k| {
                                self.anim.editor.is_curve_selected(&CurveKeyId {
                                    joint: name.to_owned(),
                                    channel,
                                    time: k.time,
                                })
                            })
                            .collect();
                        Line {
                            channel,
                            curve,
                            selected,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let names = self.curve_rows(joint.as_deref(), cx);
        let report = self.anim.sheet.clone();
        let time = self.anim.editor.time;
        let length = self.anim.editor.clip.length();
        let ruler = self.anim_ruler(cx);
        let labels = plot.map(|p| (p.ruler(false, GRID_LINES), p.ruler(true, GRID_LINES)));
        let has_rotation = channels.iter().any(|c| c.is_rotation());
        let has_position = channels.iter().any(|c| !c.is_rotation());

        let canvas_el = canvas(
            move |b, _, _| report.set(Some(b)),
            move |b, _, window, _| {
                let h = f32::from(b.size.height);
                let Some(mut plot) = plot else { return };
                plot.height = h;
                paint_plot(window, b, &plot, &lines, time, length);
            },
        )
        .absolute()
        .size_full();

        let ruler_labels = labels.map(|(left, right)| {
            let tag = |y: f32, text: String, right_side: bool| {
                div()
                    .absolute()
                    .top(px(y - 13.))
                    .when(right_side, |this| this.right(px(4.)))
                    .when(!right_side, |this| this.left(px(4.)))
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text_muted())
                    .child(text)
            };
            let mut tags: Vec<AnyElement> = Vec::new();
            if has_position {
                tags.extend(
                    left.into_iter()
                        .map(|(y, v)| tag(y, format!("{v:.2}"), false).into_any_element()),
                );
            }
            if has_rotation {
                tags.extend(right.into_iter().map(|(y, v)| {
                    tag(y, format!("{:.0}°", v.to_degrees()), true).into_any_element()
                }));
            }
            tags
        });

        let body = div()
            .id("anim-curve-plot")
            .debug_selector(|| "anim-curve-plot".into())
            .relative()
            .flex_1()
            .h_full()
            .overflow_hidden()
            .child(canvas_el)
            .children(ruler_labels.unwrap_or_default())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|shell, e: &MouseDownEvent, window, cx| {
                    window.focus(&shell.anim.focus, cx);
                    shell.curve_press(e.position, e.modifiers.shift, e.click_count, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|shell, e: &MouseDownEvent, _, _| {
                    shell.anim.gesture = Some(Gesture::Pan {
                        last_x: f32::from(e.position.x),
                    });
                }),
            );

        let header = match (&joint, self.anim.curve.channel) {
            (Some(j), Some(c)) => format!("{j} · {}", c.label()),
            (Some(j), None) => j.clone(),
            (None, _) => "Pick a joint".into(),
        };
        div()
            .id("anim-curves")
            .size_full()
            .flex()
            .flex_col()
            .bg(tokens::dock())
            .track_focus(&self.anim.focus)
            .on_key_down(cx.listener(|shell, e: &KeyDownEvent, window, cx| {
                if shell.sheet_key(&e.keystroke, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_move(
                cx.listener(|shell, e: &MouseMoveEvent, _, cx| shell.sheet_drag(e.position, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, _, _, cx| shell.sheet_release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|shell, _, _, cx| shell.sheet_release(cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|shell, _, _, cx| shell.sheet_release(cx)),
            )
            .on_scroll_wheel(cx.listener(|shell, e: &ScrollWheelEvent, window, cx| {
                shell.sheet_wheel(e, window, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .child(
                        div()
                            .w(px(NAMES_WIDTH))
                            .h(px(RULER))
                            .flex_none()
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .bg(tokens::chrome())
                            .border_b(px(1.))
                            .border_r(px(1.))
                            .border_color(tokens::border())
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_muted())
                            .child(div().truncate().child(header)),
                    )
                    .child(ruler),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        div()
                            .id("anim-curve-channels")
                            .w(px(NAMES_WIDTH))
                            .flex_none()
                            .h_full()
                            .overflow_y_scroll()
                            .border_r(px(1.))
                            .border_color(tokens::border())
                            .children(names),
                    )
                    .child(body),
            )
            .into_any_element()
    }

    fn curve_editor_empty(&mut self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .id("anim-curves")
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .bg(tokens::dock())
            .child(
                div()
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_label())
                    .child(
                        "This animation is keyframes. Convert it to edit each channel as a curve.",
                    ),
            )
            .child(div().w(px(180.)).flex().child(
                primary_button("anim-to-curves", "Convert to Curves").on_click(cx.listener(
                    |shell, _, _, cx| {
                        shell.anim.editor.convert_to_curves();
                        cx.notify();
                    },
                )),
            ))
            .into_any_element()
    }

    /// The channel list: every joint, and under the one being shown its six
    /// channels, each with its axis colour.
    fn curve_rows(&mut self, joint: Option<&str>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let rows = rows(
            self.anim.bound.as_ref().map(|b| &b.rig),
            &self.anim.editor.clip,
        );
        let only = self.anim.curve.channel;
        let keyed = |name: &str| {
            self.anim
                .editor
                .curves
                .as_ref()
                .is_some_and(|c| c.joints.get(name).is_some_and(|j| !j.is_empty()))
        };
        let mut out = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            if crate::anim_editor::keys::face_channel(&row.name).is_some() {
                continue;
            }
            let open = joint == Some(row.name.as_str());
            let name = row.name.clone();
            out.push(
                div()
                    .id(("anim-curve-joint", i))
                    .h(px(ROW))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(px(10.0 + row.depth as f32 * 12.0))
                    .pr(px(6.))
                    .gap(px(6.))
                    .text_size(tokens::text_sm())
                    .text_color(if keyed(&row.name) {
                        tokens::text_strong()
                    } else {
                        tokens::text_muted()
                    })
                    .when(open, |this| this.bg(tokens::accent_soft()))
                    .cursor_pointer()
                    .hover(|this| this.bg(tokens::hover_subtle()))
                    .child(
                        component::Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(10.))
                        .text_color(tokens::text_muted()),
                    )
                    .child(div().truncate().child(row.name.clone()))
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.anim.curve.channel = None;
                        shell.focus_channel(&name, cx);
                        window.focus(&shell.anim.focus, cx);
                    }))
                    .into_any_element(),
            );
            if !open {
                continue;
            }
            for channel in Channel::ALL {
                let picked = only == Some(channel);
                out.push(
                    div()
                        .id(("anim-curve-channel", channel.index()))
                        .debug_selector(move || format!("anim-curve-{}", channel.index()))
                        .h(px(ROW))
                        .flex_none()
                        .flex()
                        .items_center()
                        .pl(px(28.0 + row.depth as f32 * 12.0))
                        .gap(px(6.))
                        .text_size(tokens::text_sm())
                        .text_color(if only.is_none() || picked {
                            tokens::text_strong()
                        } else {
                            tokens::text_muted()
                        })
                        .when(picked, |this| this.bg(tokens::accent_soft()))
                        .cursor_pointer()
                        .hover(|this| this.bg(tokens::hover_subtle()))
                        .child(
                            div()
                                .size(px(8.))
                                .rounded(px(2.))
                                .bg(tokens::axis(channel.axis())),
                        )
                        .child(channel.label())
                        .on_click(cx.listener(move |shell, _, window, cx| {
                            shell.anim.curve.channel =
                                (shell.anim.curve.channel != Some(channel)).then_some(channel);
                            window.focus(&shell.anim.focus, cx);
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        out
    }

    /// The key or handle under `(x, y)` on the plot, nearest first.
    fn curve_hit(&self, plot: &Plot, x: f32, y: f32) -> Option<(CurveKeyId, Option<bool>)> {
        let (joint, curves) = self.shown_curves()?;
        let near = |p: (f32, f32)| (p.0 - x).hypot(p.1 - y) <= HIT;
        for id in &self.anim.editor.curve_selected {
            if id.joint != joint || !self.curve_channels().contains(&id.channel) {
                continue;
            }
            let curve = curves.curve(id.channel);
            let Some(i) = curve
                .keys
                .iter()
                .position(|k| (k.time - id.time).abs() < 1e-4)
            else {
                continue;
            };
            let incoming = i > 0 && curve.keys[i - 1].interpolation == Interpolation::Cubic;
            let (left, right) = plot.handles(id.channel, &curve.keys[i], incoming);
            if left.is_some_and(near) {
                return Some((id.clone(), Some(true)));
            }
            if right.is_some_and(near) {
                return Some((id.clone(), Some(false)));
            }
        }
        self.curve_channels().into_iter().find_map(|channel| {
            curves
                .curve(channel)
                .keys
                .iter()
                .find(|k| near(plot.point(channel, k.time, k.value)))
                .map(|k| {
                    (
                        CurveKeyId {
                            joint: joint.to_owned(),
                            channel,
                            time: k.time,
                        },
                        None,
                    )
                })
        })
    }

    fn curve_press(
        &mut self,
        at: Point<Pixels>,
        shift: bool,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        let (Some(b), Some(plot)) = (self.anim.sheet.get(), self.curve_plot()) else {
            return;
        };
        let x = f32::from(at.x - b.origin.x);
        let y = f32::from(at.y - b.origin.y);
        self.anim.editor.playing = false;
        match self.curve_hit(&plot, x, y) {
            Some((id, Some(left))) => {
                self.anim.editor.curve_selected = vec![id];
                self.anim.editor.begin_curve_drag();
                self.anim.gesture = Some(Gesture::Tangent { left });
            }
            Some((id, None)) => {
                if shift || !self.anim.editor.is_curve_selected(&id) {
                    self.anim.editor.select_curve_key(id, shift);
                }
                self.anim.editor.begin_curve_drag();
                self.anim.gesture = Some(Gesture::CurveKeys { from: (x, y) });
            }
            None if clicks >= 2 => {
                // Onto the curve nearest the pointer, if one passes close.
                let time = self.anim.timeline.time_at(x);
                let nearest = self.shown_curves().and_then(|(_, curves)| {
                    self.curve_channels()
                        .into_iter()
                        .filter_map(|c| {
                            let v = curves.curve(c).eval(time)?;
                            Some((c, (plot.y_of(c, v) - y).abs()))
                        })
                        .filter(|(_, d)| *d <= 8.0)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(c, _)| c)
                });
                if let (Some(channel), Some(joint)) = (nearest, self.anim.editor.joint.clone()) {
                    self.anim.editor.set_time(time);
                    self.anim.editor.insert_curve_key(&joint, channel);
                }
            }
            None => {
                if !shift {
                    self.anim.editor.curve_selected.clear();
                }
                self.anim.editor.set_time(self.anim.timeline.time_at(x));
                self.anim.gesture = Some(Gesture::Scrub);
            }
        }
        cx.notify();
    }

    /// A step of a curve gesture, from `sheet_drag`.
    pub(super) fn curve_drag(&mut self, gesture: Gesture, x: f32, y: f32) {
        let Some(plot) = self.curve_plot() else {
            return;
        };
        match gesture {
            Gesture::CurveKeys { from } => {
                let dt = plot.timeline.time_at(x) - plot.timeline.time_at(from.0);
                let up = from.1 - y;
                self.anim.editor.curve_drag_by(
                    dt,
                    up * plot.per_pixel(Channel::PositionX),
                    up * plot.per_pixel(Channel::RotationX),
                );
            }
            Gesture::Tangent { left } => {
                let Some(id) = self.anim.editor.curve_selected.first().cloned() else {
                    return;
                };
                if let Some(key) = self.anim.editor.curve_key(&id) {
                    let slope = plot.slope_to(id.channel, &key, x, y);
                    self.anim.editor.drag_tangent(left, slope);
                }
            }
            _ => {}
        }
    }

    /// The Curve Editor's own keys, ahead of the dope sheet's: I adds keys
    /// at the playhead on the channels shown, X or Delete removes the
    /// selected keys, A selects every key shown, Escape none.
    pub(super) fn curve_key_press(&mut self, key: &str, alt: bool) -> bool {
        let Some(joint) = self.anim.editor.joint.clone() else {
            return false;
        };
        match key {
            "i" => {
                for channel in self.curve_channels() {
                    self.anim.editor.insert_curve_key(&joint, channel);
                }
                let time = self.anim.editor.snap(self.anim.editor.time);
                self.anim.editor.curve_selected = self
                    .curve_channels()
                    .into_iter()
                    .map(|channel| CurveKeyId {
                        joint: joint.clone(),
                        channel,
                        time,
                    })
                    .collect();
            }
            "x" | "delete" | "backspace" => {
                self.anim.editor.delete_curve_keys();
            }
            "a" if alt => self.anim.editor.curve_selected.clear(),
            "a" => {
                let all = self
                    .shown_curves()
                    .map(|(_, curves)| {
                        self.curve_channels()
                            .into_iter()
                            .flat_map(|channel| {
                                curves
                                    .curve(channel)
                                    .keys
                                    .iter()
                                    .map(move |k| (channel, k.time))
                            })
                            .map(|(channel, time)| CurveKeyId {
                                joint: joint.clone(),
                                channel,
                                time,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.anim.editor.curve_selected = all;
            }
            "escape" => self.anim.editor.curve_selected.clear(),
            _ => return false,
        }
        true
    }
}

fn paint_plot(
    window: &mut Window,
    b: Bounds<Pixels>,
    plot: &Plot,
    lines: &[Line],
    time: f32,
    length: f32,
) {
    let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let tl = &plot.timeline;
    let (start, end) = (tl.x_of(0.0), tl.x_of(length.max(0.0)));
    paint::rect(window, ox, oy, start.clamp(0.0, w), h, tokens::wash_faint());
    paint::rect(
        window,
        ox + end.clamp(0.0, w),
        oy,
        (w - end).max(0.0),
        h,
        tokens::wash_faint(),
    );
    for tick in tl.ticks(w).into_iter().filter(|t| t.major) {
        paint::rect(window, ox + tick.x, oy, 1.0, h, tokens::border());
    }
    for (y, _) in plot.ruler(false, GRID_LINES) {
        paint::rect(window, ox, oy + y, w, 1.0, tokens::border());
    }
    for line in lines {
        let colour = tokens::axis(line.channel.axis());
        if !line.curve.is_empty() {
            let points: Vec<Point<Pixels>> = (0..=(w as usize / 2))
                .filter_map(|i| {
                    let x = i as f32 * 2.0;
                    let v = line.curve.eval(tl.time_at(x))?;
                    Some(point(px(ox + x), px(oy + plot.y_of(line.channel, v))))
                })
                .collect();
            // Rotation reads thinner than position, so the two stay told
            // apart where an X curve of each crosses.
            let width = if line.channel.is_rotation() {
                1.25
            } else {
                2.0
            };
            paint::polyline(window, &points, colour, width);
        }
        for (i, key) in line.curve.keys.iter().enumerate() {
            let (x, y) = plot.point(line.channel, key.time, key.value);
            if line.selected[i] {
                let incoming =
                    i > 0 && line.curve.keys[i - 1].interpolation == Interpolation::Cubic;
                let (left, right) = plot.handles(line.channel, key, incoming);
                for end in [left, right].into_iter().flatten() {
                    paint::line(
                        window,
                        (ox + x, oy + y),
                        (ox + end.0, oy + end.1),
                        tokens::text_label(),
                        1.0,
                    );
                    paint::dot(window, ox + end.0, oy + end.1, 3.0, tokens::text_label());
                }
                paint::dot(window, ox + x, oy + y, 5.0, tokens::black());
                paint::dot(window, ox + x, oy + y, 4.0, tokens::check_on());
            } else {
                paint::dot(window, ox + x, oy + y, 4.0, tokens::black());
                paint::dot(window, ox + x, oy + y, 3.0, colour);
            }
        }
    }
    paint::playhead(window, b.origin, tl, time, h);
}
