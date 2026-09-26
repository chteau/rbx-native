use gpui_kit::base::input::Rope;
use lsp_types::{CompletionItem, CompletionTextEdit, Diagnostic, Position, Range};
use serde_json::json;

use super::{editor_diagnostics, editor_offset, identifier_start, server_position, shape};

fn range(a: (u32, u32), b: (u32, u32)) -> Range {
    Range::new(Position::new(a.0, a.1), Position::new(b.0, b.1))
}

fn new_text(item: &CompletionItem) -> &str {
    match item.text_edit.as_ref().unwrap() {
        CompletionTextEdit::Edit(edit) => &edit.new_text,
        CompletionTextEdit::InsertAndReplace(edit) => &edit.new_text,
    }
}

#[test]
fn the_identifier_being_typed_starts_after_any_punctuation() {
    assert_eq!(identifier_start("game.Wor", 8), 5);
    assert_eq!(identifier_start("x:", 2), 2);
    assert_eq!(identifier_start("local é_1", 10), 6);
    assert_eq!(identifier_start("", 0), 0);
}

#[test]
fn completions_are_narrowed_ordered_and_replace_the_typed_prefix() {
    let reply = json!([
        {"label": "Workspace", "sortText": "1"},
        {"label": "workspace", "sortText": "0", "insertText": "workspace"},
        {"label": "Players", "sortText": "0"},
        {"label": "WaitForChild", "sortText": "2", "insertText": "WaitForChild()"},
    ]);
    let replace = range((0, 5), (0, 7));
    let items = shape(reply, "wo", replace);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["workspace", "Workspace"]);
    assert_eq!(new_text(&items[1]), "Workspace");
    match items[0].text_edit.as_ref().unwrap() {
        CompletionTextEdit::Edit(edit) => assert_eq!(edit.range, replace),
        other => panic!("{other:?}"),
    }

    let items = shape(
        json!({"isIncomplete": false, "items": [{"label": "WaitForChild", "insertText": "WaitForChild()"}]}),
        "",
        replace,
    );
    assert_eq!(new_text(&items[0]), "WaitForChild()");
    assert!(shape(json!(null), "", replace).is_empty());
}

#[test]
fn server_columns_are_bytes_and_editor_columns_are_characters() {
    let text = Rope::from("local é = 1\nprint(é)\n");
    // `é` is two bytes: `=` sits at byte 9 but character 8.
    let equals = "local é ".len();
    assert_eq!(server_position(&text, equals), Position::new(0, 9));
    assert_eq!(editor_offset(&text, Position::new(0, 9)), equals);

    let problem = Diagnostic {
        range: range((1, 6), (1, 8)),
        message: "unknown".into(),
        ..Default::default()
    };
    let moved = editor_diagnostics(&text, &[problem]);
    assert_eq!(moved[0].range, range((1, 6), (1, 7)));
    assert_eq!(moved[0].message, "unknown");
}

#[test]
fn a_stale_position_is_clamped_into_the_text() {
    let text = Rope::from("ab\né\n");
    assert_eq!(editor_offset(&text, Position::new(0, 99)), 2);
    assert_eq!(editor_offset(&text, Position::new(40, 0)), text.len());
    // Byte 1 of the two-byte `é` snaps back to its start.
    assert_eq!(editor_offset(&text, Position::new(1, 1)), 3);
}
