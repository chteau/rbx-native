//! Countdown overlays and the composites made from them. An overlay is a
//! transparent 512×512 image laid over the experience's current icon: one
//! of ours, drawn here as SVG and rasterized with resvg in Manrope Bold (the
//! editor's own font), or a PNG the user picked.

use std::sync::{Arc, OnceLock};

use image::{imageops, RgbaImage};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{fontdb, Options, Tree};
use serde::{Deserialize, Serialize};

/// Roblox's experience icon size, and every composite's.
pub(crate) const SIDE: u32 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Style {
    /// A dark band along the bottom: an hourglass and the time left.
    Hourglass,
    /// A red sash across the top-right corner.
    Sash,
    /// A round badge in the top-right corner, "UPDATE IN" over the time.
    Badge,
}

impl Style {
    pub(crate) const ALL: [Style; 3] = [Style::Hourglass, Style::Sash, Style::Badge];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Style::Hourglass => "Hourglass band",
            Style::Sash => "Corner sash",
            Style::Badge => "Round badge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Overlay {
    Builtin(Style),
    /// A PNG copied into the experience's folder, by file name.
    File(String),
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The SVG for `style` saying `label`.
pub(crate) fn svg(style: Style, label: &str) -> String {
    let label = escape(label);
    let upper = label.to_uppercase();
    let body = match style {
        Style::Hourglass => format!(
            r##"<defs><linearGradient id="band" x1="0" y1="0" x2="0" y2="1">
<stop offset="0" stop-color="#16181d" stop-opacity="0.82"/><stop offset="1" stop-color="#0b0c0f" stop-opacity="0.94"/></linearGradient>
<linearGradient id="sand" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffd76a"/><stop offset="1" stop-color="#f29a2e"/></linearGradient></defs>
<rect x="24" y="364" width="464" height="124" rx="30" fill="url(#band)" stroke="#ffffff" stroke-opacity="0.14" stroke-width="2"/>
<g transform="translate(58 384)">
<rect x="0" y="0" width="64" height="10" rx="5" fill="#f4f1ea"/><rect x="0" y="74" width="64" height="10" rx="5" fill="#f4f1ea"/>
<path d="M8 10 H56 C56 30 40 36 36 42 C40 48 56 54 56 74 H8 C8 54 24 48 28 42 C24 36 8 30 8 10 Z" fill="none" stroke="#f4f1ea" stroke-width="5" stroke-linejoin="round"/>
<path d="M16 18 H48 C46 28 36 33 32 38 C28 33 18 28 16 18 Z" fill="url(#sand)"/>
<path d="M32 50 C36 56 46 60 48 68 H16 C18 60 28 56 32 50 Z" fill="url(#sand)"/></g>
<text x="152" y="447" font-family="Manrope" font-weight="700" font-size="62" fill="#ffffff">{label}</text>"##
        ),
        Style::Sash => format!(
            r##"<defs><linearGradient id="sash" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#ff4d4d"/><stop offset="1" stop-color="#d81e3a"/></linearGradient></defs>
<g transform="rotate(45 360 152)">
<rect x="40" y="88" width="640" height="128" fill="#0b0c0f" fill-opacity="0.3" transform="translate(0 8)"/>
<rect x="40" y="88" width="640" height="128" fill="url(#sash)"/>
<rect x="40" y="97" width="640" height="3" fill="#ffffff" fill-opacity="0.6"/><rect x="40" y="204" width="640" height="3" fill="#ffffff" fill-opacity="0.6"/>
<text x="360" y="138" text-anchor="middle" font-family="Manrope" font-weight="700" font-size="24" letter-spacing="5" fill="#ffffff" fill-opacity="0.9">UPDATE IN</text>
<text x="360" y="190" text-anchor="middle" font-family="Manrope" font-weight="700" font-size="{size}" fill="#ffffff">{upper}</text></g>"##,
            size = if upper.chars().count() > 8 { 40 } else { 50 }
        ),
        Style::Badge => format!(
            r##"<defs><radialGradient id="disc" cx="0.35" cy="0.3" r="0.8"><stop offset="0" stop-color="#4f8cff"/><stop offset="1" stop-color="#2148c9"/></radialGradient></defs>
<circle cx="396" cy="122" r="106" fill="#0b0c0f" fill-opacity="0.35"/>
<circle cx="390" cy="116" r="102" fill="url(#disc)" stroke="#ffffff" stroke-width="6"/>
<circle cx="390" cy="116" r="86" fill="none" stroke="#ffffff" stroke-opacity="0.35" stroke-width="3" stroke-dasharray="4 10"/>
<text x="390" y="98" text-anchor="middle" font-family="Manrope" font-weight="700" font-size="18" letter-spacing="3" fill="#ffffff" fill-opacity="0.9">UPDATE IN</text>
<text x="390" y="142" text-anchor="middle" font-family="Manrope" font-weight="700" font-size="{size}" fill="#ffffff">{upper}</text>"##,
            size = match upper.chars().count() {
                0..=5 => 42,
                6..=7 => 34,
                _ => 26,
            }
        ),
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">{body}</svg>"#
    )
}

fn options() -> &'static Options<'static> {
    static OPTIONS: OnceLock<Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../../../../assets/fonts/Manrope-Bold.ttf").to_vec());
        Options {
            fontdb: Arc::new(db),
            ..Options::default()
        }
    })
}

/// `style` saying `label`, as a straight-alpha 512×512 image.
pub(crate) fn render(style: Style, label: &str) -> Option<RgbaImage> {
    rasterize(&svg(style, label))
}

/// A 512×512 SVG with Manrope Bold available, as a straight-alpha image.
pub(crate) fn rasterize(svg: &str) -> Option<RgbaImage> {
    let tree = Tree::from_str(svg, options()).ok()?;
    let mut pixmap = Pixmap::new(SIDE, SIDE)?;
    resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());
    let straight = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    RgbaImage::from_raw(SIDE, SIDE, straight)
}

/// Any image, scaled to fill 512×512.
pub(crate) fn decode_square(bytes: &[u8]) -> Option<RgbaImage> {
    let image = image::load_from_memory(bytes).ok()?;
    Some(
        image
            .resize_to_fill(SIDE, SIDE, imageops::FilterType::Lanczos3)
            .to_rgba8(),
    )
}

/// `overlay` over `base`, both 512×512.
pub(crate) fn composite(base: &RgbaImage, overlay: &RgbaImage) -> RgbaImage {
    let mut out = base.clone();
    imageops::overlay(&mut out, overlay, 0, 0);
    out
}

pub(crate) fn png(image: &RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    // Encoding an in-memory RGBA buffer to PNG cannot fail.
    let _ = image.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_draws_its_label_with_the_bundled_font() {
        for style in Style::ALL {
            let with = render(style, "6 Hours").unwrap();
            let without = render(style, "").unwrap();
            assert_eq!(with.dimensions(), (SIDE, SIDE));
            assert_ne!(with, without, "{style:?} drew no text");
            // Transparent somewhere, so the icon shows through.
            assert_eq!(with.get_pixel(256, 300)[3], 0, "{style:?}");
        }
    }

    #[test]
    fn labels_are_escaped() {
        assert!(svg(Style::Hourglass, "<1 & 2>").contains("&lt;1 &amp; 2&gt;"));
    }

    #[test]
    fn composite_keeps_the_base_under_transparency() {
        let base = RgbaImage::from_pixel(SIDE, SIDE, image::Rgba([10, 200, 30, 255]));
        let overlay = render(Style::Hourglass, "1 Hour").unwrap();
        let out = composite(&base, &overlay);
        assert_eq!(out.get_pixel(256, 100), base.get_pixel(256, 100));
        assert_ne!(out.get_pixel(260, 430), base.get_pixel(260, 430));
        assert!(decode_square(&png(&out)).is_some());
    }
}
