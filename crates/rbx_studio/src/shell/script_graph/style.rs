//! The graph's own palette: a colour per pin type, so a wire says what it
//! carries, and a tint per add-menu section for node headers. Fixed rather
//! than themed, like syntax colours: they are a key the eye learns.

use gpui_kit::assets::IconName;
use gpui_kit::{rgb, rgba, Rgba};

use crate::script_editor::graph::catalog::{Category, PinType};

pub(super) fn pin(ty: PinType) -> Rgba {
    rgb(match ty {
        PinType::Exec => 0xe6e6e6,
        PinType::Bool => 0xe06c6c,
        PinType::Number => 0x6fcf8f,
        PinType::String => 0xe0a84f,
        PinType::Instance => 0x7286e0,
        PinType::List => 0xb48ee0,
        PinType::Any => 0x9aa0aa,
    })
}

/// The header's fill and its icon's colour.
pub(super) fn header(category: Category) -> (Rgba, Rgba) {
    let (fill, icon) = match category {
        Category::Events => (0x3d2426, 0xe06c6c),
        Category::Flow => (0x2b2c31, 0xb8bcc6),
        Category::Instances => (0x3a3024, 0xe0a84f),
        Category::Properties => (0x3a3024, 0xe0a84f),
        Category::Logic => (0x233528, 0x6fcf8f),
        Category::Math => (0x233528, 0x6fcf8f),
        Category::Values => (0x262b3a, 0x7286e0),
        Category::Output => (0x2b2c31, 0xb8bcc6),
    };
    (rgb(fill), rgb(icon))
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
    }
}

pub(super) fn canvas() -> Rgba {
    rgb(0x0f0f10)
}

pub(super) fn node_body() -> Rgba {
    rgb(0x17181b)
}

pub(super) fn node_border() -> Rgba {
    rgb(0x2c2e33)
}

pub(super) fn chip() -> Rgba {
    rgb(0x202226)
}

pub(super) fn group_fill() -> Rgba {
    rgba(0x7286e00d)
}

pub(super) fn group_border() -> Rgba {
    rgba(0x7286e066)
}
