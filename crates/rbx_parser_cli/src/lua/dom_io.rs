//! Reads and writes place/model files for `rbxlua`, mirroring the format
//! sniffing `rbxdump` uses (magic bytes, not the extension) so a misnamed
//! file still loads.

use std::path::Path;

use rbx_dom::WeakDom;

pub fn read_dom(path: &Path) -> Result<WeakDom, String> {
    let display = path.display();
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read '{display}': {err}"))?;

    if rbx_xml::is_xml(&bytes) {
        let text = std::str::from_utf8(&bytes)
            .map_err(|err| format!("'{display}' is not valid UTF-8 XML: {err}"))?;
        rbx_xml::deserialize(text).map_err(|err| format!("failed to parse '{display}': {err}"))
    } else {
        rbx_binary::deserialize(&bytes).map_err(|err| format!("failed to parse '{display}': {err}"))
    }
}

/// Writes `dom` to `path`, picking the binary or XML codec from the file
/// extension since, unlike reading, there is no byte content yet to sniff.
pub fn write_dom(path: &Path, dom: &WeakDom) -> Result<(), String> {
    let display = path.display();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();

    match extension {
        "rbxl" | "rbxm" => {
            let bytes = rbx_binary::serialize(dom)
                .map_err(|err| format!("failed to serialize '{display}': {err}"))?;
            std::fs::write(path, bytes).map_err(|err| format!("failed to write '{display}': {err}"))
        }
        "rbxlx" | "rbxmx" => {
            let xml = rbx_xml::serialize(dom)
                .map_err(|err| format!("failed to serialize '{display}': {err}"))?;
            std::fs::write(path, xml).map_err(|err| format!("failed to write '{display}': {err}"))
        }
        _ => Err(format!(
            "'{display}' must end in .rbxl, .rbxm, .rbxlx or .rbxmx"
        )),
    }
}
