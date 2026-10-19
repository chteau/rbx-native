//! Up/Down in the Command Bar: the commands submitted so far, most recent
//! first, the way a shell's history walks.

/// How many submitted commands are kept; older ones fall off the front.
const CAP: usize = 100;

#[derive(Debug, Default)]
pub(crate) struct Recall {
    entries: Vec<String>,
    /// The entry shown in the input, `None` while the user is on their own
    /// line.
    cursor: Option<usize>,
    /// What was typed before the first Up, handed back by walking Down past
    /// the newest entry.
    draft: String,
}

impl Recall {
    /// Remembers a submitted command. Blank lines and a repeat of the newest
    /// entry are not kept; the walk restarts from the bottom either way.
    pub(crate) fn submit(&mut self, source: &str) {
        self.cursor = None;
        self.draft.clear();
        if source.trim().is_empty() || self.entries.last().is_some_and(|last| last == source) {
            return;
        }
        if self.entries.len() == CAP {
            self.entries.remove(0);
        }
        self.entries.push(source.to_owned());
    }

    /// Up: the next older command, given what the input holds now. `None`
    /// when there is nothing to recall.
    pub(crate) fn older(&mut self, current: &str) -> Option<&str> {
        let index = match self.cursor {
            Some(0) => 0,
            Some(index) => index - 1,
            None => {
                let newest = self.entries.len().checked_sub(1)?;
                self.draft = current.to_owned();
                newest
            }
        };
        self.cursor = Some(index);
        Some(&self.entries[index])
    }

    /// Down: the next newer command, then the draft Up started from. `None`
    /// when the walk has not started.
    pub(crate) fn newer(&mut self) -> Option<&str> {
        let index = self.cursor?;
        if index + 1 < self.entries.len() {
            self.cursor = Some(index + 1);
            Some(&self.entries[index + 1])
        } else {
            self.cursor = None;
            Some(&self.draft)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recall(entries: &[&str]) -> Recall {
        let mut recall = Recall::default();
        for entry in entries {
            recall.submit(entry);
        }
        recall
    }

    #[test]
    fn up_walks_back_from_the_most_recent_and_stops_at_the_oldest() {
        let mut recall = recall(&["a", "b", "c"]);
        assert_eq!(recall.older(""), Some("c"));
        assert_eq!(recall.older(""), Some("b"));
        assert_eq!(recall.older(""), Some("a"));
        assert_eq!(recall.older(""), Some("a"));
    }

    #[test]
    fn down_past_the_newest_returns_the_draft() {
        let mut recall = recall(&["a", "b"]);
        assert_eq!(recall.older("half typed"), Some("b"));
        assert_eq!(recall.older("ignored"), Some("a"));
        assert_eq!(recall.newer(), Some("b"));
        assert_eq!(recall.newer(), Some("half typed"));
        assert_eq!(recall.newer(), None);
    }

    #[test]
    fn nothing_is_recalled_before_a_command_was_submitted() {
        let mut recall = Recall::default();
        assert_eq!(recall.older("x"), None);
        assert_eq!(recall.newer(), None);
    }

    #[test]
    fn blank_lines_and_immediate_repeats_are_not_kept() {
        let mut recall = recall(&["a", "  ", "a", "b", "a"]);
        assert_eq!(recall.older(""), Some("a"));
        assert_eq!(recall.older(""), Some("b"));
        assert_eq!(recall.older(""), Some("a"));
        assert_eq!(recall.older(""), Some("a"));
    }

    #[test]
    fn submitting_restarts_the_walk_from_the_bottom() {
        let mut recall = recall(&["a", "b"]);
        recall.older("");
        recall.older("");
        recall.submit("c");
        assert_eq!(recall.older(""), Some("c"));
    }

    #[test]
    fn the_oldest_entries_fall_off_past_the_cap() {
        let mut recall = Recall::default();
        for index in 0..CAP + 5 {
            recall.submit(&index.to_string());
        }
        let mut oldest = String::new();
        while let Some(entry) = recall.older("") {
            if entry == oldest {
                break;
            }
            oldest = entry.to_owned();
        }
        assert_eq!(oldest, "5");
    }
}
