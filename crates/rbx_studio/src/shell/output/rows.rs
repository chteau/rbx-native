//! Drawing one Output row: a Command Bar run or app warning
//! ([`output_row`]) or a message from the running game ([`game_row`]).

use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::*;

use crate::settings::OutputDisplay;
use crate::tokens;

use super::row_kind::RowColor;
use super::{game, tables, truncate, OutputEntry, OutputLog, Shell, LABEL_MAX_LEN};

/// What the panel's display toggles say every row shows.
#[derive(Debug, Clone, Copy)]
pub(super) struct Show {
    pub(super) timestamp: bool,
    pub(super) display: OutputDisplay,
}

/// The colour a row's level paints with, `None` for the default text.
fn level_color(color: RowColor, cx: &App) -> Option<Hsla> {
    match color {
        RowColor::Default => None,
        RowColor::Info => Some(tokens::output_info().into()),
        RowColor::Warning => Some(cx.theme().warning),
        RowColor::Danger => Some(cx.theme().danger),
    }
}

/// One row of the log: a per-kind icon (see `row_kind::RowKind`), optionally
/// that entry's timestamp, the truncated source that was run, and the result
/// label — clicking anywhere on the row recalls that source into the Command
/// Bar. A run that printed a table draws its `print` lines with each table
/// as an expandable tree instead of the flat label, unless Log Mode is on.
pub(super) fn output_row(
    entry: &OutputEntry,
    index: usize,
    show: Show,
    log: &OutputLog,
    cx: &mut Context<Shell>,
) -> AnyElement {
    if let Some(message) = &entry.game {
        return game_row(entry, message, index, show, log, cx).into_any_element();
    }
    let kind = entry.kind();
    let color = level_color(kind.color(), cx);
    let icon = Icon::new(kind.icon()).small();
    let icon = match color {
        Some(color) => icon.text_color(color),
        None => icon,
    };
    let source = entry.source().to_string();

    let mut row = h_flex()
        .id(format!("output-entry-{index}"))
        .w_full()
        .items_start()
        .gap_2()
        .px_2()
        .py_1()
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.recall_command(&source, window, cx);
        }))
        // Top-aligned so a wrapped message keeps its icon and source on its
        // first line. The zero-width space makes the icon's box one text
        // line tall, centring it on that line as `items_center` did on a
        // single-line row.
        .child(h_flex().flex_none().text_xs().child(icon).child("\u{200B}"));

    if show.timestamp {
        row = row.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(entry.timestamp_label()),
        );
    }

    // `min_w_0` is what wraps a long message instead of cutting it at the
    // panel edge: gpui measures text's min-content width unwrapped, so
    // without it the label refuses to shrink and runs out of the row.
    let mut result_label = if entry.printed.is_empty() || show.display.log_mode {
        div().min_w_0().text_xs().child(entry.label())
    } else {
        div()
            .min_w_0()
            .text_xs()
            .child(tables::printed(entry, show.display, log, cx))
    };
    if let Some(color) = color {
        result_label = result_label.text_color(color);
    }

    row.child(div().flex_1().text_xs().child(entry.truncated_source()))
        .child(result_label)
        .into_any_element()
}

/// A click that opens `location`'s script at its line (`Shell::play_reveal`).
fn reveal(
    context: game::Context,
    location: &game::Location,
    cx: &mut Context<Shell>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let location = location.clone();
    let shell = cx.weak_entity();
    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
        cx.stop_propagation();
        let _ = shell.update(cx, |shell, cx| {
            shell.play_reveal(context, &location, window, cx);
        });
    }
}

/// A Play message (see `game`): the level icon, the Server/Client tag in
/// its context colour (Show Context), the message in the level's colour with
/// the error's stack frames under it, and the error's `Script:line` at the
/// end (Show Source).
/// Clicking the row, its location or a frame opens that script at that line
/// (`Shell::play_reveal`); nothing is recalled into the Command Bar.
fn game_row(
    entry: &OutputEntry,
    message: &game::GameMessage,
    index: usize,
    show: Show,
    log: &OutputLog,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let kind = entry.kind();
    let color = level_color(kind.color(), cx);
    let icon = Icon::new(kind.icon()).small();
    let icon = match color {
        Some(color) => icon.text_color(color),
        None => icon,
    };
    let context = message.context;
    let tag_color = match context {
        game::Context::Server => tokens::output_server(),
        game::Context::Client => tokens::output_client(),
    };

    let mut row = h_flex()
        .id(format!("output-entry-{index}"))
        .w_full()
        .items_start()
        .gap_2()
        .px_2()
        .py_1()
        .child(h_flex().flex_none().text_xs().child(icon).child("\u{200B}"));
    if let Some(location) = &message.location {
        row = row.cursor_pointer().on_click(reveal(context, location, cx));
    }
    if show.timestamp {
        row = row.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(entry.timestamp_label()),
        );
    }
    if show.display.context {
        row = row.child(
            div()
                .flex_none()
                .w(px(44.))
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tag_color)
                .child(context.label()),
        );
    }

    let mut text = div().min_w_0().text_xs();
    text = if entry.printed.is_empty() || show.display.log_mode {
        text.child(truncate(&message.text, LABEL_MAX_LEN))
    } else {
        text.child(tables::printed(entry, show.display, log, cx))
    };
    if let Some(color) = color {
        text = text.text_color(color);
    }
    let mut body = v_flex().flex_1().min_w_0().gap_0p5().child(text);
    for (frame_index, frame) in message.frames.iter().enumerate() {
        let line = format!(
            "{}, line {}{}{}",
            frame.location.path,
            frame.location.line,
            if frame.detail.is_empty() { "" } else { " " },
            frame.detail
        );
        body = body.child(
            div()
                .id(format!("output-entry-{index}-frame-{frame_index}"))
                .min_w_0()
                .pl_3()
                .text_xs()
                .text_color(tokens::text_muted())
                .cursor_pointer()
                .hover(|this| this.text_color(tag_color))
                .on_click(reveal(context, &frame.location, cx))
                .child(line),
        );
    }
    row = row.child(body);
    if let Some(location) = message.location.as_ref().filter(|_| show.display.source) {
        row = row.child(
            div()
                .id(format!("output-entry-{index}-location"))
                .flex_none()
                .text_xs()
                .text_color(tokens::text_muted())
                .cursor_pointer()
                .hover(|this| this.text_color(tag_color).underline())
                .on_click(reveal(context, location, cx))
                .child(location.short()),
        );
    }
    row
}
