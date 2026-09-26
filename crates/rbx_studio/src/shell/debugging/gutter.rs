//! Breakpoints in the Script Editor's gutter: the icons and what clicking
//! them does. The right-click menu is `breakpoint_menu`.
//!
//! Studio's gutter, per `studio/debugging.md`: a click on an empty line
//! inserts a breakpoint, a click on its icon disables or re-enables it, a
//! middle-click deletes it, and right-click offers the four kinds and the
//! edit window. Icons: a red circle, a circled `=` for a conditional one, a
//! red diamond for a logpoint, each hollow while disabled.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_base::input::Gutter;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::debugger::Kind;
use crate::tokens;

use super::super::Shell;
use super::breakpoint_menu::Menu;

/// A marker's side, in pixels.
const MARK: f32 = 10.;

impl Shell {
    /// Re-installs every open tab's gutter from the breakpoints and the
    /// pause. Called after anything that changes either — never per frame,
    /// since installing one re-renders the editor.
    pub(in crate::shell) fn sync_gutters(&mut self, cx: &mut Context<Self>) {
        let scripts: Vec<Ref> = self.scripts.open.keys().copied().collect();
        for script in scripts {
            let gutter = self.gutter_for(script, cx);
            if let Some(open) = self.scripts.open.get(&script) {
                open.state
                    .update(cx, |state, cx| state.set_gutter(Some(gutter), cx));
            }
        }
    }

    fn gutter_for(&self, script: Ref, cx: &mut Context<Self>) -> Gutter {
        let marks: HashMap<usize, (Kind, bool)> = self
            .debug
            .breakpoints
            .of(script)
            .map(|stored| {
                (
                    stored.breakpoint.line as usize - 1,
                    (stored.kind(), stored.enabled),
                )
            })
            .collect();
        let paused = self
            .debug
            .run
            .as_ref()
            .filter(|run| run.script == script)
            .and_then(|run| run.pause.as_ref())
            .map(|pause| pause.line as usize - 1);
        let shell = cx.entity().downgrade();
        Gutter {
            marker: Rc::new(move |line, _, _| {
                marker(marks.get(&line).copied(), paused == Some(line))
            }),
            on_click: Rc::new(move |line, button, event, _, cx| {
                let position = event.position;
                let _ = shell.update(cx, |shell, cx| {
                    shell.gutter_clicked(script, line as u32 + 1, button, position, cx);
                });
            }),
            highlight: paused.map(|line| (line, Hsla::from(tokens::warning()).opacity(0.22))),
        }
    }

    fn gutter_clicked(
        &mut self,
        script: Ref,
        line: u32,
        button: MouseButton,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let exists = self.debug.breakpoints.get(script, line).is_some();
        match button {
            MouseButton::Left if exists => self.debug.breakpoints.toggle_enabled(script, line),
            MouseButton::Left => self.debug.breakpoints.insert(script, line, Kind::Standard),
            MouseButton::Middle => self.debug.breakpoints.remove(script, line),
            MouseButton::Right => {
                self.debug.menu = Some(Menu::new(script, line, position));
                cx.notify();
                return;
            }
            _ => return,
        }
        self.sync_gutters(cx);
        cx.notify();
    }

    /// F9: a standard breakpoint on the caret's line, or none if it had one.
    pub(in crate::shell) fn toggle_breakpoint_at_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(script) = self.scripts.tabs.active() else {
            return;
        };
        let Some(open) = self.scripts.open.get(&script) else {
            return;
        };
        let (text, cursor) = {
            let state = open.state.read(cx);
            (state.value().to_string(), state.cursor())
        };
        let line = line_of(&text, cursor);
        match self.debug.breakpoints.get(script, line) {
            Some(_) => self.debug.breakpoints.remove(script, line),
            None => self.debug.breakpoints.insert(script, line, Kind::Standard),
        }
        self.sync_gutters(cx);
        cx.notify();
    }
}

/// The 1-based line holding byte `offset`.
fn line_of(text: &str, offset: usize) -> u32 {
    let end = offset.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32
        + 1
}

/// One gutter cell's icon: the breakpoint's, with the paused-here arrow on
/// top of it when the script is stopped on this line.
fn marker(mark: Option<(Kind, bool)>, paused: bool) -> Option<AnyElement> {
    if mark.is_none() && !paused {
        return None;
    }
    let red: Hsla = tokens::text_error().into();
    let yellow: Hsla = tokens::warning().into();
    Some(
        div()
            .relative()
            .ml(px(2.))
            .size(px(MARK + 2.))
            .flex()
            .items_center()
            .justify_center()
            .when_some(mark, |this, (kind, enabled)| {
                this.child(breakpoint_icon(kind, enabled, red))
            })
            .when(paused, |this| {
                this.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| arrow(bounds, yellow, window),
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .into_any_element(),
    )
}

fn breakpoint_icon(kind: Kind, enabled: bool, red: Hsla) -> AnyElement {
    if kind == Kind::Logpoint {
        return canvas(
            |_, _, _| {},
            move |bounds, _, window, _| diamond(bounds, red, enabled, window),
        )
        .size(px(MARK))
        .into_any_element();
    }
    div()
        .size(px(MARK))
        .rounded_full()
        .border_1()
        .border_color(red)
        .when(enabled, |this| this.bg(red))
        .flex()
        .items_center()
        .justify_center()
        .when(kind == Kind::Conditional, |this| {
            this.child(
                div()
                    .text_size(px(8.))
                    .line_height(px(8.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(if enabled { tokens::dock().into() } else { red })
                    .child("="),
            )
        })
        .into_any_element()
}

fn diamond(bounds: Bounds<Pixels>, colour: Hsla, filled: bool, window: &mut Window) {
    let c = bounds.center();
    let r = bounds.size.width / 2.;
    let points = [
        point(c.x, c.y - r),
        point(c.x + r, c.y),
        point(c.x, c.y + r),
        point(c.x - r, c.y),
    ];
    let mut builder = match filled {
        true => PathBuilder::fill(),
        false => PathBuilder::stroke(px(1.)),
    };
    builder.add_polygon(&points, true);
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour);
    }
}

/// Studio's yellow "runs next" arrow.
fn arrow(bounds: Bounds<Pixels>, colour: Hsla, window: &mut Window) {
    let c = bounds.center();
    let r = bounds.size.width / 2.;
    let mut builder = PathBuilder::fill();
    builder.add_polygon(
        &[
            point(c.x - r, c.y - r * 0.35),
            point(c.x, c.y - r * 0.35),
            point(c.x, c.y - r * 0.8),
            point(c.x + r, c.y),
            point(c.x, c.y + r * 0.8),
            point(c.x, c.y + r * 0.35),
            point(c.x - r, c.y + r * 0.35),
        ],
        true,
    );
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::line_of;

    #[test]
    fn line_of_counts_newlines_before_the_offset() {
        let text = "a\nbc\nd";
        assert_eq!(line_of(text, 0), 1);
        assert_eq!(line_of(text, 2), 2);
        assert_eq!(line_of(text, 4), 2);
        assert_eq!(line_of(text, 5), 3);
        assert_eq!(line_of(text, 99), 3);
    }
}
