//! Reads the `<file>.merge.json` the `rbxmerge` git driver leaves beside a
//! file it could not merge cleanly (the format is documented where it is
//! written, `rbx_parser_cli`'s `src/merge/sidecar.rs`).

use std::path::{Path, PathBuf};

use rbx_dom::WeakDom;

/// What a sidecar hands back: the base and theirs DOMs the driver merged.
pub(super) struct Sidecar {
    pub(super) path: PathBuf,
    pub(super) base: WeakDom,
    pub(super) theirs: WeakDom,
    /// The `file` field: the path git merged, for the window's subtitle.
    pub(super) file: String,
}

/// `<place>.merge.json`, the sidecar path for `place`.
pub(super) fn path_for(place: &Path) -> PathBuf {
    let mut name = place.as_os_str().to_owned();
    name.push(".merge.json");
    PathBuf::from(name)
}

/// Reads and decodes the sidecar at `path`. `Err` names what is wrong
/// (unreadable, not JSON, unknown version, base64 or place bytes bad).
pub(super) fn read(path: &Path) -> Result<Sidecar, String> {
    use base64::Engine as _;

    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;
    let version = json.get("version").and_then(serde_json::Value::as_u64);
    if version != Some(1) {
        return Err(format!(
            "{}: unsupported merge sidecar version {}",
            path.display(),
            version.map_or("(missing)".to_string(), |v| v.to_string())
        ));
    }
    let decode = |field: &str| -> Result<WeakDom, String> {
        let encoded = json
            .get(field)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("{}: missing \"{field}\"", path.display()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| format!("{}: \"{field}\" is not base64: {e}", path.display()))?;
        rbx_viewer::parse_place(&bytes, path)
            .map_err(|e| format!("{}: \"{field}\": {e}", path.display()))
    };
    let base = decode("base")?;
    let theirs = decode("theirs")?;
    let file = json
        .get("file")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(Sidecar {
        path: path.to_path_buf(),
        base,
        theirs,
        file,
    })
}
