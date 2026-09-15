//! The Output panel: Studio's own scrollback of every Command Bar run, kept
//! around after the Command Bar's own one-line label (see
//! `command_bar::Feedback`) is overwritten by the next run.
//!
//! `shell::command::run_command` appends one [`OutputEntry`] per run, success
//! or failure, to the [`OutputLog`] `Shell` owns; this module renders that
//! log as the Output panel's body plus its title-bar controls (filter, Clear)
//! — see `shell::dock`'s `Section::Output` for how the panel itself is wired
//! into the dock.
//!
//! Clicking a logged entry recalls its source back into the Command Bar
//! (see [`Shell::recall_command`]) instead of Studio's own Up/Down-through-history:
//! the vendored `InputState` (see `gpui-base`) binds `up`/`down` to cursor
//! movement in its own key context already, so intercepting them from outside
//! without breaking ordinary editing would mean fighting that binding rather
//! than working with it. A clickable list gets the same "browse and recall"
//! behaviour without the fight.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Selectable as _, Sizable};
use gpui_kit::*;

use crate::command_bar::Feedback;

use super::Shell;

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
}

impl OutputEntry {
    fn new(source: &str, feedback: Feedback) -> Self {
        OutputEntry {
            source: source.to_string(),
            feedback,
        }
    }

    pub(crate) fn is_error(&self) -> bool {
        self.feedback.is_error()
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    fn truncated_source(&self) -> SharedString {
        truncate(&self.source, SOURCE_MAX_LEN)
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
                rows = rows.child(output_row(entry, index, cx));
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

/// One row of the log: a coloured marker for success/error, the truncated
/// source that was run, and the result label — clicking anywhere on the row
/// recalls that source into the Command Bar.
fn output_row(entry: &OutputEntry, index: usize, cx: &mut Context<Shell>) -> impl IntoElement {
    let color = if entry.is_error() {
        cx.theme().danger
    } else {
        cx.theme().success
    };
    let marker = if entry.is_error() { "✕" } else { "✓" };
    let source = entry.source().to_string();

    h_flex()
        .id(format!("output-entry-{index}"))
        .w_full()
        .gap_2()
        .px_2()
        .py_1()
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.recall_command(&source, window, cx);
        }))
        .child(div().text_xs().text_color(color).child(marker))
        .child(div().flex_1().text_xs().child(entry.truncated_source()))
        .child(
            div()
                .text_xs()
                .text_color(color)
                .child(entry.feedback.label()),
        )
}

#[cfg(test)]
mod tests {
    // Not `use super::*;`: the parent module's `use gpui_kit::*;` re-exports
    // `gpui::test`, which would then shadow `std`'s `#[test]` here and expand
    // every plain test below through GPUI's randomized-test machinery instead
    // — see `shell::dock`'s own test module for the same guard.
    use super::{OutputEntry, OutputFilter, OutputLog, CAP, SOURCE_MAX_LEN};
    use crate::command_bar::Feedback;

    fn output(text: &str) -> Feedback {
        Feedback::from_run(Ok(vec![text.to_string()]))
    }

    fn error(text: &str) -> Feedback {
        Feedback::from_run(Err(text.to_string()))
    }

    #[test]
    fn a_fresh_log_is_empty() {
        let log = OutputLog::default();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn pushed_entries_come_back_oldest_first() {
        let mut log = OutputLog::default();
        log.push("print(1)", output("1"));
        log.push("print(2)", output("2"));

        let sources: Vec<&str> = log
            .filtered(OutputFilter::All)
            .map(|e| e.source())
            .collect();
        assert_eq!(sources, vec!["print(1)", "print(2)"]);
    }

    #[test]
    fn clear_empties_the_log() {
        let mut log = OutputLog::default();
        log.push("print(1)", output("1"));
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn pushing_past_the_cap_drops_the_oldest_entry() {
        let mut log = OutputLog::default();
        for i in 0..CAP + 5 {
            log.push(&format!("print({i})"), output("x"));
        }

        assert_eq!(log.len(), CAP);
        let first = log.filtered(OutputFilter::All).next().unwrap();
        // The five oldest (0..5) should have been dropped to stay at CAP.
        assert_eq!(first.source(), "print(5)");
    }

    #[test]
    fn filter_output_only_hides_errors() {
        let mut log = OutputLog::default();
        log.push("ok", output("done"));
        log.push("bad", error("boom"));

        let sources: Vec<&str> = log
            .filtered(OutputFilter::Output)
            .map(|e| e.source())
            .collect();
        assert_eq!(sources, vec!["ok"]);
    }

    #[test]
    fn filter_errors_only_hides_output() {
        let mut log = OutputLog::default();
        log.push("ok", output("done"));
        log.push("bad", error("boom"));

        let sources: Vec<&str> = log
            .filtered(OutputFilter::Errors)
            .map(|e| e.source())
            .collect();
        assert_eq!(sources, vec!["bad"]);
    }

    #[test]
    fn filter_all_shows_everything() {
        let mut log = OutputLog::default();
        log.push("ok", output("done"));
        log.push("bad", error("boom"));

        assert_eq!(log.filtered(OutputFilter::All).count(), 2);
    }

    #[test]
    fn an_entry_reports_its_own_error_state() {
        let ok_entry = OutputEntry::new("ok", output("done"));
        let err_entry = OutputEntry::new("bad", error("boom"));
        assert!(!ok_entry.is_error());
        assert!(err_entry.is_error());
    }

    #[test]
    fn a_long_source_is_truncated_with_an_ellipsis() {
        let entry = OutputEntry::new(&"x".repeat(SOURCE_MAX_LEN + 20), output("done"));
        let label = entry.truncated_source();
        assert_eq!(label.chars().count(), SOURCE_MAX_LEN + 1);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn filter_default_is_all() {
        assert_eq!(OutputFilter::default(), OutputFilter::All);
    }

    #[test]
    fn a_pushed_warning_is_not_treated_as_an_error() {
        let mut log = OutputLog::default();
        log.push_warning("asset 1: fetching asset 1 failed");

        let entry = log.filtered(OutputFilter::All).next().unwrap();
        assert!(!entry.is_error());
    }

    #[test]
    fn a_warning_shows_under_all_and_output_but_never_errors() {
        let mut log = OutputLog::default();
        log.push_warning("boom");

        assert_eq!(log.filtered(OutputFilter::All).count(), 1);
        assert_eq!(log.filtered(OutputFilter::Output).count(), 1);
        assert_eq!(log.filtered(OutputFilter::Errors).count(), 0);
    }

    #[test]
    fn pushing_warnings_past_the_cap_drops_the_oldest() {
        let mut log = OutputLog::default();
        for i in 0..CAP + 5 {
            log.push_warning(&format!("warning {i}"));
        }

        assert_eq!(log.len(), CAP);
    }
}
