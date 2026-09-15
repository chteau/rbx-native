//! Image decoding for resolved assets.

use image::RgbaImage;

use crate::error::AssetError;
use crate::resolver::Asset;
use crate::sniff::AssetKind;

/// Decodes an [`Asset`] to RGBA8, using its sniffed [`AssetKind`] to pick the
/// decoder rather than trusting a (nonexistent) file extension.
pub fn decode_image(asset: &Asset) -> Result<RgbaImage, AssetError> {
    if asset.kind == AssetKind::Dds {
        return crate::dds::decode(&asset.bytes);
    }
    let format = image_format_for(&asset.kind).ok_or_else(|| {
        AssetError::ImageDecode(format!("{:?} is not a decodable image format", asset.kind))
    })?;
    image::load_from_memory_with_format(&asset.bytes, format)
        .map(|img| img.to_rgba8())
        .map_err(|e| AssetError::ImageDecode(e.to_string()))
}

fn image_format_for(kind: &AssetKind) -> Option<image::ImageFormat> {
    match kind {
        AssetKind::Png => Some(image::ImageFormat::Png),
        AssetKind::Jpeg => Some(image::ImageFormat::Jpeg),
        AssetKind::Webp => Some(image::ImageFormat::WebP),
        AssetKind::Bmp => Some(image::ImageFormat::Bmp),
        AssetKind::Tga => Some(image::ImageFormat::Tga),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sniff::sniff;

    /// A hand-built 2x2 PNG, since we can't reach into the network or the
    /// filesystem for test fixtures here.
    fn tiny_png_bytes() -> Vec<u8> {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 255, 0, 255]));
        img.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
        img.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn decodes_a_png_asset() {
        let bytes = tiny_png_bytes();
        let kind = sniff(&bytes);
        assert_eq!(kind, AssetKind::Png);
        let asset = Asset { bytes, kind };
        let decoded = decode_image(&asset).unwrap();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert_eq!(*decoded.get_pixel(0, 0), image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn refuses_non_image_kind() {
        let asset = Asset {
            bytes: b"not an image".to_vec(),
            kind: AssetKind::Unknown,
        };
        assert!(decode_image(&asset).is_err());
    }
}
