//! Text typed into a running game. The platform's text input (what the key
//! typed in the keyboard layout, an input method's commit, the emoji
//! picker, dictation, a remote desktop's characters) is what a focused
//! TextBox and the chat bar get, never a guess from the key that was
//! pressed: that one only becomes the game's `KeyCode`.

use std::ops::Range;

use gpui_kit::*;

use super::WorkspaceView;

/// The game view's [`InputHandler`] while a game runs: each piece of text
/// the platform commits goes to [`WorkspaceView::game_text`]. There is no
/// text to edit here, so the queries answer nothing and a composition shows
/// only once it's committed.
pub(super) struct GameText(pub(super) WeakEntity<WorkspaceView>);

impl InputHandler for GameText {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<UTF16Selection> {
        None
    }

    fn marked_text_range(&mut self, _window: &mut Window, _cx: &mut App) -> Option<Range<usize>> {
        None
    }

    fn text_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<String> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut App,
    ) {
        let _ = self.0.update(cx, |view, cx| view.game_text(text, cx));
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        _new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut App) {}

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        None
    }

    /// A held key repeats, as it does in Roblox, instead of opening
    /// macOS's accent picker over the game (holding `a` or `e` to walk).
    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }
}

/// What of `text` goes into a TextBox or the chat bar: Return, Tab and
/// Backspace act as keys there, so their control characters are dropped.
pub(super) fn typed(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn control_characters_are_left_to_the_keys() {
        assert_eq!(super::typed("h\u{e9}\r\n\t\u{8}"), "h\u{e9}");
        assert_eq!(super::typed("\u{1f600}"), "\u{1f600}");
    }
}
