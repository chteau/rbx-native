//! The left column: search, the ten pages, the two settings that are
//! windows of their own, and the config folder.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::kit::{self, icon, mono, soon_pill, text};
use super::SettingsWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Appearance,
    Accessibility,
    Layout,
    Viewport,
    Dragger,
    ExplorerOutput,
    Files,
    Argon,
    Account,
    Beta,
}

impl Page {
    pub(super) const ALL: [Page; 10] = [
        Page::Appearance,
        Page::Accessibility,
        Page::Layout,
        Page::Viewport,
        Page::Dragger,
        Page::ExplorerOutput,
        Page::Files,
        Page::Argon,
        Page::Account,
        Page::Beta,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance",
            Page::Accessibility => "Accessibility",
            Page::Layout => "Layout",
            Page::Viewport => "Viewport",
            Page::Dragger => "Dragger & snapping",
            Page::ExplorerOutput => "Explorer & Output",
            Page::Files => "Files & recovery",
            Page::Argon => "Argon",
            Page::Account => "Account",
            Page::Beta => "Beta features",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Page::Appearance => "palette",
            Page::Accessibility => "accessibility",
            Page::Layout => "panels-top-left",
            Page::Viewport => "box",
            Page::Dragger => "move",
            Page::ExplorerOutput => "panel-left",
            Page::Files => "save",
            Page::Argon => "refresh-cw",
            Page::Account => "user",
            Page::Beta => "flask-conical",
        }
    }

    /// Whether the whole page is on the roadmap.
    fn soon(self) -> bool {
        self == Page::Beta
    }

    /// The page's h1 and the line under it.
    pub(super) fn heading(self) -> (&'static str, &'static str) {
        let subtitle = match self {
            Page::Appearance => "How RbxNative looks. Changes apply as you make them.",
            Page::Accessibility => "Motion and hit targets. RbxNative also follows the system\u{2019}s reduced-motion setting.",
            Page::Layout => "Where the docks sit. The current arrangement is saved as you drag.",
            Page::Viewport => "Rendering, camera and what the viewport draws on top of the scene.",
            Page::Dragger => "How parts move under the mouse. The same switches sit in the Viewport dock.",
            Page::ExplorerOutput => "Defaults for the Explorer tree and the Output log.",
            Page::Files => "Auto-saves and the copies Play makes. None of this is built yet.",
            Page::Argon => "Live sync with an Argon project. The Argon dock shows the same settings.",
            Page::Account => "Your Roblox access and what RbxNative shares about you.",
            Page::Beta => "Try features before they\u{2019}re finished. Switch one off if it misbehaves.",
        };
        (self.label(), subtitle)
    }

    /// A page by the key `RBX_STUDIO_SETTINGS_PAGE` names it with: its
    /// label, lowercased, up to the first space.
    pub(super) fn from_key(key: &str) -> Option<Page> {
        let key = key.to_lowercase();
        Page::ALL.into_iter().find(|page| {
            page.label()
                .split_whitespace()
                .next()
                .is_some_and(|word| word.to_lowercase() == key)
        })
    }
}

impl SettingsWindow {
    pub(super) fn nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pages = Page::ALL.into_iter().map(|page| {
            let current = page == self.page;
            nav_item(
                ("page", page as usize),
                page.glyph(),
                15.,
                page.label(),
                32.,
                12.5,
                17.,
            )
            .map(|this| {
                if current {
                    this.bg(tokens::accent_soft())
                        .text_color(tokens::check_on())
                        .font_weight(FontWeight::SEMIBOLD)
                } else {
                    this
                }
            })
            .when(page.soon(), |this| {
                this.child(soon_pill(("page-soon", page as usize)))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                cx.notify();
            }))
        });
        let own_windows = [
            ("keyboard", "Keyboard shortcuts"),
            ("sliders-horizontal", "Game settings"),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (glyph, label))| {
            nav_item(("own", i), glyph, 14., label, 30., 12., 16.).child(soon_pill(("own-soon", i)))
        });
        let folder = crate::settings::default_config_dir();
        let shown = folder.as_ref().map(|path| tilde(path)).unwrap_or_default();
        v_flex()
            .w(px(232.))
            .flex_none()
            .bg(tokens::black())
            .border_r_1()
            .border_color(tokens::border())
            .pt(px(14.))
            .px(px(12.))
            .pb(px(12.))
            .child(self.search_field())
            .child(v_flex().gap(px(2.)).mt(px(14.)).children(pages))
            .child(
                div()
                    .h(px(1.))
                    .bg(tokens::border())
                    .mt(px(14.))
                    .mb(px(10.))
                    .mx(px(4.)),
            )
            .child(
                text(10.5, 14.)
                    .px(px(10.))
                    .pb(px(6.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens::text3())
                    .child("OWN WINDOWS"),
            )
            .child(v_flex().gap(px(2.)).children(own_windows))
            .child(div().flex_1())
            .child(
                nav_item(
                    "open-config",
                    "folder",
                    14.,
                    "Open config folder",
                    30.,
                    12.,
                    16.,
                )
                .child(icon("external-link", 11.))
                .on_click(move |_, _, cx| {
                    if let Some(folder) = &folder {
                        let _ = std::fs::create_dir_all(folder);
                        cx.open_with_system(folder);
                    }
                }),
            )
            .child(
                mono(10.5, 14.)
                    .pt(px(4.))
                    .px(px(10.))
                    .text_color(tokens::text3())
                    .truncate()
                    .child(shown),
            )
    }
}

fn nav_item(
    id: impl Into<ElementId>,
    glyph: &'static str,
    glyph_size: f32,
    label: &'static str,
    h: f32,
    size: f32,
    line: f32,
) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .h(px(h))
        .flex_none()
        .gap(px(10.))
        .px(px(10.))
        .items_center()
        .rounded(px(6.))
        .text_size(px(size))
        .line_height(px(line))
        .text_color(tokens::text2())
        .cursor_pointer()
        .hover(|this| this.bg(rgba(0xFFFFFF08)).text_color(tokens::text()))
        .child(kit::icon(glyph, glyph_size))
        .child(div().flex_1().child(label))
}

/// `path` with the home directory spelt `~`.
fn tilde(path: &std::path::Path) -> String {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::Page;

    #[test]
    fn every_page_is_reachable_by_its_first_word() {
        for page in Page::ALL {
            let key = page.label().split_whitespace().next().unwrap();
            assert_eq!(Page::from_key(key), Some(page), "{key}");
        }
        assert_eq!(Page::from_key("dragger"), Some(Page::Dragger));
        assert_eq!(Page::from_key("nowhere"), None);
    }
}
