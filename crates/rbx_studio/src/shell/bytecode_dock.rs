//! The Bytecode dock: what the active script compiles to, as Luau's own
//! compiler prints it, with the remarks, native IR and assembly views a
//! `luau-compile` run would give you.
//!
//! The text comes from `rbx_bytecode::dump`, run on a background thread over
//! the live editor text, so the listing follows every keystroke without the
//! editor ever waiting on the compiler. Each result is stamped with a hash of
//! everything that went into it (script, text, view, options); when the
//! stamp stops matching, the next render starts a new dump, and a result that
//! arrives for a stale stamp is dropped rather than shown.
//!
//! The listing is virtualised, since a big script compiles to thousands of
//! lines. Source lines in it are clickable and jump to that line in the
//! editor.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_bytecode::{Options, Target, View};
use rbx_dom::Ref;

use crate::tokens;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::tooltip;
use super::Shell;

struct Listing {
    script: Ref,
    key: u64,
    lines: Result<Rc<Vec<SharedString>>, String>,
}

pub(super) struct BytecodeDock {
    view: View,
    options: Options,
    scroll: UniformListScrollHandle,
    shown: Option<Listing>,
    running: bool,
    /// The newest key the render asked for.
    wanted: Option<u64>,
}

impl Default for BytecodeDock {
    fn default() -> Self {
        Self {
            view: View::Bytecode,
            options: Options::default(),
            scroll: UniformListScrollHandle::default(),
            shown: None,
            running: false,
            wanted: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Row {
    Function,
    Source(u32),
    Remark,
    Local,
    Other,
}

fn classify(line: &str) -> Row {
    if line.starts_with("Function ") {
        Row::Function
    } else if let Some(number) = source_line_number(line) {
        Row::Source(number)
    } else if line.trim_start().starts_with("REMARK") {
        Row::Remark
    } else if is_local(line) {
        Row::Local
    } else {
        Row::Other
    }
}

/// The line number of an indented `    12: code` source line.
fn source_line_number(line: &str) -> Option<u32> {
    if !line.starts_with(' ') {
        return None;
    }
    let rest = line.trim_start();
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits == 0 || !rest[digits..].starts_with(':') {
        return None;
    }
    rest[..digits].parse().ok()
}

/// A local-variable line such as `R0: number [argument]`.
fn is_local(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('R') else {
        return false;
    };
    let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
    digits > 0 && rest[digits..].starts_with(':')
}

fn line_start(text: &str, line: u32) -> usize {
    if line <= 1 {
        return 0;
    }
    text.match_indices('\n')
        .nth(line as usize - 2)
        .map_or(text.len(), |(index, _)| index + 1)
}

fn split_lines(text: &str) -> Vec<SharedString> {
    text.lines()
        .map(|line| SharedString::from(line.replace('\t', "    ")))
        .collect()
}

fn chip(
    id: impl Into<ElementId>,
    label: &str,
    on: bool,
    hint: Option<&'static str>,
) -> Stateful<Div> {
    chip_frame(id, on, hint).child(label.to_string())
}

fn chip_frame(id: impl Into<ElementId>, on: bool, hint: Option<&'static str>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .rounded_sm()
        .cursor_pointer()
        .text_size(tokens::text_xs())
        .text_color(tokens::text())
        .when(on, |this| this.bg(tokens::accent_soft()))
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .when_some(hint, |this, hint| {
            this.tooltip(move |window, cx| tooltip::text(hint, window, cx))
        })
}

const OPTIMIZE_HINTS: [&str; 3] = [
    "No optimization",
    "The default: constant folding and fast calls, still easy to debug",
    "Inlining and loop unrolling on top, what live games run",
];

const DEBUG_HINTS: [&str; 3] = [
    "No debug info",
    "Line info and function names, enough for a traceback",
    "Local and upvalue names too, what a debugger needs",
];

fn label(text: &'static str) -> impl IntoElement {
    div()
        .mr_1()
        .text_size(tokens::text_xs())
        .text_color(tokens::text3())
        .child(text)
}

/// A chip with a check in front, for an on/off option.
fn toggle(id: &'static str, text: &str, on: bool, hint: &'static str) -> Stateful<Div> {
    chip_frame(id, on, Some(hint))
        .when(on, |this| {
            this.child(
                Icon::new(IconName::Check)
                    .xsmall()
                    .text_color(tokens::check_on()),
            )
        })
        .child(text.to_string())
}

impl Shell {
    pub(super) fn bytecode_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::BytecodeOverflow,
            chrome::dock_options_trigger(
                "bytecode-overflow",
                IconName::Ellipsis,
                16.,
                "Bytecode dock options",
            ),
            self.move_items(Panel::Bytecode),
            cx,
        );

        let active = self.scripts.tabs.active();
        let text = active.and_then(|r| {
            self.scripts
                .open
                .get(&r)
                .map(|open| open.state.read(cx).value().to_string())
        });
        let view = self.bytecode.view;
        let options = self.bytecode.options;

        if let (Some(script), Some(text)) = (active, text) {
            let mut hasher = DefaultHasher::new();
            (script, &text, view, options).hash(&mut hasher);
            let key = hasher.finish();
            self.bytecode.wanted = Some(key);
            let current = self.bytecode.shown.as_ref().is_some_and(|l| l.key == key);
            if !current && !self.bytecode.running {
                self.bytecode.running = true;
                cx.spawn(async move |this, cx| {
                    let out = cx
                        .background_spawn(async move { rbx_bytecode::dump(&text, view, &options) })
                        .await;
                    let _ = this.update(cx, |shell, cx| {
                        shell.bytecode.running = false;
                        if shell.bytecode.wanted == Some(key) {
                            shell.bytecode.shown = Some(Listing {
                                script,
                                key,
                                lines: out.map(|text| Rc::new(split_lines(&text))),
                            });
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }

        let dock = &self.bytecode;
        let listing = active.and_then(|r| {
            dock.shown
                .as_ref()
                .filter(|l| l.script == r)
                .map(|l| l.lines.clone())
        });
        let current = match (&dock.shown, dock.wanted) {
            (Some(l), Some(key)) => l.key == key,
            _ => false,
        };
        let status = match &listing {
            Some(Ok(lines)) if current => format!("{} lines", lines.len()),
            Some(Err(_)) if current => "Error".to_string(),
            _ if active.is_some() => "Compiling…".to_string(),
            _ => String::new(),
        };
        let copy_lines = match &listing {
            Some(Ok(lines)) => Some(lines.clone()),
            _ => None,
        };

        // Three rows: what to show (and the listing's size, and Copy), how to
        // compile it, and for the native views which CPU to generate for.
        let mut views = h_flex().flex_wrap().gap_1();
        for (i, v) in View::ALL.into_iter().enumerate() {
            views = views.child(
                chip(
                    SharedString::from(format!("bytecode-view-{i}")),
                    v.label(),
                    v == view,
                    None,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.bytecode.view = v;
                    cx.notify();
                })),
            );
        }
        let top = h_flex()
            .w_full()
            .flex_wrap()
            .gap_x_2()
            .gap_y_1()
            .child(views)
            .child(
                div()
                    .ml_auto()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text3())
                    .whitespace_nowrap()
                    .child(status),
            )
            .child(
                chip_frame("bytecode-copy", false, Some("Copy the whole listing"))
                    .child(
                        Icon::new(IconName::Copy)
                            .xsmall()
                            .text_color(tokens::text2()),
                    )
                    .child("Copy")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(lines) = &copy_lines {
                            let joined = lines
                                .iter()
                                .map(|l| l.as_ref())
                                .collect::<Vec<_>>()
                                .join("\n");
                            cx.write_to_clipboard(ClipboardItem::new_string(joined));
                        }
                    })),
            );

        let mut optimize = h_flex().gap_1().child(label("Optimize"));
        for n in 0..=2u8 {
            optimize = optimize.child(
                chip(
                    SharedString::from(format!("bytecode-opt-{n}")),
                    &n.to_string(),
                    options.optimization == n,
                    Some(OPTIMIZE_HINTS[n as usize]),
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.bytecode.options.optimization = n;
                    cx.notify();
                })),
            );
        }
        let mut debug = h_flex().gap_1().child(label("Debug"));
        for n in 0..=2u8 {
            debug = debug.child(
                chip(
                    SharedString::from(format!("bytecode-debug-{n}")),
                    &n.to_string(),
                    options.debug == n,
                    Some(DEBUG_HINTS[n as usize]),
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.bytecode.options.debug = n;
                    cx.notify();
                })),
            );
        }
        let compile = h_flex()
            .w_full()
            .flex_wrap()
            .gap_x_3()
            .gap_y_1()
            .child(optimize)
            .child(debug)
            .child(
                toggle(
                    "bytecode-roblox",
                    "Roblox",
                    options.roblox,
                    "Compile like Roblox: Vector3.new folds to a constant and Roblox \
                     datatypes are known to the native code generator",
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.bytecode.options.roblox = !shell.bytecode.options.roblox;
                    cx.notify();
                })),
            )
            .when(view.is_native(), |this| {
                this.child(
                    toggle(
                        "bytecode-cold",
                        "All functions",
                        options.cold_functions,
                        "Native code for every function, even the ones the code \
                         generator would skip as not worth it",
                    )
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.bytecode.options.cold_functions =
                            !shell.bytecode.options.cold_functions;
                        cx.notify();
                    })),
                )
            });

        let mut header = v_flex()
            .w_full()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(tokens::border())
            .child(top)
            .child(compile);
        if view.is_native() {
            let mut targets = h_flex().w_full().flex_wrap().gap_1().child(label("Target"));
            for (i, t) in Target::ALL.into_iter().enumerate() {
                targets = targets.child(
                    chip(
                        SharedString::from(format!("bytecode-target-{i}")),
                        t.label(),
                        options.target == t,
                        None,
                    )
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.bytecode.options.target = t;
                        cx.notify();
                    })),
                );
            }
            header = header.child(targets);
        }

        let content = match (active, listing) {
            (None, _) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(tokens::text_xs())
                .text_color(tokens::text_muted())
                .child("Open a script to see what Luau compiles it to")
                .into_any_element(),
            (Some(_), None) => div().flex_1().into_any_element(),
            (Some(_), Some(Err(message))) => div()
                .id("bytecode-error")
                .flex_1()
                .overflow_y_scroll()
                .p_2()
                .font_family(tokens::font_mono())
                .text_size(tokens::text_xs())
                .text_color(tokens::text_error())
                .child(message)
                .into_any_element(),
            (Some(script), Some(Ok(lines))) => {
                let count = lines.len();
                uniform_list(
                    "bytecode-lines",
                    count,
                    cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|ix| {
                                let line = lines[ix].clone();
                                let row = classify(&line);
                                let color = match row {
                                    Row::Function => tokens::text(),
                                    Row::Source(_) => tokens::text_muted(),
                                    Row::Remark => tokens::warning(),
                                    Row::Local => tokens::output_info(),
                                    Row::Other => tokens::text(),
                                };
                                let base = div()
                                    .id(ix)
                                    .px_2()
                                    .whitespace_nowrap()
                                    .font_family(tokens::font_mono())
                                    .text_size(tokens::text_xs())
                                    .text_color(color)
                                    .when(row == Row::Function, |this| {
                                        this.font_weight(FontWeight::BOLD)
                                    })
                                    .child(line);
                                match row {
                                    Row::Source(number) => base
                                        .cursor_pointer()
                                        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                                        .on_click(cx.listener(move |shell, _, window, cx| {
                                            shell.activate_script(script, cx);
                                            if let Some(open) = shell.scripts.open.get(&script) {
                                                open.state.update(cx, |state, cx| {
                                                    let text = state.value().to_string();
                                                    let start = line_start(&text, number);
                                                    state.set_selected_range(start..start, cx);
                                                    state.focus(window, cx);
                                                });
                                            }
                                            // The status bar's Ln/Col is the
                                            // shell's to redraw, not the editor's.
                                            cx.notify();
                                        }))
                                        .into_any_element(),
                                    _ => base.into_any_element(),
                                }
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.bytecode.scroll)
                .flex_1()
                .into_any_element()
            }
        };

        let body = v_flex().size_full().child(header).child(content);
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }
    /// `RBX_STUDIO_BYTECODE=1|<view>[,<target>]`: see the crate's doc
    /// comment.
    pub(super) fn apply_debug_bytecode(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var("RBX_STUDIO_BYTECODE") else {
            return;
        };
        self.set_panel_open(Panel::Bytecode, true, cx);
        let mut parts = spec.split(',').map(str::trim);
        let named = |label: &str, wanted: &str| label.eq_ignore_ascii_case(wanted);
        if let Some(view) = parts.next() {
            if let Some(view) = View::ALL.into_iter().find(|v| named(v.label(), view)) {
                self.bytecode.view = view;
            }
        }
        if let Some(target) = parts.next() {
            if let Some(target) = Target::ALL.into_iter().find(|t| named(t.label(), target)) {
                self.bytecode.options.target = target;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{classify, line_start, source_line_number, split_lines, Row};

    #[test]
    fn classifies_lines() {
        assert_eq!(classify("Function 3 (name):"), Row::Function);
        assert_eq!(classify("    12: local x = 1"), Row::Source(12));
        assert_eq!(classify("REMARK builtin"), Row::Remark);
        assert_eq!(classify("R0: number [argument]"), Row::Local);
        assert_eq!(classify("LOADN R0 1"), Row::Other);
        assert_eq!(classify(""), Row::Other);
    }

    #[test]
    fn parses_source_line_numbers() {
        assert_eq!(source_line_number("    7: x"), Some(7));
        assert_eq!(source_line_number("7: x"), None);
        assert_eq!(source_line_number("    x: y"), None);
        assert_eq!(source_line_number("    12"), None);
    }

    #[test]
    fn finds_line_starts() {
        let text = "a\nbb\nccc";
        assert_eq!(line_start(text, 0), 0);
        assert_eq!(line_start(text, 1), 0);
        assert_eq!(line_start(text, 2), 2);
        assert_eq!(line_start(text, 3), 5);
        assert_eq!(line_start(text, 9), text.len());
    }

    #[test]
    fn expands_tabs() {
        assert_eq!(split_lines("a\tb\nc")[0].as_ref(), "a    b");
    }
}
