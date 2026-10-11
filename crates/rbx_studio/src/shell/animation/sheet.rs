//! The dope sheet, Blender's layout: a ruler of frames along the top, the
//! channel list down the left (a summary row, then each joint in the rig's
//! hierarchy), and each joint's keys as diamonds on its row. Click a key to
//! select it (Shift adds), drag to move, click or drag on the ruler to scrub,
//! wheel to zoom, Shift+wheel or middle-drag to pan.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Gesture, Shell};
use crate::anim_editor::{rows, Row};
use crate::timeline::{self, paint, Timeline};
use crate::tokens;

pub(super) const NAMES_WIDTH: f32 = 196.0;
const RULER: f32 = 26.0;
const ROW: f32 = 20.0;
const KEY: f32 = 4.5;

/// What the sheet paints, copied out of the editor for the paint closure.
#[derive(Clone)]
struct Lines {
    rows: Vec<(Vec<f32>, Vec<bool>)>,
    summary: Vec<(f32, bool)>,
    selected_row: Option<usize>,
    length: f32,
}

impl Shell {
    fn sheet_rows(&self) -> Vec<Row> {
        rows(
            self.anim.bound.as_ref().map(|b| &b.rig),
            &self.anim.editor.clip,
        )
    }

    fn sheet_lines(&self, rows: &[Row]) -> Lines {
        let e = &self.anim.editor;
        let summary = e
            .clip
            .keyframes
            .iter()
            .map(|k| {
                (
                    k.time,
                    k.poses.keys().all(|j| e.is_selected(j, k.time)) && !k.poses.is_empty(),
                )
            })
            .collect();
        Lines {
            rows: rows
                .iter()
                .map(|r| {
                    let keys: Vec<f32> = e.clip.track(&r.name).map(|(t, _)| t).collect();
                    let sel = keys.iter().map(|t| e.is_selected(&r.name, *t)).collect();
                    (keys, sel)
                })
                .collect(),
            summary,
            selected_row: e
                .joint
                .as_ref()
                .and_then(|j| rows.iter().position(|r| &r.name == j)),
            length: e.clip.length(),
        }
    }

    pub(super) fn dope_sheet(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.sheet_rows();
        let lines = self.sheet_lines(&rows);
        let timeline = self.anim.timeline;
        let time = self.anim.editor.time;
        let ruler_bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));
        let width = self
            .anim
            .sheet
            .get()
            .map_or(800.0, |b| f32::from(b.size.width));
        let ticks = timeline.ticks(width);
        let fps = self.anim.editor.fps;
        let markers: Vec<f32> = self
            .anim
            .editor
            .clip
            .keyframes
            .iter()
            .filter(|k| !k.markers.is_empty())
            .map(|k| k.time)
            .collect();

        let names = rows.iter().enumerate().map(|(i, row)| {
            let selected = lines.selected_row == Some(i);
            let name = row.name.clone();
            div()
                .id(("anim-channel", i))
                .h(px(ROW))
                .flex_none()
                .flex()
                .items_center()
                .pl(px(10.0 + row.depth as f32 * 12.0))
                .pr(px(6.0))
                .gap(px(6.0))
                .text_size(tokens::text_sm())
                .text_color(if row.in_rig {
                    tokens::text_strong()
                } else {
                    tokens::warning()
                })
                .when(selected, |this| this.bg(tokens::accent_soft()))
                .cursor_pointer()
                .hover(|this| this.bg(tokens::hover_subtle()))
                .child(
                    component::Icon::new(IconName::Bone)
                        .size(px(12.))
                        .text_color(tokens::text_muted()),
                )
                .child(div().truncate().child(row.name.clone()))
                .when(!row.in_rig, |this| {
                    this.tooltip(|window, cx| {
                        super::super::tooltip::text(
                            "Not a joint of this rig: it will not play",
                            window,
                            cx,
                        )
                    })
                })
                .on_click(cx.listener(move |shell, _, window, cx| {
                    shell.focus_channel(&name, cx);
                    window.focus(&shell.anim.focus, cx);
                }))
        });

        let sheet_report = self.anim.sheet.clone();
        let body_lines = lines.clone();
        let body_height = (rows.len() + 1) as f32 * ROW;
        let report_ruler = ruler_bounds.clone();

        let ruler = div()
            .id("anim-ruler")
            .relative()
            .h(px(RULER))
            .flex_1()
            .bg(tokens::chrome())
            .border_b(px(1.))
            .border_color(tokens::border())
            .overflow_hidden()
            .child(
                canvas(
                    move |b, _, _| report_ruler.set(Some(b)),
                    move |b, _, window, _| {
                        let origin = b.origin;
                        let (ox, oy) = (f32::from(origin.x), f32::from(origin.y));
                        paint::ticks(window, origin, &ticks, RULER, 0.0);
                        for m in &markers {
                            let x = ox + timeline.x_of(*m);
                            paint::diamond(
                                window,
                                x,
                                oy + 6.0,
                                3.0,
                                tokens::warning(),
                                tokens::black(),
                            );
                        }
                        paint::playhead(window, origin, &timeline, time, RULER);
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(
                timeline
                    .ticks(width)
                    .into_iter()
                    .filter(|t| t.major)
                    .map(|t| {
                        div()
                            .absolute()
                            .top(px(3.))
                            .left(px(t.x + 3.))
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_muted())
                            .child(timeline::label(t.frame, fps, false))
                    }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(px(timeline.x_of(time) - 14.))
                    .w(px(28.))
                    .h(px(18.))
                    .rounded(px(4.))
                    .bg(tokens::check_on())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::avatar_on_accent())
                    .child(self.anim.editor.frame().to_string()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, e: &MouseDownEvent, window, cx| {
                    window.focus(&shell.anim.focus, cx);
                    if let Some(b) = ruler_bounds.get() {
                        let x = f32::from(e.position.x - b.origin.x);
                        shell.anim.gesture = Some(Gesture::Scrub);
                        shell.anim.editor.playing = false;
                        shell.anim.editor.set_time(shell.anim.timeline.time_at(x));
                        cx.notify();
                    }
                }),
            );

        let keys = div()
            .id("anim-keys")
            .relative()
            .flex_1()
            .h(px(body_height))
            .child(
                canvas(
                    move |b, _, _| sheet_report.set(Some(b)),
                    move |b, _, window, _| paint_rows(window, b, &timeline, &body_lines, time),
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|shell, e: &MouseDownEvent, window, cx| {
                    window.focus(&shell.anim.focus, cx);
                    shell.sheet_press(e.position, e.modifiers.shift, e.click_count, cx);
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

        let summary_label = format!("Summary · {}", self.anim.editor.clip.name);
        div()
            .id("anim-sheet")
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
                            .child(format!("{} fps", fps)),
                    )
                    .child(ruler),
            )
            .child(
                div()
                    .id("anim-sheet-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .child(
                                div()
                                    .w(px(NAMES_WIDTH))
                                    .flex_none()
                                    .border_r(px(1.))
                                    .border_color(tokens::border())
                                    .child(
                                        div()
                                            .h(px(ROW))
                                            .flex()
                                            .items_center()
                                            .px(px(8.))
                                            .gap(px(6.))
                                            .text_size(tokens::text_sm())
                                            .text_color(tokens::text_label())
                                            .bg(tokens::wash_faint())
                                            .child(
                                                component::Icon::new(IconName::Film).size(px(12.)),
                                            )
                                            .child(div().truncate().child(summary_label)),
                                    )
                                    .children(names),
                            )
                            .child(keys),
                    ),
            )
            .into_any_element()
    }

    /// Selects a channel: the joint the pose fields edit, and its part in
    /// the Explorer and viewport, where the Rotate tool can then pose it.
    pub(super) fn focus_channel(&mut self, joint: &str, cx: &mut Context<Self>) {
        self.anim.editor.joint = Some(joint.to_owned());
        let part = self
            .anim
            .bound
            .as_ref()
            .and_then(|b| b.rig.joint_for(joint))
            .map(|j| j.part1);
        if let Some(part) = part {
            self.select(part, cx);
        }
        cx.notify();
    }

    fn sheet_press(
        &mut self,
        at: Point<Pixels>,
        shift: bool,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(b) = self.anim.sheet.get() else {
            return;
        };
        let x = f32::from(at.x - b.origin.x);
        let y = f32::from(at.y - b.origin.y);
        let row = (y / ROW).floor() as isize;
        let rows = self.sheet_rows();
        let tl = self.anim.timeline;
        self.anim.editor.playing = false;
        if row == 0 {
            let times: Vec<f32> = self
                .anim
                .editor
                .clip
                .keyframes
                .iter()
                .map(|k| k.time)
                .collect();
            if let Some(i) = tl.hit(&times, x) {
                self.anim.editor.select_keyframe(times[i], shift);
                self.anim.editor.begin_drag();
                self.anim.gesture = Some(Gesture::MoveKeys { from_x: x });
                cx.notify();
                return;
            }
        } else if let Some(r) = rows.get((row - 1).max(0) as usize).filter(|_| row >= 1) {
            let times: Vec<f32> = self
                .anim
                .editor
                .clip
                .track(&r.name)
                .map(|(t, _)| t)
                .collect();
            if let Some(i) = tl.hit(&times, x) {
                let key = (r.name.clone(), times[i]);
                if !self.anim.editor.is_selected(&key.0, key.1) || shift {
                    self.anim.editor.select(key, shift);
                }
                self.anim.editor.joint = Some(r.name.clone());
                self.anim.editor.begin_drag();
                self.anim.gesture = Some(Gesture::MoveKeys { from_x: x });
                cx.notify();
                return;
            }
            if clicks >= 2 {
                let name = r.name.clone();
                self.anim.editor.set_time(tl.time_at(x));
                self.key_joints(Some(&name), cx);
                return;
            }
            self.anim.editor.joint = Some(r.name.clone());
        }
        if !shift {
            self.anim.editor.selected.clear();
        }
        self.anim.editor.set_time(tl.time_at(x));
        self.anim.gesture = Some(Gesture::Scrub);
        cx.notify();
    }

    fn sheet_drag(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(gesture) = self.anim.gesture else {
            return;
        };
        let Some(b) = self.anim.sheet.get() else {
            return;
        };
        let x = f32::from(at.x - b.origin.x);
        match gesture {
            Gesture::Scrub => {
                let t = self.anim.timeline.time_at(x);
                self.anim.editor.set_time(t);
            }
            Gesture::MoveKeys { from_x } => {
                let tl = self.anim.timeline;
                self.anim.editor.drag_by(tl.time_at(x) - tl.time_at(from_x));
            }
            Gesture::Pan { last_x } => {
                let now = f32::from(at.x);
                self.anim.timeline.pan(now - last_x);
                self.anim.gesture = Some(Gesture::Pan { last_x: now });
            }
        }
        cx.notify();
    }

    fn sheet_release(&mut self, cx: &mut Context<Self>) {
        if self.anim.gesture.take().is_some() {
            self.anim.editor.end_drag();
            cx.notify();
        }
    }

    fn sheet_wheel(&mut self, e: &ScrollWheelEvent, window: &Window, cx: &mut Context<Self>) {
        let Some(b) = self.anim.sheet.get() else {
            return;
        };
        let delta = e.delta.pixel_delta(window.line_height());
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if e.modifiers.shift || dx.abs() > dy.abs() {
            self.anim.timeline.pan(if dx != 0.0 { dx } else { dy });
        } else if e.modifiers.control || e.modifiers.platform {
            let x = f32::from(e.position.x - b.origin.x);
            self.anim.timeline.zoom_about(x, (dy * 0.01).exp());
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Keys on the dope sheet, Blender's where they exist: Space plays,
    /// I inserts a key, X or Delete removes, arrows step frames (Up/Down
    /// jump between keys), Home/End go to the ends, A selects every key,
    /// Alt+A or Escape none, F fits the clip, Ctrl+C/V copy and paste.
    fn sheet_key(&mut self, key: &Keystroke, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let m = &key.modifiers;
        let ctrl = m.control || m.platform;
        let e = &mut self.anim.editor;
        match key.key.as_str() {
            "space" => e.toggle_play(),
            "i" if !ctrl => {
                let joint = e.joint.clone();
                self.key_joints(joint.as_deref(), cx);
                return true;
            }
            "x" | "delete" | "backspace" if !ctrl => {
                e.delete_selected();
            }
            "left" => e.step_frames(-1),
            "right" => e.step_frames(1),
            "up" => e.jump_key(true),
            "down" => e.jump_key(false),
            "home" => e.set_time(0.0),
            "end" => {
                let length = e.clip.length();
                e.set_time(length);
            }
            "a" if m.alt => e.selected.clear(),
            "a" if !ctrl => {
                let all = e
                    .clip
                    .keyframes
                    .iter()
                    .flat_map(|k| k.poses.keys().map(move |j| (j.clone(), k.time)))
                    .collect();
                e.selected = all;
            }
            "escape" => e.selected.clear(),
            "f" if !ctrl => self.fit_timeline(),
            "c" if ctrl => {
                e.copy();
            }
            "v" if ctrl => {
                e.paste();
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Fits the timeline to the clip's length (or a second, for an empty
    /// one) across the sheet's current width.
    pub(super) fn fit_timeline(&mut self) {
        let width = self
            .anim
            .sheet
            .get()
            .map_or(800.0, |b| f32::from(b.size.width));
        let length = self.anim.editor.clip.length().max(1.0);
        self.anim.timeline.fps = self.anim.editor.fps;
        self.anim.timeline.fit(length, width);
        self.anim.fitted = true;
    }

    /// Keys `joint` (or every joint the rig has, with `None`) at the
    /// playhead in the pose it shows now.
    pub(super) fn key_joints(&mut self, joint: Option<&str>, cx: &mut Context<Self>) {
        let current = self.current_transforms();
        let names: Vec<String> = match (joint, &self.anim.bound) {
            (Some(j), _) => vec![j.to_owned()],
            (None, Some(b)) => b
                .rig
                .joints
                .iter()
                .map(|j| b.rig.part_name(j.part1).to_owned())
                .collect(),
            (None, None) => self.anim.editor.clip.joints(),
        };
        let poses: Vec<(String, rbx_animation::Frame)> = names
            .into_iter()
            .map(|n| {
                let f = current.get(&n).copied().unwrap_or_default();
                (n, f)
            })
            .collect();
        self.anim.editor.key_all(&poses);
        cx.notify();
    }
}

fn paint_rows(window: &mut Window, b: Bounds<Pixels>, tl: &Timeline, lines: &Lines, time: f32) {
    let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    // Outside the clip's span, shaded: Blender's frame-range dimming.
    let (start, end) = (tl.x_of(0.0), tl.x_of(lines.length.max(0.0)));
    paint::rect(window, ox, oy, start.clamp(0.0, w), h, tokens::wash_faint());
    paint::rect(
        window,
        ox + end.clamp(0.0, w),
        oy,
        (w - end).max(0.0),
        h,
        tokens::wash_faint(),
    );
    paint::rect(window, ox, oy, w, ROW, tokens::wash_faint());
    if let Some(r) = lines.selected_row {
        paint::rect(
            window,
            ox,
            oy + (r + 1) as f32 * ROW,
            w,
            ROW,
            tokens::accent_soft(),
        );
    }
    for tick in tl.ticks(w).into_iter().filter(|t| t.major) {
        paint::rect(window, ox + tick.x, oy, 1.0, h, tokens::border());
    }
    for r in 0..=lines.rows.len() {
        paint::rect(
            window,
            ox,
            oy + (r + 1) as f32 * ROW - 1.0,
            w,
            1.0,
            tokens::border(),
        );
    }
    let key_fill = |selected: bool| {
        if selected {
            tokens::check_on()
        } else {
            tokens::text_label()
        }
    };
    for (t, selected) in &lines.summary {
        let x = ox + tl.x_of(*t);
        paint::diamond(
            window,
            x,
            oy + ROW * 0.5,
            KEY + 1.0,
            key_fill(*selected),
            tokens::black(),
        );
    }
    for (r, (keys, selected)) in lines.rows.iter().enumerate() {
        let y = oy + (r + 1) as f32 * ROW + ROW * 0.5;
        // A bar between two keys holding the same pose would need values;
        // the run between first and last key is drawn instead, as the span
        // the joint is animated over.
        if let (Some(first), Some(last)) = (keys.first(), keys.last()) {
            if last > first {
                paint::rect(
                    window,
                    ox + tl.x_of(*first),
                    y - 1.0,
                    tl.x_of(*last) - tl.x_of(*first),
                    2.0,
                    tokens::border2(),
                );
            }
        }
        for (t, s) in keys.iter().zip(selected) {
            paint::diamond(
                window,
                ox + tl.x_of(*t),
                y,
                KEY,
                key_fill(*s),
                tokens::black(),
            );
        }
    }
    paint::playhead(window, b.origin, tl, time, h);
}
