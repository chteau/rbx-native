//! Go to Declaration for the script editor: [`outline::declaration`] answered
//! through GPUI Kit's `DefinitionProvider`, which gives Ctrl-hover its
//! underline and Ctrl-click its jump with nothing else to wire.

use anyhow::Result;
use gpui_kit::base::input::{DefinitionProvider, Rope, RopeExt as _};
use gpui_kit::{App, Task, Window};
use lsp_types::{LocationLink, Range as LspRange};

use super::outline;

/// Answers every lookup from the text the editor hands it, so one instance
/// serves a tab for its whole life however the text changes.
pub(crate) struct Declarations;

impl DefinitionProvider for Declarations {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<Vec<LocationLink>>> {
        let source = text.to_string();
        let links = outline::declaration(&source, offset)
            .map(|target| {
                let origin = text.word_range(offset).unwrap_or(offset..offset);
                let range = |r: std::ops::Range<usize>| LspRange {
                    start: text.offset_to_position(r.start),
                    end: text.offset_to_position(r.end),
                };
                LocationLink {
                    origin_selection_range: Some(range(origin)),
                    // Not `http(s)`, which is all the editor needs to treat the
                    // link as a jump within its own text rather than a URL to
                    // open (see `gpui_base`'s `go_to_definition`).
                    target_uri: SCRIPT_URI.parse().expect("a valid URI literal"),
                    target_range: range(target.clone()),
                    target_selection_range: range(target),
                }
            })
            .into_iter()
            .collect();
        Task::ready(Ok(links))
    }
}

const SCRIPT_URI: &str = "rbx-script:current";
