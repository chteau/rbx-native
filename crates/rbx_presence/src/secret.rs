//! The per-place secret, kept inside the place: a `Folder` named
//! [`FOLDER_NAME`] in `ServerStorage` with a string attribute
//! [`ATTRIBUTE`]. `ServerStorage` is never sent to game clients, and Team
//! Create replicates it to every collaborator, so the people who can edit
//! the place are the ones who can read it. Anyone holding a copy of the
//! place file can read it too (an uncopylocked place gives it away).

use rbx_dom::{attributes, Ref, Variant, WeakDom};

pub const SERVICE: &str = "ServerStorage";
pub const FOLDER_NAME: &str = "RbxNativePresence";
pub const ATTRIBUTE: &str = "PresenceSecret";

/// The secret, if the place has a well-formed one (64 lower-case hex
/// digits).
pub fn read(dom: &WeakDom) -> Option<String> {
    let folder = find_folder(dom, find_service(dom)?)?;
    let attributes = dom.get(folder)?.attributes();
    match attributes.get(ATTRIBUTE) {
        Some(Variant::String(s))
            if s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
        {
            Some(s.clone())
        }
        _ => None,
    }
}

fn find_service(dom: &WeakDom) -> Option<Ref> {
    dom.root_refs().iter().copied().find(|r| {
        dom.get(*r)
            .is_some_and(|i| i.name() == SERVICE && i.class() == SERVICE)
    })
}

fn find_folder(dom: &WeakDom, service: Ref) -> Option<Ref> {
    dom.get(service)?.children().iter().copied().find(|r| {
        dom.get(*r)
            .is_some_and(|i| i.name() == FOLDER_NAME && i.class() == "Folder")
    })
}

/// A fresh 64-hex-digit secret from the OS random source.
pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    hex::encode(bytes)
}

/// Writes `secret` into the place: reuses an existing folder or creates one
/// under `ServerStorage` (`None` when the place has no `ServerStorage`).
/// Returns the folder. The change is logged in `dom`'s change list like any
/// other edit.
pub fn write(dom: &mut WeakDom, secret: &str) -> Option<Ref> {
    let service = find_service(dom)?;
    let folder = match find_folder(dom, service) {
        Some(f) => f,
        None => dom.new_instance("Folder", FOLDER_NAME, Some(service)),
    };
    let mut attributes = dom.get(folder)?.attributes();
    attributes.insert(ATTRIBUTE.to_string(), Variant::String(secret.to_string()));
    let raw = attributes::encode(&attributes, |_| None)?;
    dom.set_property(
        folder,
        "AttributesSerialize",
        Variant::Unknown { type_id: 0x01, raw },
    )
    .ok()?;
    Some(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> WeakDom {
        let mut dom = WeakDom::new();
        dom.new_instance(SERVICE, SERVICE, None);
        dom
    }

    #[test]
    fn write_then_read_round_trips() {
        let mut dom = place();
        let secret = generate();
        assert_eq!(secret.len(), 64);
        assert!(write(&mut dom, &secret).is_some());
        assert_eq!(read(&dom), Some(secret));
    }

    #[test]
    fn write_twice_reuses_folder() {
        let mut dom = place();
        let a = write(&mut dom, &generate()).unwrap();
        let second = generate();
        let b = write(&mut dom, &second).unwrap();
        assert_eq!(a, b);
        assert_eq!(read(&dom), Some(second));
        let service = find_service(&dom).unwrap();
        assert_eq!(dom.get(service).unwrap().children().len(), 1);
    }

    #[test]
    fn malformed_attribute_reads_none() {
        for bad in ["short", &"A".repeat(64), &"g".repeat(64)] {
            let mut dom = place();
            write(&mut dom, bad).unwrap();
            assert_eq!(read(&dom), None);
        }
    }

    #[test]
    fn no_server_storage_writes_none() {
        let mut dom = WeakDom::new();
        dom.new_instance("Workspace", "Workspace", None);
        assert_eq!(write(&mut dom, &generate()), None);
        assert_eq!(read(&dom), None);
    }
}
