//! The graph's own palette: a colour per pin type, so a wire says what it
//! carries, and a tint per add-menu section for node headers. The hues are
//! fixed, like syntax colours, because they are a key the eye learns; the
//! surfaces they sit on follow the theme.

use gpui_kit::assets::IconName;
use gpui_kit::component::ThemeMode;
use gpui_kit::{rgb, rgba, Rgba};

use crate::script_editor::graph::catalog::{Category, PinType};
use crate::{theme, tokens};

pub(super) fn pin(ty: PinType) -> Rgba {
    let (dark, light) = match ty {
        PinType::Exec => (0xe6e6e6, 0x55555c),
        PinType::Bool => (0xe06c6c, 0xc23b3b),
        PinType::Number => (0x6fcf8f, 0x1c8752),
        PinType::String => (0xe0a84f, 0xa86a10),
        PinType::Instance => (0x7286e0, 0x3a52c4),
        PinType::List => (0xb48ee0, 0x7a4fd0),
        PinType::Any => (0x9aa0aa, 0x6b7078),
        PinType::Word => (0xc9ccd3, 0x55555c),
    };
    by_mode(dark, light)
}

/// The header's fill and its icon's colour. The fill is the icon's colour
/// washed over the node body, so it sits on whatever surface the theme has.
pub(super) fn header(category: Category) -> (Rgba, Rgba) {
    let (dark, light) = match category {
        Category::Events => (0xe06c6c, 0xc23b3b),
        Category::Flow | Category::Output => (0xb8bcc6, 0x55555c),
        Category::Instances | Category::Properties => (0xe0a84f, 0xa86a10),
        Category::Logic | Category::Math => (0x6fcf8f, 0x1c8752),
        Category::Values => (0x7286e0, 0x3a52c4),
        Category::Code => (0xa9a0e0, 0x6a5fc8),
    };
    let icon = by_mode(dark, light);
    (Rgba { a: 0.16, ..icon }, icon)
}

/// The pastels wash out on a light ground, so a light theme gets the same
/// hues darkened to stay readable on white.
fn by_mode(dark: u32, light: u32) -> Rgba {
    match theme::active().mode {
        ThemeMode::Light => rgb(light),
        _ => rgb(dark),
    }
}

pub(super) fn icon(category: Category) -> IconName {
    match category {
        Category::Events => IconName::Play,
        Category::Flow => IconName::ChevronsUpDown,
        Category::Instances => IconName::Box,
        Category::Properties => IconName::SlidersHorizontal,
        Category::Logic => IconName::Check,
        Category::Math => IconName::Calculator,
        Category::Values => IconName::Hash,
        Category::Output => IconName::Terminal,
        Category::Code => IconName::Code,
    }
}

pub(super) fn canvas() -> Rgba {
    tokens::black()
}

pub(super) fn node_body() -> Rgba {
    tokens::dock()
}

pub(super) fn node_border() -> Rgba {
    tokens::border2()
}

pub(super) fn chip() -> Rgba {
    tokens::field_select()
}

pub(super) fn group_fill() -> Rgba {
    rgba(0x7286e00d)
}

pub(super) fn group_border() -> Rgba {
    rgba(0x7286e066)
}
