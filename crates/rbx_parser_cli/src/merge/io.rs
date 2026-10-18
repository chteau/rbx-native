//! Reads and writes place files for `rbxmerge`, sniffing the format from the
//! bytes because git hands the driver temp files with no extension.

use std::path::Path;

use rbx_dom::WeakDom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Binary,
    Xml,
}

pub fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|err| format!("failed to read '{}': {err}", path.display()))
}

pub fn parse(bytes: &[u8], path: &Path) -> Result<(WeakDom, Format), String> {
    let display = path.display();
    if rbx_xml::is_xml(bytes) {
        let text = std::str::from_utf8(bytes)
            .map_err(|err| format!("'{display}' is not valid UTF-8 XML: {err}"))?;
        let dom = rbx_xml::deserialize(text)
            .map_err(|err| format!("failed to parse '{display}': {err}"))?;
        Ok((dom, Format::Xml))
    } else {
        let dom = rbx_binary::deserialize(bytes)
            .map_err(|err| format!("failed to parse '{display}': {err}"))?;
        Ok((dom, Format::Binary))
    }
}

/// Serializes fully before returning so a failure cannot leave a
/// half-written file behind.
pub fn encode(dom: &WeakDom, format: Format) -> Result<Vec<u8>, String> {
    match format {
        Format::Binary => {
            // A class default for what an instance does not hold, not a
            // zero: see `rbx_binary::serialize_with_defaults`.
            let database = rbx_reflection::ReflectionDatabase::shared();
            rbx_binary::serialize_with_defaults(dom, |class, key| {
                database.stored_default(class, key).cloned()
            })
            .map_err(|err| format!("failed to serialize merged file: {err}"))
        }
        Format::Xml => rbx_xml::serialize(dom)
            .map(String::into_bytes)
            .map_err(|err| format!("failed to serialize merged file: {err}")),
    }
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes)
        .map_err(|err| format!("failed to write '{}': {err}", path.display()))
}
