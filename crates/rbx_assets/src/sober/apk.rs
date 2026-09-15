//! Looks up a single texture entry inside Sober's downloaded Roblox Android
//! client APK, which is itself a plain zip archive.

use std::io::{Cursor, Read};

use super::SoberError;

/// Roblox's Android APK bundles most Studio content textures under
/// `assets/content/`, with a handful of extras under `assets/ExtraContent/`
/// (both observed empirically in Sober's downloaded `base.apk`).
pub(super) fn extract_from_apk(
    apk_bytes: &[u8],
    relative_path: &str,
) -> Result<Vec<u8>, SoberError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(apk_bytes)).map_err(|e| SoberError::Zip(e.to_string()))?;

    for prefix in ["assets/content/", "assets/ExtraContent/"] {
        let candidate = format!("{prefix}{relative_path}");
        if let Ok(mut entry) = archive.by_name(&candidate) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| SoberError::Io(e.to_string()))?;
            return Ok(buf);
        }
    }
    Err(SoberError::NotFound(relative_path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, data) in entries {
                writer.start_file(*name, options).unwrap();
                std::io::Write::write_all(&mut writer, data).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn finds_entry_under_content() {
        let zip = build_test_zip(&[("assets/content/textures/face.png", b"pngdata")]);
        assert_eq!(
            extract_from_apk(&zip, "textures/face.png").unwrap(),
            b"pngdata"
        );
    }

    /// The second lookup attempt this module makes: some files only exist
    /// under `ExtraContent`, not `content`.
    #[test]
    fn falls_back_to_extra_content() {
        let zip = build_test_zip(&[("assets/ExtraContent/textures/foo.png", b"extra")]);
        assert_eq!(
            extract_from_apk(&zip, "textures/foo.png").unwrap(),
            b"extra"
        );
    }

    #[test]
    fn prefers_content_over_extra_content_when_both_present() {
        let zip = build_test_zip(&[
            ("assets/content/textures/dup.png", b"main"),
            ("assets/ExtraContent/textures/dup.png", b"extra"),
        ]);
        assert_eq!(extract_from_apk(&zip, "textures/dup.png").unwrap(), b"main");
    }

    #[test]
    fn missing_entry_is_reported() {
        let zip = build_test_zip(&[("assets/content/textures/other.png", b"x")]);
        let err = extract_from_apk(&zip, "textures/face.png");
        assert!(matches!(err, Err(SoberError::NotFound(_))));
    }
}
