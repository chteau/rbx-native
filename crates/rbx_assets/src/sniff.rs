//! Byte-level format detection: PNG, JPEG, WebP, BMP, TGA (by footer), Roblox
//! mesh, models, audio, and unknown. Assets are identified by magic bytes since
//! Roblox serves them without file extensions.

/// The concrete format detected by [`sniff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetKind {
    Png,
    Jpeg,
    Webp,
    Bmp,
    Tga,
    /// A DXT-compressed DDS file — including Roblox's default skybox and many
    /// particle textures, which ship under a `.tex` extension but the same
    /// `"DDS "` magic.
    Dds,
    /// A Roblox mesh file; `version` is the literal header token (e.g. `"4.00"`).
    RobloxMesh {
        version: String,
    },
    RobloxBinaryModel,
    RobloxXmlModel,
    Ogg,
    Mp3,
    Webm,
    Unknown,
}

impl AssetKind {
    /// The conventional file extension for this kind, if any, with no leading dot.
    pub fn extension(&self) -> Option<&'static str> {
        match self {
            AssetKind::Png => Some("png"),
            AssetKind::Jpeg => Some("jpg"),
            AssetKind::Webp => Some("webp"),
            AssetKind::Bmp => Some("bmp"),
            AssetKind::Tga => Some("tga"),
            AssetKind::Dds => Some("dds"),
            AssetKind::RobloxMesh { .. } => Some("mesh"),
            AssetKind::RobloxBinaryModel => Some("rbxm"),
            AssetKind::RobloxXmlModel => Some("rbxmx"),
            AssetKind::Ogg => Some("ogg"),
            AssetKind::Mp3 => Some("mp3"),
            AssetKind::Webm => Some("webm"),
            AssetKind::Unknown => None,
        }
    }
}

const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const TGA_FOOTER: &[u8] = b"TRUEVISION-XFILE.";

/// Detects the format of `data` from its magic bytes (or, for TGA, its footer).
///
/// TGA has no header magic bytes; only its footer unambiguously identifies the
/// format.
pub fn sniff(data: &[u8]) -> AssetKind {
    if data.starts_with(&PNG_MAGIC) {
        return AssetKind::Png;
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return AssetKind::Jpeg;
    }
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return AssetKind::Webp;
    }
    if data.starts_with(b"BM") {
        return AssetKind::Bmp;
    }
    if data.starts_with(b"DDS ") {
        return AssetKind::Dds;
    }
    if let Some(version) = sniff_mesh_version(data) {
        return AssetKind::RobloxMesh { version };
    }
    if data.starts_with(b"<roblox!") {
        return AssetKind::RobloxBinaryModel;
    }
    if data.starts_with(b"<roblox") {
        return AssetKind::RobloxXmlModel;
    }
    if data.starts_with(b"OggS") {
        return AssetKind::Ogg;
    }
    if data.starts_with(b"ID3")
        || data.starts_with(&[0xFF, 0xFB])
        || data.starts_with(&[0xFF, 0xF3])
        || data.starts_with(&[0xFF, 0xF2])
    {
        return AssetKind::Mp3;
    }
    if data.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return AssetKind::Webm;
    }
    // TGA has no header magic; only its footer identifies it reliably. The
    // signature field is padded to 18 bytes with a trailing NUL, so search
    // rather than compare at a fixed offset.
    if data.len() >= 26
        && data[data.len() - 26..]
            .windows(TGA_FOOTER.len())
            .any(|w| w == TGA_FOOTER)
    {
        return AssetKind::Tga;
    }
    AssetKind::Unknown
}

/// Roblox mesh files are ASCII text starting with `version <major>.<minor>`.
/// Major versions 1-7 have been observed in the wild; any other major version
/// is rejected to avoid false positives on arbitrary text starting with "version".
fn sniff_mesh_version(data: &[u8]) -> Option<String> {
    let prefix_len = data.len().min(32);
    let text = std::str::from_utf8(&data[..prefix_len]).ok()?;
    let rest = text.strip_prefix("version ")?;
    let version: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let major = version.chars().next()?;
    ('1'..='7').contains(&major).then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_png() {
        assert_eq!(sniff(&PNG_MAGIC), AssetKind::Png);
    }

    #[test]
    fn sniffs_jpeg() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), AssetKind::Jpeg);
    }

    #[test]
    fn sniffs_webp() {
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&[0, 0, 0, 0]); // chunk size, irrelevant here
        data.extend_from_slice(b"WEBP");
        assert_eq!(sniff(&data), AssetKind::Webp);
    }

    #[test]
    fn sniffs_bmp() {
        assert_eq!(sniff(b"BM....rest"), AssetKind::Bmp);
    }

    #[test]
    fn sniffs_dds() {
        assert_eq!(sniff(b"DDS |DXT1restofheader"), AssetKind::Dds);
    }

    #[test]
    fn sniffs_roblox_mesh_and_keeps_version() {
        assert_eq!(
            sniff(b"version 4.00\nheader stuff"),
            AssetKind::RobloxMesh {
                version: "4.00".to_string()
            }
        );
        assert_eq!(
            sniff(b"version 1.00\n"),
            AssetKind::RobloxMesh {
                version: "1.00".to_string()
            }
        );
        assert_eq!(
            sniff(b"version 7.00\n"),
            AssetKind::RobloxMesh {
                version: "7.00".to_string()
            }
        );
    }

    #[test]
    fn rejects_out_of_range_mesh_version() {
        // Major version 9 has never existed; don't misdetect arbitrary text.
        assert_eq!(sniff(b"version 9.00\n"), AssetKind::Unknown);
    }

    #[test]
    fn sniffs_rbxm_binary_model() {
        assert_eq!(
            sniff(b"<roblox!\x89\xff\x0d\x0a\x1a\x0a"),
            AssetKind::RobloxBinaryModel
        );
    }

    #[test]
    fn sniffs_rbxmx_xml_model() {
        assert_eq!(
            sniff(b"<roblox xmlns:xmime=\"...\">"),
            AssetKind::RobloxXmlModel
        );
    }

    #[test]
    fn sniffs_ogg() {
        assert_eq!(sniff(b"OggS\x00\x02"), AssetKind::Ogg);
    }

    #[test]
    fn sniffs_mp3_id3_tag() {
        assert_eq!(sniff(b"ID3\x03\x00"), AssetKind::Mp3);
    }

    #[test]
    fn sniffs_mp3_frame_sync() {
        assert_eq!(sniff(&[0xFF, 0xFB, 0x90]), AssetKind::Mp3);
        assert_eq!(sniff(&[0xFF, 0xF3, 0x90]), AssetKind::Mp3);
        assert_eq!(sniff(&[0xFF, 0xF2, 0x90]), AssetKind::Mp3);
    }

    #[test]
    fn sniffs_webm() {
        assert_eq!(sniff(&[0x1A, 0x45, 0xDF, 0xA3, 0x00]), AssetKind::Webm);
    }

    #[test]
    fn sniffs_tga_by_footer() {
        let mut data = vec![0u8; 8]; // fake pixel data
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // extension/dev dir offsets
        data.extend_from_slice(b"TRUEVISION-XFILE.");
        assert_eq!(sniff(&data), AssetKind::Tga);
    }

    #[test]
    fn unknown_for_garbage() {
        assert_eq!(sniff(b"not a real asset"), AssetKind::Unknown);
    }

    #[test]
    fn unknown_for_empty() {
        assert_eq!(sniff(&[]), AssetKind::Unknown);
    }

    #[test]
    fn extensions_map_correctly() {
        assert_eq!(AssetKind::Png.extension(), Some("png"));
        assert_eq!(AssetKind::Jpeg.extension(), Some("jpg"));
        assert_eq!(AssetKind::Webp.extension(), Some("webp"));
        assert_eq!(AssetKind::Bmp.extension(), Some("bmp"));
        assert_eq!(AssetKind::Tga.extension(), Some("tga"));
        assert_eq!(AssetKind::Dds.extension(), Some("dds"));
        assert_eq!(
            AssetKind::RobloxMesh {
                version: "4.00".to_string()
            }
            .extension(),
            Some("mesh")
        );
        assert_eq!(AssetKind::RobloxBinaryModel.extension(), Some("rbxm"));
        assert_eq!(AssetKind::RobloxXmlModel.extension(), Some("rbxmx"));
        assert_eq!(AssetKind::Ogg.extension(), Some("ogg"));
        assert_eq!(AssetKind::Mp3.extension(), Some("mp3"));
        assert_eq!(AssetKind::Webm.extension(), Some("webm"));
        assert_eq!(AssetKind::Unknown.extension(), None);
    }
}
