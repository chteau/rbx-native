//! Reading a place file (`.rbxl` or `.rbxlx`) into a DOM.

use std::path::Path;

use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

pub fn read_place(path: &Path) -> Result<WeakDom, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read {path:?}: {err}"))?;
    parse_place(&bytes).map_err(|err| format!("failed to parse {path:?}: {err}"))
}

/// Either format, with property names normalised the way `rbx_viewer`
/// does it, and an empty change log: loading isn't an edit.
pub fn parse_place(bytes: &[u8]) -> Result<WeakDom, String> {
    let database = ReflectionDatabase::shared();
    let mut dom = if rbx_xml::is_xml(bytes) {
        let text = std::str::from_utf8(bytes).map_err(|err| err.to_string())?;
        let mut dom = rbx_xml::deserialize(text).map_err(|err| err.to_string())?;
        database.normalize_names(&mut dom);
        dom
    } else {
        let (mut dom, names) =
            rbx_binary::deserialize_with_names(bytes).map_err(|err| err.to_string())?;
        database.normalize_spellings(
            &mut dom,
            names
                .iter()
                .map(|(class, property)| (class.as_str(), property.as_str())),
        );
        dom
    };
    dom.take_changes();
    Ok(dom)
}
