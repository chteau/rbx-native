//! Script debugging: the breakpoints set in the Script Editor's gutter, and
//! the session that runs a script with them (see [`session`]).
//!
//! Breakpoints live in the editor session only, never in the place file —
//! Studio keeps them out of the saved place too — so closing the editor
//! forgets them.

pub(crate) mod session;

use std::collections::{BTreeMap, HashMap};

use rbx_dom::Ref;
use rbx_lua::Breakpoint;

/// The four ways Studio's gutter menu inserts a breakpoint. They differ
/// only in how the breakpoint starts out; any of them can be edited into
/// any other afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Standard,
    Conditional,
    Logpoint,
    Temporary,
}

/// One breakpoint as the editor keeps it: what the debugger runs, plus the
/// two settings that only matter between runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) breakpoint: Breakpoint,
    /// A disabled breakpoint keeps its condition and message but is not
    /// handed to the debugger.
    pub(crate) enabled: bool,
    /// Removed once the run it was set for is over — Studio's "Remove
    /// Breakpoint on Hit", which it describes as removing itself after the
    /// first playtest session.
    pub(crate) temporary: bool,
}

impl Stored {
    pub(crate) fn new(line: u32, kind: Kind) -> Self {
        Stored {
            breakpoint: Breakpoint {
                line,
                // A logpoint starts with an empty message the edit popup
                // asks for; `continue_execution` is what makes it a logpoint.
                continue_execution: kind == Kind::Logpoint,
                ..Breakpoint::default()
            },
            enabled: true,
            temporary: kind == Kind::Temporary,
        }
    }

    /// Which gutter icon it gets.
    pub(crate) fn kind(&self) -> Kind {
        if self.breakpoint.continue_execution {
            Kind::Logpoint
        } else if self.breakpoint.condition.is_some() {
            Kind::Conditional
        } else if self.temporary {
            Kind::Temporary
        } else {
            Kind::Standard
        }
    }
}

/// Every script's breakpoints, keyed by 1-based line.
#[derive(Debug, Default)]
pub(crate) struct Breakpoints {
    scripts: HashMap<Ref, BTreeMap<u32, Stored>>,
}

impl Breakpoints {
    pub(crate) fn get(&self, script: Ref, line: u32) -> Option<&Stored> {
        self.scripts.get(&script)?.get(&line)
    }

    pub(crate) fn of(&self, script: Ref) -> impl Iterator<Item = &Stored> {
        self.scripts
            .get(&script)
            .into_iter()
            .flat_map(BTreeMap::values)
    }

    pub(crate) fn insert(&mut self, script: Ref, line: u32, kind: Kind) {
        self.set(script, Stored::new(line, kind));
    }

    /// Adds or replaces the breakpoint on `stored`'s own line.
    pub(crate) fn set(&mut self, script: Ref, stored: Stored) {
        self.scripts
            .entry(script)
            .or_default()
            .insert(stored.breakpoint.line, stored);
    }

    pub(crate) fn remove(&mut self, script: Ref, line: u32) {
        if let Some(lines) = self.scripts.get_mut(&script) {
            lines.remove(&line);
        }
    }

    /// Clicking a breakpoint's icon: enabled ↔ disabled.
    pub(crate) fn toggle_enabled(&mut self, script: Ref, line: u32) {
        if let Some(stored) = self
            .scripts
            .get_mut(&script)
            .and_then(|lines| lines.get_mut(&line))
        {
            stored.enabled = !stored.enabled;
        }
    }

    /// What a run of `script` is handed: its enabled breakpoints.
    pub(crate) fn for_run(&self, script: Ref) -> Vec<Breakpoint> {
        self.of(script)
            .filter(|stored| stored.enabled)
            .map(|stored| stored.breakpoint.clone())
            .collect()
    }

    /// Moves `script`'s breakpoints with an edit that turned its text from
    /// `old` into `new` (see [`remap_line`]).
    pub(crate) fn follow_edit(&mut self, script: Ref, old: &str, new: &str) {
        let Some(lines) = self.scripts.get_mut(&script) else {
            return;
        };
        if lines.is_empty() {
            return;
        }
        let edit = Edit::between(old, new);
        let mut moved = BTreeMap::new();
        for (line, mut stored) in std::mem::take(lines) {
            if let Some(to) = edit.remap(line) {
                stored.breakpoint.line = to;
                // Two landing on one line: the one that was already there
                // (the lower, since lines are visited in order) stays.
                moved.entry(to).or_insert(stored);
            }
        }
        *lines = moved;
    }

    /// A run of `script` is over: its temporary breakpoints go.
    pub(crate) fn end_run(&mut self, script: Ref) {
        if let Some(lines) = self.scripts.get_mut(&script) {
            lines.retain(|_, stored| !stored.temporary);
        }
    }
}

/// One edit to a script, as the lines it left alone: the run of identical
/// lines at the top and the run at the bottom. Whatever lies between is the
/// edited block.
struct Edit {
    old_lines: usize,
    new_lines: usize,
    prefix: usize,
    suffix: usize,
}

impl Edit {
    fn between(old: &str, new: &str) -> Self {
        let old: Vec<&str> = old.split('\n').collect();
        let new: Vec<&str> = new.split('\n').collect();
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(new[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        Edit {
            old_lines: old.len(),
            new_lines: new.len(),
            prefix,
            suffix,
        }
    }

    /// Where 1-based `line` is after the edit. A line above the edited block
    /// stays put, one below it moves with it; one inside it stays within
    /// what the block became, and goes if the block was deleted outright —
    /// a breakpoint on a deleted line is deleted with it.
    fn remap(&self, line: u32) -> Option<u32> {
        let index = line as usize - 1;
        if index < self.prefix {
            return Some(line);
        }
        if index >= self.old_lines - self.suffix {
            return Some((index + self.new_lines - self.old_lines) as u32 + 1);
        }
        let block_end = self.new_lines - self.suffix;
        (block_end > self.prefix).then(|| index.min(block_end - 1) as u32 + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remap(old: &str, new: &str, line: u32) -> Option<u32> {
        Edit::between(old, new).remap(line)
    }

    #[test]
    fn lines_below_an_insert_move_down_and_lines_above_stay() {
        let old = "a\nb\nc";
        let new = "a\nx\ny\nb\nc";
        assert_eq!(remap(old, new, 1), Some(1));
        assert_eq!(remap(old, new, 2), Some(4));
        assert_eq!(remap(old, new, 3), Some(5));
    }

    #[test]
    fn enter_at_the_end_of_a_line_leaves_its_breakpoint_and_moves_the_rest() {
        // The caret at the end of `b`: `b` is unchanged, a blank line follows.
        let old = "a\nb\nc";
        let new = "a\nb\n\nc";
        assert_eq!(remap(old, new, 2), Some(2));
        assert_eq!(remap(old, new, 3), Some(4));
    }

    #[test]
    fn enter_at_the_start_of_a_line_takes_its_breakpoint_down() {
        let old = "a\nb\nc";
        let new = "a\n\nb\nc";
        assert_eq!(remap(old, new, 2), Some(3));
    }

    #[test]
    fn a_deleted_line_takes_its_breakpoint_and_the_rest_move_up() {
        let old = "a\nb\nc\nd";
        let new = "a\nd";
        assert_eq!(remap(old, new, 2), None);
        assert_eq!(remap(old, new, 3), None);
        assert_eq!(remap(old, new, 4), Some(2));
    }

    #[test]
    fn a_line_edited_in_place_keeps_its_breakpoint() {
        assert_eq!(remap("a\nb\nc", "a\nB!\nc", 2), Some(2));
    }

    #[test]
    fn a_breakpoints_line_follows_the_edit_and_collisions_keep_one() {
        let mut breakpoints = Breakpoints::default();
        breakpoints.insert(SCRIPT, 2, Kind::Standard);
        breakpoints.insert(SCRIPT, 3, Kind::Logpoint);
        breakpoints.follow_edit(SCRIPT, "a\nb\nc", "x\na\nb\nc");
        let lines: Vec<u32> = breakpoints.of(SCRIPT).map(|b| b.breakpoint.line).collect();
        assert_eq!(lines, vec![3, 4]);

        // Replacing lines 3-4 with one line: both land on it, the first stays.
        breakpoints.follow_edit(SCRIPT, "x\na\nb\nc", "x\na\nz");
        let kinds: Vec<(u32, Kind)> = breakpoints
            .of(SCRIPT)
            .map(|b| (b.breakpoint.line, b.kind()))
            .collect();
        assert_eq!(kinds, vec![(3, Kind::Standard)]);
    }

    const SCRIPT: Ref = Ref::new(7);

    #[test]
    fn each_kind_starts_out_as_the_kind_it_was_inserted_as() {
        assert_eq!(Stored::new(1, Kind::Standard).kind(), Kind::Standard);
        assert_eq!(Stored::new(1, Kind::Logpoint).kind(), Kind::Logpoint);
        assert_eq!(Stored::new(1, Kind::Temporary).kind(), Kind::Temporary);
        let mut conditional = Stored::new(1, Kind::Conditional);
        conditional.breakpoint.condition = Some("x == 1".into());
        assert_eq!(conditional.kind(), Kind::Conditional);
    }

    #[test]
    fn a_disabled_breakpoint_is_kept_but_not_run() {
        let mut breakpoints = Breakpoints::default();
        breakpoints.insert(SCRIPT, 3, Kind::Standard);
        breakpoints.insert(SCRIPT, 5, Kind::Standard);
        breakpoints.toggle_enabled(SCRIPT, 3);

        let lines: Vec<u32> = breakpoints.for_run(SCRIPT).iter().map(|b| b.line).collect();
        assert_eq!(lines, vec![5]);
        assert!(breakpoints.get(SCRIPT, 3).is_some_and(|b| !b.enabled));
    }

    #[test]
    fn temporary_breakpoints_go_when_the_run_ends_and_the_rest_stay() {
        let mut breakpoints = Breakpoints::default();
        breakpoints.insert(SCRIPT, 2, Kind::Temporary);
        breakpoints.insert(SCRIPT, 4, Kind::Logpoint);
        breakpoints.end_run(SCRIPT);

        let lines: Vec<u32> = breakpoints.of(SCRIPT).map(|b| b.breakpoint.line).collect();
        assert_eq!(lines, vec![4]);
    }

    #[test]
    fn breakpoints_belong_to_their_own_script() {
        let mut breakpoints = Breakpoints::default();
        breakpoints.insert(SCRIPT, 2, Kind::Standard);
        assert!(breakpoints.for_run(Ref::new(8)).is_empty());
        breakpoints.remove(SCRIPT, 2);
        assert!(breakpoints.for_run(SCRIPT).is_empty());
    }
}
