//! The text of a `TextBox` being typed in: its caret, selection and undo
//! history, with no idea of where any character is drawn.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    text: String,
    caret: usize,
    anchor: usize,
}

#[derive(Clone, Debug, Default)]
pub(super) struct TextModel {
    pub(super) text: String,
    /// Byte offsets, both on character boundaries; the selection runs
    /// between them.
    pub(super) caret: usize,
    pub(super) anchor: usize,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The last change was typing, which the next keystroke joins into one
    /// undo step.
    typing: bool,
}

/// What a word is to double-click and Ctrl+arrows: letters, digits and
/// `_` together, every other character apart from them.
fn class(c: char) -> u8 {
    match c {
        c if c.is_alphanumeric() || c == '_' => 0,
        c if c.is_whitespace() => 1,
        _ => 2,
    }
}

impl TextModel {
    pub(super) fn new(text: String) -> Self {
        let end = text.len();
        TextModel {
            text,
            caret: end,
            anchor: end,
            ..TextModel::default()
        }
    }

    pub(super) fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    pub(super) fn selected(&self) -> &str {
        &self.text[self.selection()]
    }

    pub(super) fn move_to(&mut self, offset: usize, select: bool) {
        self.caret = self.boundary(offset);
        if !select {
            self.anchor = self.caret;
        }
        self.typing = false;
    }

    pub(super) fn select(&mut self, range: Range<usize>) {
        self.anchor = self.boundary(range.start);
        self.caret = self.boundary(range.end);
        self.typing = false;
    }

    /// `offset` clamped into the text and moved back onto a character
    /// boundary.
    fn boundary(&self, offset: usize) -> usize {
        let mut at = offset.min(self.text.len());
        while !self.text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    fn previous(&self, at: usize) -> usize {
        self.text[..at]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self, at: usize) -> usize {
        self.text[at..]
            .chars()
            .next()
            .map_or(at, |c| at + c.len_utf8())
    }

    /// Where Ctrl+Left lands: back over any spaces, then over the word.
    pub(super) fn word_start(&self, at: usize) -> usize {
        let mut at = at;
        let mut word = None;
        while at > 0 {
            let before = self.previous(at);
            let kind = self.text[before..].chars().next().map_or(1, class);
            match word {
                None if kind == 1 => {}
                None => word = Some(kind),
                Some(w) if w != kind => break,
                Some(_) => {}
            }
            at = before;
        }
        at
    }

    /// Where Ctrl+Right lands, as on Windows: past the word and the spaces
    /// after it, on the next word's start.
    pub(super) fn word_end(&self, at: usize) -> usize {
        let mut at = at;
        let first = self.text[at..].chars().next().map(class);
        while let Some(c) = self.text[at..].chars().next() {
            if Some(class(c)) != first || first == Some(1) {
                break;
            }
            at += c.len_utf8();
        }
        while let Some(c) = self.text[at..].chars().next().filter(|c| class(*c) == 1) {
            at += c.len_utf8();
        }
        at
    }

    /// The run of one kind of character around `at`, what a double-click
    /// selects: "text" out of "dark text here", not its space.
    pub(super) fn word_at(&self, at: usize) -> Range<usize> {
        let at = self.boundary(at);
        let Some(kind) = self.text[at..]
            .chars()
            .next()
            .or_else(|| self.text[..at].chars().next_back())
            .map(class)
        else {
            return at..at;
        };
        let mut start = at;
        while start > 0 && self.text[self.previous(start)..].chars().next().map(class) == Some(kind)
        {
            start = self.previous(start);
        }
        let mut end = at;
        while self.text[end..].chars().next().map(class) == Some(kind) {
            end = self.next(end);
        }
        start..end
    }

    /// Left/Right: a selection collapses to its own end first.
    pub(super) fn step(&mut self, forward: bool, select: bool, word: bool) {
        let range = self.selection();
        let to = match (select, range.is_empty(), forward, word) {
            (false, false, false, false) => range.start,
            (false, false, true, false) => range.end,
            (_, _, false, true) => self.word_start(self.caret),
            (_, _, true, true) => self.word_end(self.caret),
            (_, _, false, false) => self.previous(self.caret),
            (_, _, true, false) => self.next(self.caret),
        };
        self.move_to(to, select);
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            caret: self.caret,
            anchor: self.anchor,
        }
    }

    /// Replaces `range` (the selection when `None`) with `with`, leaving the
    /// caret after it. `typed` joins a run of keystrokes into one undo step.
    pub(super) fn replace(&mut self, range: Option<Range<usize>>, with: &str, typed: bool) {
        let range = range.map_or_else(
            || self.selection(),
            |r| self.boundary(r.start)..self.boundary(r.end),
        );
        if range.is_empty() && with.is_empty() {
            return;
        }
        if !(typed && self.typing) {
            self.undo.push(self.snapshot());
        }
        self.redo.clear();
        self.text.replace_range(range.clone(), with);
        self.caret = range.start + with.len();
        self.anchor = self.caret;
        self.typing = typed;
    }

    /// Backspace and Delete: the selection, or one character (one word with
    /// Ctrl) before or after the caret.
    pub(super) fn erase(&mut self, forward: bool, word: bool) {
        let range = self.selection();
        let range = match (range.is_empty(), forward, word) {
            (false, _, _) => range,
            (true, false, false) => self.previous(self.caret)..self.caret,
            (true, false, true) => self.word_start(self.caret)..self.caret,
            (true, true, false) => self.caret..self.next(self.caret),
            (true, true, true) => self.caret..self.word_end(self.caret),
        };
        self.replace(Some(range), "", false);
    }

    pub(super) fn undo(&mut self) -> bool {
        self.history(true)
    }

    pub(super) fn redo(&mut self) -> bool {
        self.history(false)
    }

    fn history(&mut self, back: bool) -> bool {
        let (from, to) = match back {
            true => (&mut self.undo, &mut self.redo),
            false => (&mut self.redo, &mut self.undo),
        };
        let Some(snapshot) = from.pop() else {
            return false;
        };
        to.push(Snapshot {
            text: std::mem::replace(&mut self.text, snapshot.text),
            caret: self.caret,
            anchor: self.anchor,
        });
        self.caret = snapshot.caret;
        self.anchor = snapshot.anchor;
        self.typing = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, caret: usize) -> TextModel {
        let mut model = TextModel::new(text.to_string());
        model.move_to(caret, false);
        model
    }

    #[test]
    fn a_double_click_takes_the_word_and_not_its_space() {
        let model = at("dark text here", 0);
        assert_eq!(model.word_at(7), 5..9);
        assert_eq!(model.word_at(4), 4..5, "the space alone");
        assert_eq!(model.word_at(14), 10..14, "past the end, the last word");
    }

    #[test]
    fn ctrl_arrows_go_word_by_word_like_windows() {
        let model = at("one two,  three", 0);
        assert_eq!(model.word_end(0), 4);
        assert_eq!(model.word_end(4), 7);
        assert_eq!(model.word_end(7), 10);
        assert_eq!(model.word_start(15), 10);
        assert_eq!(model.word_start(10), 7);
        assert_eq!(model.word_start(4), 0);
    }

    #[test]
    fn arrows_collapse_a_selection_to_its_end() {
        let mut model = at("hello", 1);
        model.move_to(4, true);
        model.step(false, false, false);
        assert_eq!((model.caret, model.anchor), (1, 1));
        model.move_to(4, true);
        model.step(true, false, false);
        assert_eq!((model.caret, model.anchor), (4, 4));
        model.step(true, true, false);
        assert_eq!(model.selection(), 4..5);
    }

    #[test]
    fn typing_is_one_undo_step_and_an_erase_another() {
        let mut model = at("ab", 2);
        for c in ["c", "d", "e"] {
            model.replace(None, c, true);
        }
        model.erase(false, false);
        assert_eq!(model.text, "abcd");
        assert!(model.undo());
        assert_eq!(model.text, "abcde");
        assert!(model.undo());
        assert_eq!((model.text.as_str(), model.caret), ("ab", 2));
        assert!(model.redo());
        assert_eq!(model.text, "abcde");
    }

    #[test]
    fn offsets_never_split_a_character() {
        let mut model = at("é!", 0);
        model.move_to(1, false);
        assert_eq!(model.caret, 0);
        model.step(true, false, false);
        assert_eq!(model.caret, 2);
        model.erase(false, false);
        assert_eq!(model.text, "!");
    }
}
