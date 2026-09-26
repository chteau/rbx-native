//! Go to Definition through `luau-lsp`, which follows a name across scripts
//! — into the `ModuleScript` a `require` returned, say — where the lexer
//! lookup in `script_editor::goto` stays within one.
//!
//! A target in the same script is converted to the editor's columns here
//! and jumped to by the editor itself. A target in another script keeps the
//! server's columns: only that script's own text can convert them, and the
//! Shell opens it (see `shell::luau_lsp::reveal_location`).

use std::rc::Rc;

use anyhow::Result;
use gpui_kit::base::input::{DefinitionProvider, Rope, RopeExt as _};
use gpui_kit::{App, Task, Window};
use lsp_types::{GotoDefinitionResponse, LocationLink, Range};
use serde_json::Value;

use super::{editor_offset, Document};
use crate::luau_lsp;

pub(crate) struct Definitions(pub(crate) Rc<Document>);

impl DefinitionProvider for Definitions {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<LocationLink>>> {
        let reply = self.0.definition(text, offset);
        let own = self.0.uri().to_owned();
        let text = text.clone();
        let origin = text.word_range(offset).unwrap_or(offset..offset);
        cx.background_executor().spawn(async move {
            let reply = luau_lsp::wait(reply).map_err(anyhow::Error::msg)?;
            Ok(links(reply, &own, &text, origin))
        })
    }
}

/// The server's answer, in whichever of its three shapes, as links from the
/// hovered word (`origin`, a byte range of `text`).
pub(crate) fn links(
    reply: Value,
    own: &str,
    text: &Rope,
    origin: std::ops::Range<usize>,
) -> Vec<LocationLink> {
    let links = match serde_json::from_value(reply) {
        Ok(GotoDefinitionResponse::Scalar(location)) => vec![(location.uri, location.range)],
        Ok(GotoDefinitionResponse::Array(locations)) => locations
            .into_iter()
            .map(|location| (location.uri, location.range))
            .collect(),
        Ok(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| (link.target_uri, link.target_selection_range))
            .collect(),
        Err(_) => Vec::new(),
    };
    let to_editor = |range: Range| {
        let position = |p| text.offset_to_position(editor_offset(text, p));
        Range::new(position(range.start), position(range.end))
    };
    links
        .into_iter()
        .map(|(uri, range)| {
            let range = match same_file(uri.as_str(), own) {
                true => to_editor(range),
                false => range,
            };
            LocationLink {
                origin_selection_range: Some(Range::new(
                    text.offset_to_position(origin.start),
                    text.offset_to_position(origin.end),
                )),
                target_uri: uri,
                target_range: range,
                target_selection_range: range,
            }
        })
        .collect()
}

/// By file name, for the reason `luau_lsp::Mirror::script_at` gives.
pub(crate) fn same_file(a: &str, b: &str) -> bool {
    let name = |uri| luau_lsp::path(uri).and_then(|path| path.file_name().map(ToOwned::to_owned));
    name(a).is_some() && name(a) == name(b)
}

#[cfg(test)]
mod tests {
    use gpui_kit::base::input::Rope;
    use lsp_types::{Position, Range};
    use serde_json::json;

    use super::{links, same_file};
    use crate::luau_lsp;

    /// From a real path, since a URI without a drive letter is no file path
    /// on Windows.
    fn file(folder: &str, name: &str) -> String {
        luau_lsp::uri(&std::env::temp_dir().join(folder).join(name))
    }

    fn range(a: (u32, u32), b: (u32, u32)) -> serde_json::Value {
        json!({"start": {"line": a.0, "character": a.1}, "end": {"line": b.0, "character": b.1}})
    }

    #[test]
    fn a_target_in_this_script_moves_into_editor_columns_and_another_does_not() {
        // `é` is two bytes: byte column 9 is character 8.
        let text = Rope::from("local é = 1\nprint(é)\n");
        let (own, other) = (file("place", "7.luau"), file("place", "9.luau"));
        let reply = json!([
            {"uri": own, "range": range((0, 9), (0, 10))},
            {"uri": other, "range": range((0, 9), (0, 10))},
        ]);
        let found = links(reply, &own, &text, 0..5);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0].target_selection_range,
            Range::new(Position::new(0, 8), Position::new(0, 9))
        );
        assert_eq!(
            found[1].target_selection_range,
            Range::new(Position::new(0, 9), Position::new(0, 10))
        );
        assert_eq!(
            found[0].origin_selection_range,
            Some(Range::new(Position::new(0, 0), Position::new(0, 5)))
        );
    }

    #[test]
    fn every_reply_shape_is_read() {
        let text = Rope::from("x");
        let (own, other) = (file("place", "7.luau"), file("place", "9.luau"));
        let scalar = json!({"uri": other, "range": range((1, 0), (1, 1))});
        let link = json!([{"targetUri": other, "targetRange": range((0, 0), (3, 0)),
            "targetSelectionRange": range((1, 0), (1, 1))}]);
        for reply in [scalar, link] {
            let found = links(reply, &own, &text, 0..1);
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].target_selection_range.start, Position::new(1, 0));
        }
        assert!(links(json!(null), &own, &text, 0..1).is_empty());
    }

    #[test]
    fn files_are_compared_by_name() {
        let own = file("place", "7.luau");
        assert!(same_file(&own, &file("other", "7.luau")));
        assert!(!same_file(&own, &file("place", "9.luau")));
        assert!(!same_file("not a uri", "not a uri"));
    }
}
