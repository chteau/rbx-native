//! Ctrl+S: writes the current DOM back to the file it was opened from, in the
//! format that file was already in — binary or XML, sniffed once at load and
//! never re-guessed afterwards (see [`Format::sniff`], called from
//! `main::load`; the Shell-facing wiring lives in `shell::save`).
//!
//! The write never touches the original file until the new content is fully
//! on disk: it lands in a sibling `<name>.tmp-<pid>` file first, and only a
//! fully written temp file is renamed over the original. A serializer error
//! — e.g. `rbx_binary::SerializeError::InconsistentProperty` from a
//! script-created instance with a property the binary format cannot express
//! — returns before that rename ever happens, so a bad DOM never corrupts a
//! file that was fine before the save was attempted.
//!
//! `RBX_STUDIO_SAVE_AS=<path>` redirects one save to a scratch path instead of
//! the file the place was opened from, applied once at startup right after
//! any `RBX_STUDIO_INSERT` mutation — a debugging aid for a headless
//! save/reload round trip, since nothing else can send Ctrl+S to the editor
//! on its own (see `AGENTS.md`'s safety rules).

use std::path::Path;

use gpui_kit::Modifiers;
use rbx_dom::WeakDom;

/// Read once at startup by `Shell::apply_debug_save`; documented in this
/// module's doc comment.
pub(crate) const SAVE_AS_VARIABLE: &str = "RBX_STUDIO_SAVE_AS";

/// Which serializer a place's file was read with — the one a save must use
/// again, since Ctrl+S never changes a file's format under the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Binary,
    Xml,
}

impl Format {
    /// Sniffs `bytes` exactly as `rbx_viewer::read_place` does when picking a
    /// deserializer. That function does not expose which branch it took, so
    /// `main::load` sniffs the same bytes a second time here rather than
    /// changing `rbx_viewer`'s public surface for this.
    pub(crate) fn sniff(bytes: &[u8]) -> Self {
        if rbx_xml::is_xml(bytes) {
            Format::Xml
        } else {
            Format::Binary
        }
    }

    fn encode(self, dom: &WeakDom) -> Result<Vec<u8>, String> {
        match self {
            Format::Binary => rbx_binary::serialize(dom).map_err(|err| err.to_string()),
            Format::Xml => rbx_xml::serialize(dom)
                .map(String::into_bytes)
                .map_err(|err| err.to_string()),
        }
    }
}

/// What Ctrl+S does — the only window-level shortcut this module owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Save,
}

/// Maps one keystroke to the save action. Mirrors `shell::keys::action_for`'s
/// pure-function shape for the same reason: the mapping is testable without a
/// window, and `shell::save` only has to call it.
pub(crate) fn action_for(key: &str, modifiers: Modifiers) -> Option<Action> {
    match key {
        "s" if modifiers.control => Some(Action::Save),
        _ => None,
    }
}

/// Serializes `dom` in `format` and writes it atomically to `path`.
pub(crate) fn save(dom: &WeakDom, format: Format, path: &Path) -> Result<(), String> {
    let bytes = format.encode(dom)?;
    write_atomic(path, &bytes)
}

/// Writes `bytes` to `path` without ever leaving a half-written file behind:
/// the new content lands in a sibling temp file first, and only a fully
/// written temp file is renamed over the original — mirrors
/// `rbx_assets::native`'s own `write_atomic`, which is private to that crate
/// and so duplicated here rather than shared across a crate boundary for one
/// small helper.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

    std::fs::write(&tmp_path, bytes)
        .map_err(|err| format!("failed to write {tmp_path:?}: {err}"))?;
    std::fs::rename(&tmp_path, path).map_err(|err| format!("failed to save {path:?}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::{NumberSequence, Variant};

    /// A per-process, per-test scratch path: unique enough that parallel test
    /// threads never collide on the same file.
    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "rbx_studio_save_test_{}_{name}",
            std::process::id()
        ));
        path
    }

    #[test]
    fn a_binary_save_round_trips_through_rbx_binary() {
        let mut dom = WeakDom::new();
        dom.new_instance("Part", "Saved", None);
        let path = temp_path("round_trip.rbxl");

        save(&dom, Format::Binary, &path).expect("save should succeed");
        let reloaded = rbx_binary::deserialize(&std::fs::read(&path).expect("file should exist"))
            .expect("bytes should deserialize");
        assert!(crate::explorer::find_by_name(&reloaded, "Saved").is_some());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_xml_save_round_trips_through_rbx_xml() {
        let mut dom = WeakDom::new();
        dom.new_instance("Part", "Saved", None);
        let path = temp_path("round_trip.rbxlx");

        save(&dom, Format::Xml, &path).expect("save should succeed");
        let text = std::fs::read_to_string(&path).expect("file should exist");
        let reloaded = rbx_xml::deserialize(&text).expect("text should deserialize");
        assert!(crate::explorer::find_by_name(&reloaded, "Saved").is_some());

        std::fs::remove_file(&path).ok();
    }

    /// A hand-written place may use a property's canonical name; the place
    /// is read with the name Studio saves instead (see
    /// `rbx_reflection::ReflectionDatabase::normalize_names`), and saved
    /// back with it.
    #[test]
    fn a_canonical_name_in_a_file_is_saved_as_studio_saves_it() {
        let original = temp_path("canonical.rbxlx");
        std::fs::write(
            &original,
            r#"<roblox version="4"><Item class="Part" referent="RBX0"><Properties>
            <string name="Name">Written</string>
            <Vector3 name="Size"><X>5</X><Y>6</Y><Z>7</Z></Vector3>
            <Color3 name="Color"><R>1</R><G>0</G><B>0</B></Color3>
            </Properties></Item></roblox>"#,
        )
        .unwrap();

        let dom = rbx_viewer::read_place(&original).unwrap();
        let part = crate::explorer::find_by_name(&dom, "Written").unwrap();
        let properties = dom.get(part).unwrap().properties();
        assert!(properties.contains_key("size") && !properties.contains_key("Size"));
        assert_eq!(
            properties.get("Color3uint8"),
            Some(&Variant::Color3uint8 { r: 255, g: 0, b: 0 })
        );

        let saved = temp_path("canonical_saved.rbxlx");
        save(&dom, Format::Xml, &saved).unwrap();
        let text = std::fs::read_to_string(&saved).unwrap();
        assert!(text.contains(r#"name="size""#) && text.contains(r#"name="Color3uint8""#));
        assert!(!text.contains(r#"name="Size""#) && !text.contains(r#"name="Color""#));

        std::fs::remove_file(&original).ok();
        std::fs::remove_file(&saved).ok();
    }

    #[test]
    fn sniffing_a_binary_header_picks_binary() {
        let mut dom = WeakDom::new();
        dom.new_instance("Part", "Saved", None);
        let bytes = rbx_binary::serialize(&dom).expect("serialize");
        assert_eq!(Format::sniff(&bytes), Format::Binary);
    }

    #[test]
    fn sniffing_an_xml_document_picks_xml() {
        let mut dom = WeakDom::new();
        dom.new_instance("Part", "Saved", None);
        let text = rbx_xml::serialize(&dom).expect("serialize");
        assert_eq!(Format::sniff(text.as_bytes()), Format::Xml);
    }

    /// A place opened as binary must be saved back as binary even after a
    /// round trip through a fresh in-memory DOM built the same way
    /// `main::load` builds one — the "remembers its format" contract this
    /// module exists for.
    #[test]
    fn a_place_opened_as_binary_saves_as_binary_again() {
        let mut seed = WeakDom::new();
        seed.new_instance("Part", "Seed", None);
        let original = temp_path("remember_binary.rbxl");
        std::fs::write(&original, rbx_binary::serialize(&seed).unwrap()).unwrap();

        let bytes = std::fs::read(&original).unwrap();
        let format = Format::sniff(&bytes);
        let dom = rbx_binary::deserialize(&bytes).unwrap();
        assert_eq!(format, Format::Binary);

        let saved_to = temp_path("remember_binary_out.rbxl");
        save(&dom, format, &saved_to).expect("save should succeed");
        let reloaded = rbx_binary::deserialize(&std::fs::read(&saved_to).unwrap()).unwrap();
        assert!(crate::explorer::find_by_name(&reloaded, "Seed").is_some());

        std::fs::remove_file(&original).ok();
        std::fs::remove_file(&saved_to).ok();
    }

    #[test]
    fn a_place_opened_as_xml_saves_as_xml_again() {
        let mut seed = WeakDom::new();
        seed.new_instance("Part", "Seed", None);
        let original = temp_path("remember_xml.rbxlx");
        std::fs::write(&original, rbx_xml::serialize(&seed).unwrap()).unwrap();

        let text = std::fs::read_to_string(&original).unwrap();
        let format = Format::sniff(text.as_bytes());
        let dom = rbx_xml::deserialize(&text).unwrap();
        assert_eq!(format, Format::Xml);

        let saved_to = temp_path("remember_xml_out.rbxlx");
        save(&dom, format, &saved_to).expect("save should succeed");
        let reloaded_text = std::fs::read_to_string(&saved_to).unwrap();
        let reloaded = rbx_xml::deserialize(&reloaded_text).unwrap();
        assert!(crate::explorer::find_by_name(&reloaded, "Seed").is_some());

        std::fs::remove_file(&original).ok();
        std::fs::remove_file(&saved_to).ok();
    }

    // Mirrors `rbx_binary::serialize`'s own
    // `a_kind_with_no_neutral_value_is_still_rejected_when_missing` test: two
    // instances of the same class disagree on a property with no neutral
    // fallback, which is exactly the "script-created instance with an
    // unfillable type" case the task brief calls out.
    #[test]
    fn a_failed_serialize_never_touches_the_existing_file() {
        let path = temp_path("untouched.rbxl");
        std::fs::write(&path, b"original content").expect("seed the file");

        let mut dom = WeakDom::new();
        let a = dom.new_instance("ParticleEmitter", "A", None);
        let _b = dom.new_instance("ParticleEmitter", "B", None);
        dom.set_property(
            a,
            "Transparency",
            Variant::NumberSequence(NumberSequence { keypoints: vec![] }),
        )
        .unwrap();

        let result = save(&dom, Format::Binary, &path);

        assert!(result.is_err());
        assert_eq!(
            std::fs::read(&path).expect("file should still exist"),
            b"original content"
        );
        let tmp_path = path.with_file_name(format!(
            "{}.tmp-{}",
            path.file_name().unwrap().to_string_lossy(),
            std::process::id()
        ));
        assert!(
            !tmp_path.exists(),
            "a failed serialize must not leave a temp file behind"
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn ctrl_s_saves() {
        let modifiers = Modifiers {
            control: true,
            ..Modifiers::none()
        };
        assert_eq!(action_for("s", modifiers), Some(Action::Save));
    }

    #[test]
    fn s_without_control_does_nothing() {
        assert_eq!(action_for("s", Modifiers::none()), None);
    }

    #[test]
    fn any_other_key_with_control_does_nothing() {
        let modifiers = Modifiers {
            control: true,
            ..Modifiers::none()
        };
        assert_eq!(action_for("a", modifiers), None);
    }
}
