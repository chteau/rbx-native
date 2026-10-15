//! The Output panel: Studio's own scrollback of every Command Bar run, kept
//! around after the Command Bar's own one-line label (see
//! `command_bar::Feedback`) is overwritten by the next run.
//!
//! `shell::command::run_command` appends one [`OutputEntry`] per run, success
//! or failure, to the [`OutputLog`] `Shell` owns; this module renders that
//! log as the Output panel's body plus its strip controls (search, filter,
//! Clear). `shell::workspace` wires the panel into its dock, overflow menu
//! and "Show Timestamp" toggle included. Each row's icon and colour come from
//! its `row_kind::RowKind`, so `print`/success, `warn` and `error` each read
//! distinctly, as in Studio's own Output window.
//!
//! Clicking a logged entry recalls its source back into the Command Bar
//! (see [`Shell::recall_command`]) instead of Studio's own Up/Down-through-history:
//! the vendored `InputState` (see `gpui-base`) binds `up`/`down` to cursor
//! movement in its own key context already, so intercepting them from outside
//! without breaking ordinary editing would mean fighting that binding rather
//! than working with it. A clickable list gets the same "browse and recall"
//! behaviour without the fight.

use std::collections::HashSet;
use std::time::SystemTime;

use rbx_lua::PrintPart;

use crate::shell::chrome::ScrollbarY as _;
use gpui_kit::component::v_flex;
use gpui_kit::*;

use crate::tokens;

use crate::command_bar::Feedback;

use row_kind::{format_timestamp, RowKind};

use super::Shell;

mod controls;
mod filter;
pub(crate) mod game;
mod row_kind;
mod rows;
mod tables;

pub(crate) use filter::{ContextFilter, OutputFilter};

/// How many runs the log keeps before dropping the oldest. Unlike
/// `History`'s 50 (whole-DOM snapshots, genuinely expensive to keep many of),
/// an entry here is two short strings — a much higher cap costs nothing, so
/// this is an order of magnitude up rather than matched exactly.
pub(crate) const CAP: usize = 200;

/// Past this many characters a logged command's source is cut with an
/// ellipsis. Shorter than `command_bar::feedback`'s own 200: here the source
/// is a supporting label next to the result, not the thing being read.
const SOURCE_MAX_LEN: usize = 80;

/// Past this many characters a row's result is cut. Rows wrap, so this is
/// far above the Command Bar label's 200 — a path in a warning has to fit —
/// but still a cap: a `print` loop's whole output would otherwise wrap into
/// one row hundreds of lines tall.
const LABEL_MAX_LEN: usize = 2000;

/// The `source` every [`OutputLog::push_warning`] entry carries — there is no
/// command behind a warning the way there is behind a Command Bar run, so
/// this is what shows in the row's source column instead.
const WARNING_SOURCE: &str = "warning";

/// The narrowest the Output search box may get as the dock shrinks — it
/// otherwise takes all the width the strip has left.
const SEARCH_MIN_WIDTH: f32 = 140.0;

/// One run's worth of history: what was typed and what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputEntry {
    /// Unique for the log's lifetime, unlike a position that shifts as the
    /// oldest entries drop: what a table's open/closed state is keyed by.
    id: u64,
    source: String,
    feedback: Feedback,
    /// Captured unconditionally at [`OutputEntry::new`] time — cheap to keep
    /// on every entry — but only ever painted when the Output panel's own
    /// "Show Timestamp" toggle (`Shell::output_show_timestamps`) is on; see
    /// [`OutputEntry::timestamp_label`].
    timestamp: SystemTime,
    /// Set for a message from the running game during Play (see `game`):
    /// such a row shows its Server/Client tag and, for an error, a link to
    /// where it happened instead of a Command Bar source.
    game: Option<game::GameMessage>,
    /// A Command Bar run's `print` calls with their tables kept as trees
    /// (see `tables`); empty when nothing it printed was a table.
    printed: Vec<Vec<PrintPart>>,
    /// Said by a plugin (see `shell::plugins`): Studio's **User Plugin**
    /// context.
    plugin: bool,
}

impl OutputEntry {
    fn new(id: u64, source: &str, feedback: Feedback) -> Self {
        OutputEntry {
            id,
            source: source.to_string(),
            feedback,
            timestamp: SystemTime::now(),
            game: None,
            printed: Vec::new(),
            plugin: false,
        }
    }

    pub(crate) fn from_plugin(&self) -> bool {
        self.plugin
    }

    /// Where the message came from: the game's side during Play, `None` for
    /// everything said in edit mode.
    pub(crate) fn context(&self) -> Option<game::Context> {
        self.game.as_ref().map(|message| message.context)
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

    /// Whether this entry answers a free-text search, matched against both
    /// halves of what the row actually shows — the command that was run and
    /// the result beside it. Case-insensitive, because nobody searching a log
    /// for `attempt to index nil` types it the way the error did.
    ///
    /// An empty query matches everything rather than nothing: the box is a
    /// narrowing on top of the level filter, not a second thing to satisfy.
    fn matches_query(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let query = query.to_lowercase();
        self.source.to_lowercase().contains(&query) || self.label().to_lowercase().contains(&query)
    }

    fn label(&self) -> SharedString {
        self.feedback.label_within(LABEL_MAX_LEN)
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

/// A bounded, append-only log of Command Bar runs — oldest first, newest at
/// the bottom, the order Studio's own Output window scrolls in.
#[derive(Default)]
pub(crate) struct OutputLog {
    entries: Vec<OutputEntry>,
    next_id: u64,
    /// Printed tables whose open state is the opposite of the default (see
    /// `OutputDisplay::tables_expanded`), as (entry id, path of entry
    /// indices from the printed table down). Flipping the default then
    /// flips every untouched table and keeps what was clicked clicked.
    flipped: HashSet<(u64, Vec<u32>)>,
}

impl OutputLog {
    /// Appends one run, dropping the oldest entry first if the log is
    /// already at [`CAP`] — same drop-oldest shape as `History::push`.
    pub(crate) fn push(&mut self, source: &str, feedback: Feedback) {
        if self.entries.len() >= CAP {
            let dropped = self.entries.remove(0).id;
            self.flipped.retain(|(id, _)| *id != dropped);
        }
        self.entries
            .push(OutputEntry::new(self.next_id, source, feedback));
        self.next_id += 1;
    }

    /// [`OutputLog::push`] for a line a plugin printed, `source` being the
    /// plugin's name.
    pub(crate) fn push_plugin(&mut self, source: &str, feedback: Feedback) {
        self.push(source, feedback);
        if let Some(entry) = self.entries.last_mut() {
            entry.plugin = true;
        }
    }

    /// [`OutputLog::push`] for a Command Bar run, keeping its `print` calls'
    /// structure when one of them printed a table.
    pub(crate) fn push_run(
        &mut self,
        source: &str,
        feedback: Feedback,
        printed: Vec<Vec<PrintPart>>,
    ) {
        self.push(source, feedback);
        let has_table = printed
            .iter()
            .flatten()
            .any(|part| matches!(part, PrintPart::Table(_)));
        if let (true, Some(entry)) = (has_table, self.entries.last_mut()) {
            entry.printed = printed;
        }
    }

    /// Whether the printed table at `path` in entry `id` is open, given
    /// whether tables open by default.
    pub(crate) fn table_open(&self, id: u64, path: &[u32], by_default: bool) -> bool {
        by_default != self.flipped.contains(&(id, path.to_vec()))
    }

    pub(crate) fn toggle_table(&mut self, id: u64, path: Vec<u32>) {
        let key = (id, path);
        if !self.flipped.remove(&key) {
            self.flipped.insert(key);
        }
    }

    /// [`OutputLog::push`], unless the newest entry is already this very one
    /// from the same source, pushed within the last half second: one key
    /// press that two handlers both act on is still one row.
    pub(crate) fn push_once(&mut self, source: &str, feedback: Feedback) {
        let repeat = self.entries.last().is_some_and(|last| {
            last.source == source
                && last.feedback == feedback
                && last
                    .timestamp
                    .elapsed()
                    .is_ok_and(|age| age < std::time::Duration::from_millis(500))
        });
        if !repeat {
            self.push(source, feedback);
        }
    }

    /// Appends a message from the running game (`print`/`warn`/an error):
    /// `feedback` carries its level and Roblox's whole message, stack
    /// included; `game::parse` splits it for the row.
    pub(crate) fn push_game(&mut self, context: game::Context, feedback: Feedback) {
        let text = match &feedback {
            Feedback::Output(text)
            | Feedback::Info(text)
            | Feedback::Error(text)
            | Feedback::Warning(text) => text,
            Feedback::Idle => return,
        };
        let message = game::parse(context, text);
        self.push(context.label(), feedback);
        if let Some(entry) = self.entries.last_mut() {
            entry.game = Some(message);
        }
    }

    /// Appends one warning from somewhere other than a Command Bar run (an
    /// asset-fetch/decode failure, a texture that fell back to a default) —
    /// same drop-oldest-at-[`CAP`] shape as [`OutputLog::push`], which this
    /// is not: it is neither a run's output nor an error, and
    /// [`OutputFilter::Warnings`] is the bucket that owns it.
    pub(crate) fn push_warning(&mut self, message: &str) {
        self.push(WARNING_SOURCE, Feedback::Warning(message.to_string()));
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.flipped.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The entries the panel draws: the level and context filters, then the
    /// search box's text narrowing what survives them. `query` is taken already trimmed —
    /// the caller reads it off an `InputState`, where a trailing space is a
    /// keystroke in progress rather than part of what is being looked for.
    pub(crate) fn filtered<'a>(
        &'a self,
        filter: OutputFilter,
        context: ContextFilter,
        query: &'a str,
    ) -> impl Iterator<Item = &'a OutputEntry> {
        self.entries.iter().filter(move |entry| {
            filter.matches(entry) && context.matches(entry) && entry.matches_query(query)
        })
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

    pub(super) fn output_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let filter = self.output_filter;
        let show = rows::Show {
            timestamp: self.output_show_timestamps,
            display: self.output_display,
        };
        let query = self.output_search.read(cx).value().trim().to_lowercase();
        let entries: Vec<&OutputEntry> = self
            .output
            .filtered(filter, self.output_context, &query)
            .collect();

        let list = if entries.is_empty() {
            v_flex().flex_1().p_2().child(
                div()
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text_placeholder())
                    .child(if self.output.is_empty() {
                        "Nothing run yet."
                    } else {
                        "No entries match this filter."
                    }),
            )
        } else {
            let mut rows = v_flex().flex_1();
            for (index, entry) in entries.into_iter().enumerate() {
                rows = rows.child(rows::output_row(entry, index, show, &self.output, cx));
            }
            rows
        };

        v_flex().size_full().child(
            div()
                .id("output-log")
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.output_scroll)
                .pr(super::chrome::SCROLLBAR_GUTTER)
                .child(list)
                .scrollbar_y(&self.output_scroll),
        )
    }
}

#[cfg(test)]
mod tests;
