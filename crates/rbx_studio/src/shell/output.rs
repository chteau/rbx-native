//! The Output panel: Studio's own scrollback of every Command Bar run, kept
//! around after the Command Bar's own one-line label (see
//! `command_bar::Feedback`) is overwritten by the next run.
//!
//! `shell::command::run_command` appends one [`OutputEntry`] per run, success
//! or failure, to the [`OutputLog`] `Shell` owns; this module renders that
//! log as the Output panel's body plus its title-bar controls (filter, Clear)
//! — see `shell::dock`'s `Section::Output` for how the panel itself is wired
//! into the dock, including its "Show Timestamp" toggle
//! (`Shell::output_show_timestamps`), which lives in that overflow menu
//! alongside Explorer's and Viewport's own toggles rather than in this
//! panel's own title-bar row. Each row's icon and color come from its
//! `row_kind::RowKind` — a plain `✕`/`✓` marker used to be the only
//! distinction between error and everything else; now `print`/success,
//! `warn` and `error` each read distinctly, matching real Studio's Output
//! window.
//!
//! Clicking a logged entry recalls its source back into the Command Bar
//! (see [`Shell::recall_command`]) instead of Studio's own Up/Down-through-history:
//! the vendored `InputState` (see `gpui-base`) binds `up`/`down` to cursor
//! movement in its own key context already, so intercepting them from outside
//! without breaking ordinary editing would mean fighting that binding rather
//! than working with it. A clickable list gets the same "browse and recall"
//! behaviour without the fight.

use std::time::SystemTime;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Selectable as _, Sizable};
use gpui_kit::*;

use crate::command_bar::Feedback;

use row_kind::{format_timestamp, RowColor, RowKind};

use super::Shell;

mod row_kind;

/// How many runs the log keeps before dropping the oldest. Unlike
/// `History`'s 50 (whole-DOM snapshots, genuinely expensive to keep many of),
/// an entry here is two short strings — a much higher cap costs nothing, so
/// this is an order of magnitude up rather than matched exactly.
pub(crate) const CAP: usize = 200;

/// Past this many characters a logged command's source is cut with an
/// ellipsis. Shorter than `command_bar::feedback`'s own 200: here the source
/// is a supporting label next to the result, not the thing being read.
const SOURCE_MAX_LEN: usize = 80;

/// The `source` every [`OutputLog::push_warning`] entry carries — there is no
/// command behind a warning the way there is behind a Command Bar run, so
/// this is what shows in the row's source column instead.
const WARNING_SOURCE: &str = "warning";

/// One run's worth of history: what was typed and what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputEntry {
    source: String,
    feedback: Feedback,
    /// Captured unconditionally at [`OutputEntry::new`] time — cheap to keep
    /// on every entry — but only ever painted when the Output panel's own
    /// "Show Timestamp" toggle (`Shell::output_show_timestamps`) is on; see
    /// [`OutputEntry::timestamp_label`].
    timestamp: SystemTime,
}

impl OutputEntry {
    fn new(source: &str, feedback: Feedback) -> Self {
        OutputEntry {
            source: source.to_string(),
            feedback,
            timestamp: SystemTime::now(),
        }
    }

    pub(crate) fn is_error(&self) -> bool {
        self.feedback.is_error()
    }

    /// Which color/icon family this row paints with — see `row_kind::RowKind`.
    pub(crate) fn kind(&self) -> RowKind {
        RowKind::of(&self.feedback)
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    fn truncated_source(&self) -> SharedString {
        truncate(&self.source, SOURCE_MAX_LEN)
    }

    fn timestamp_label(&self) -> SharedString {
        format_timestamp(self.timestamp)
    }
}

/// Collapses embedded newlines into one row, then cuts to `max_len`
/// characters — the same shape as `command_bar::feedback::truncate`, kept
/// separate since the two truncate to different lengths for different
/// reasons (see that function's own doc comment).
fn truncate(text: &str, max_len: usize) -> SharedString {
    let flat = text.replace('\n', "  ");
    match flat.char_indices().nth(max_len) {
        Some((cut, _)) => SharedString::from(format!("{}…", &flat[..cut])),
        None => SharedString::from(flat),
    }
}

/// Which entries [`OutputLog::filtered`] shows. Only two outcomes exist today
/// — `rbx_lua::Runtime::run` returns `Result<Vec<String>, String>`, nothing
/// else is distinguishable (see `command_bar::run`) — so this is an honest
/// two-way filter plus "no filter", not a three-way one with a "warnings"
/// bucket nothing would ever populate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum OutputFilter {
    #[default]
    All,
    Output,
    Errors,
}

impl OutputFilter {
    fn matches(self, entry: &OutputEntry) -> bool {
        match self {
            OutputFilter::All => true,
            OutputFilter::Output => !entry.is_error(),
            OutputFilter::Errors => entry.is_error(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            OutputFilter::All => "All",
            OutputFilter::Output => "Output",
            OutputFilter::Errors => "Errors",
        }
    }
}

/// A bounded, append-only log of Command Bar runs — oldest first, newest at
/// the bottom, the order Studio's own Output window scrolls in.
#[derive(Default)]
pub(crate) struct OutputLog {
    entries: Vec<OutputEntry>,
}

impl OutputLog {
    /// Appends one run, dropping the oldest entry first if the log is
    /// already at [`CAP`] — same drop-oldest shape as `History::push`.
    pub(crate) fn push(&mut self, source: &str, feedback: Feedback) {
        if self.entries.len() >= CAP {
            self.entries.remove(0);
        }
        self.entries.push(OutputEntry::new(source, feedback));
    }

    /// Appends one warning from somewhere other than a Command Bar run (an
    /// asset-fetch/decode failure, a texture that fell back to a default) —
    /// same drop-oldest-at-[`CAP`] shape as [`OutputLog::push`], which this
    /// is not: `OutputFilter::Errors` never catches it (see
    /// `Feedback::Warning::is_error`), so it stays visible under "All" and
    /// "Output" instead of quietly vanishing into a filtered view.
    pub(crate) fn push_warning(&mut self, message: &str) {
        self.push(WARNING_SOURCE, Feedback::Warning(message.to_string()));
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn filtered(&self, filter: OutputFilter) -> impl Iterator<Item = &OutputEntry> {
        self.entries
            .iter()
            .filter(move |entry| filter.matches(entry))
    }
}

impl Shell {
    /// Recalls a past run's source back into the Command Bar's input, focused
    /// and ready to edit or re-run — the click-to-recall alternative to
    /// Studio's own Up/Down-through-history (see this module's doc comment
    /// for why the Output panel takes that approach instead).
    pub(super) fn recall_command(
        &mut self,
        source: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.command_bar.input().update(cx, |input, cx| {
            input.set_value(source, window, cx);
        });
        let handle = self.command_bar.input().read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// The title-bar controls for the Output tab (see `shell::dock`'s
    /// `title_suffix`): the level filter and the Clear button, in the same
    /// spot the Viewport tab's graphics-quality dropdown lives.
    pub(super) fn output_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.output_filter;
        h_flex()
            .gap_1()
            .px_1()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} runs", self.output.len())),
            )
            .children(
                [
                    OutputFilter::All,
                    OutputFilter::Output,
                    OutputFilter::Errors,
                ]
                .map(|level| {
                    Button::new(("output-filter", level as usize))
                        .label(level.label())
                        .xsmall()
                        .selected(current == level)
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.output_filter = level;
                            cx.notify();
                        }))
                }),
            )
            .child(
                Button::new("output-clear")
                    .label("Clear")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.output.clear();
                        cx.notify();
                    })),
            )
    }

    /// The Output panel's body: the filtered log, oldest first, each row
    /// clickable to recall its source (see [`Shell::recall_command`]).
    pub(super) fn output_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let filter = self.output_filter;
        let show_timestamp = self.output_show_timestamps;
        let entries: Vec<&OutputEntry> = self.output.filtered(filter).collect();

        let list = if entries.is_empty() {
            v_flex().flex_1().p_2().child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.output.is_empty() {
                        "Nothing run yet."
                    } else {
                        "No entries match this filter."
                    }),
            )
        } else {
            let mut rows = v_flex().flex_1();
            for (index, entry) in entries.into_iter().enumerate() {
                rows = rows.child(output_row(entry, index, show_timestamp, cx));
            }
            rows
        };

        v_flex()
            .size_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .id("output-log")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.output_scroll)
                    .child(list)
                    .vertical_scrollbar(&self.output_scroll),
            )
    }
}

/// One row of the log: a per-kind icon (see `row_kind::RowKind`), optionally
/// that entry's timestamp, the truncated source that was run, and the result
/// label — clicking anywhere on the row recalls that source into the Command
/// Bar.
fn output_row(
    entry: &OutputEntry,
    index: usize,
    show_timestamp: bool,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let kind = entry.kind();
    let color = match kind.color() {
        RowColor::Default => None,
        RowColor::Warning => Some(cx.theme().warning),
        RowColor::Danger => Some(cx.theme().danger),
    };
    let icon = Icon::new(kind.icon()).small();
    let icon = match color {
        Some(color) => icon.text_color(color),
        None => icon,
    };
    let source = entry.source().to_string();

    let mut row = h_flex()
        .id(format!("output-entry-{index}"))
        .w_full()
        .gap_2()
        .px_2()
        .py_1()
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.recall_command(&source, window, cx);
        }))
        .child(icon);

    if show_timestamp {
        row = row.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(entry.timestamp_label()),
        );
    }

    let mut result_label = div().text_xs().child(entry.feedback.label());
    if let Some(color) = color {
        result_label = result_label.text_color(color);
    }

    row.child(div().flex_1().text_xs().child(entry.truncated_source()))
        .child(result_label)
}

#[cfg(test)]
mod tests;
