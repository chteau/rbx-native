//! The About page (`Help › About RbxNative`): the version, where the source
//! lives, and the credits, one section per group of `crate::credits`.

use gpui_kit::*;

use crate::credits;
use crate::updater;

use super::kit::{ghost_icon, Row, Section};
use super::SettingsWindow;

const REPOSITORY: &str = "https://github.com/chteau/rbx-native";
const RELEASES: &str = "https://github.com/chteau/rbx-native/releases";

/// "v0.2.3" for a release, with a note for a local build.
pub(super) fn version() -> String {
    updater::current()
        .map(|v| format!("v{v}"))
        .unwrap_or_else(|| format!("v{} (development build)", env!("CARGO_PKG_VERSION")))
}

/// A row's link out, opened in the browser.
fn link(id: impl Into<ElementId>, label: &'static str, url: &'static str) -> impl IntoElement {
    ghost_icon(id, "external-link", label).on_click(move |_, _, cx| cx.open_url(url))
}

impl SettingsWindow {
    pub(super) fn about_page(&self) -> Vec<Section> {
        let app = Section::new(
            "RbxNative",
            vec![
                Row::new("Version", link("about-releases", "Releases", RELEASES))
                    .describe(version()),
                Row::new("Source code", link("about-source", "Repository", REPOSITORY))
                    .describe("Open source under the MIT licence, at github.com/chteau/rbx-native."),
                Row::new("Licence texts", div()).describe(
                    "CREDITS.md and the licenses folder ship beside the program, with THIRD-PARTY-LICENSES.html for the Rust crates built in.",
                ),
            ],
        );
        let groups = credits::groups().iter().enumerate().map(|(g, group)| {
            let rows = group
                .entries
                .iter()
                .enumerate()
                .map(|(e, credit)| {
                    Row::new(
                        credit.name.as_str(),
                        link(
                            ("about-credit", g * 100 + e),
                            "Open page",
                            credit.url.as_str(),
                        ),
                    )
                    .chip(credit.license.as_str())
                    .describe(format!("{} \u{b7} {}", credit.owner, credit.used_for))
                })
                .collect();
            Section::new(group.title.as_str(), rows)
        });
        std::iter::once(app).chain(groups).collect()
    }
}
