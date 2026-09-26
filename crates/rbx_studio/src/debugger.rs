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

    /// A run of `script` is over: its temporary breakpoints go.
    pub(crate) fn end_run(&mut self, script: Ref) {
        if let Some(lines) = self.scripts.get_mut(&script) {
            lines.retain(|_, stored| !stored.temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
