//! The core GUI's icons: our own line drawings on a 24 px canvas, white,
//! rasterized once each (GPUI's `svg()` would do for monochrome, but these
//! share `class_icons`' rasterizer and cache shape with the keyboard's
//! Twemoji).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Glyph {
    Logo,
    Menu,
    Chat,
    ChatOn,
    Close,
    People,
    Settings,
    Gallery,
    Report,
    Help,
    Invite,
    Grid,
    List,
    Zoom,
    Smile,
    /// Next to the name of the experience's owner in the People list.
    Owner,
    /// Next to the name of a verified account.
    Verified,
}

const OPEN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#F7F7F8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">"##;

fn body(glyph: Glyph) -> &'static str {
    match glyph {
        Glyph::Logo => "",
        Glyph::Menu => r##"<path d="M4 6.5h16M4 12h16M4 17.5h16"/>"##,
        Glyph::Chat => {
            r##"<path d="M4 4.5h16v11.5h-5.5l-2.5 3-2.5-3H4z"/><path d="M8.5 8.8h7M10 12h4"/>"##
        }
        Glyph::ChatOn => {
            r##"<path d="M4 4.5h16v11.5h-5.5l-2.5 3-2.5-3H4z" fill="#F7F7F8"/><path d="M8.5 8.8h7M10 12h4" stroke="#191F24"/>"##
        }
        Glyph::Close => r##"<path d="M6 6l12 12M18 6L6 18"/>"##,
        Glyph::People => {
            r##"<circle cx="9" cy="8" r="3.2"/><path d="M3.5 19c.6-3.2 2.8-5 5.5-5s4.9 1.8 5.5 5z"/><path d="M15 5.2a3 3 0 0 1 0 5.6M17 14.3c1.8.6 3 2.2 3.5 4.7h-3"/>"##
        }
        Glyph::Settings => {
            r##"<circle cx="12" cy="12" r="3"/><path d="M12 3.5l1.6 2.2 2.7-.5.7 2.6 2.5 1.2-1 2.5 1 2.5-2.5 1.2-.7 2.6-2.7-.5L12 20.5l-1.6-2.2-2.7.5-.7-2.6L4.5 15l1-2.5-1-2.5L7 8.8l.7-2.6 2.7.5z"/>"##
        }
        Glyph::Gallery => {
            r##"<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><circle cx="9" cy="9.5" r="1.6"/><path d="M4 17l5-4.5 3.5 3 3-2.5 4.5 4"/>"##
        }
        Glyph::Report => r##"<path d="M5.5 21V4"/><path d="M5.5 4.5h11l-2.5 4 2.5 4h-11"/>"##,
        Glyph::Help => {
            r##"<circle cx="12" cy="12" r="8.5"/><path d="M9.6 9.6a2.5 2.5 0 1 1 3.4 2.3c-.7.3-1 .8-1 1.6v.5"/><path d="M12 16.8v.2"/>"##
        }
        Glyph::Invite => {
            r##"<circle cx="10" cy="8" r="3.5"/><path d="M3.5 20c.5-3.6 3-5.6 6.5-5.6 1.2 0 2.3.2 3.2.7"/><path d="M15 15l5.5 5.5M15 15v4M15 15h4"/>"##
        }
        Glyph::Grid => {
            r##"<rect x="4.5" y="4.5" width="15" height="15" rx="1"/><path d="M9.5 4.5v15M14.5 4.5v15M4.5 9.5h15M4.5 14.5h15"/>"##
        }
        Glyph::List => {
            r##"<path d="M9 7h11M9 12h11M9 17h11"/><path d="M4.5 7h.5M4.5 12h.5M4.5 17h.5" stroke-width="2.4"/>"##
        }
        Glyph::Zoom => {
            r##"<circle cx="10.5" cy="10.5" r="6.5"/><path d="M15.3 15.3L20 20M10.5 7.8v5.4M7.8 10.5h5.4"/>"##
        }
        Glyph::Smile => {
            r##"<circle cx="12" cy="12" r="8.5"/><path d="M8.5 14c.8 1.4 2 2.1 3.5 2.1s2.7-.7 3.5-2.1"/><path d="M9.3 9.5v.8M14.7 9.5v.8" stroke-width="2.2"/>"##
        }
        // A builder's hammer leaning on a small cog: the maker of the place.
        Glyph::Owner => {
            r##"<path d="M4.5 19.5l8-8"/><path d="M9.5 5.5l3-2 6 6-2 3z" fill="#F7F7F8"/><circle cx="17.5" cy="17.5" r="2.3"/><path d="M17.5 13.6v1.3M17.5 20.1v1.3M13.6 17.5h1.3M20.1 17.5h1.3" stroke-width="1.5"/>"##
        }
        // A blue rounded tile with a white tick.
        Glyph::Verified => {
            r##"<rect x="3" y="3" width="18" height="18" rx="4.5" fill="#1F6FEB" stroke="none"/><path d="M7.5 12.3l3 3 6-6.3" stroke-width="2.4"/>"##
        }
    }
}

fn tile(glyph: Glyph) -> Option<Arc<RenderImage>> {
    thread_local! {
        static CACHE: RefCell<HashMap<Glyph, Option<Arc<RenderImage>>>> =
            RefCell::new(HashMap::new());
    }
    CACHE.with_borrow_mut(|cache| {
        cache
            .entry(glyph)
            .or_insert_with(|| {
                if glyph == Glyph::Logo {
                    let svg =
                        include_bytes!("../../../../../assets/icons/brand/rbxnative-logo.svg");
                    return crate::class_icons::rasterize_at(svg, 64);
                }
                let svg = format!("{OPEN}{}</svg>", body(glyph));
                crate::class_icons::rasterize_at(svg.as_bytes(), 64)
            })
            .clone()
    })
}

/// `glyph` in a `size` square.
pub(super) fn icon(glyph: Glyph, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .when_some(tile(glyph), |this, image| {
            this.child(img(image).size_full())
        })
}

#[cfg(test)]
mod tests {
    use super::{tile, Glyph};

    #[test]
    fn every_glyph_rasterizes() {
        for glyph in [
            Glyph::Logo,
            Glyph::Menu,
            Glyph::Chat,
            Glyph::ChatOn,
            Glyph::Close,
            Glyph::People,
            Glyph::Settings,
            Glyph::Gallery,
            Glyph::Report,
            Glyph::Help,
            Glyph::Invite,
            Glyph::Grid,
            Glyph::List,
            Glyph::Zoom,
            Glyph::Smile,
            Glyph::Owner,
            Glyph::Verified,
        ] {
            assert!(tile(glyph).is_some(), "{glyph:?}");
        }
    }
}
