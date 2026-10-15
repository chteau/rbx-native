//! The per-place secret, kept inside the place: a `Folder` named
//! [`FOLDER_NAME`] in `ServerStorage` with a string attribute
//! [`ATTRIBUTE`]. `ServerStorage` is never sent to game clients, and Team
//! Create replicates it to every collaborator, so the people who can edit
//! the place are the ones who can read it. Anyone holding a copy of the
//! place file can read it too (an uncopylocked place gives it away).

use rbx_dom::{attributes, Ref, Variant, WeakDom};

use crate::relay::{RelayConfig, RelayKind};

pub const SERVICE: &str = "ServerStorage";
pub const FOLDER_NAME: &str = "RbxNativePresence";
pub const ATTRIBUTE: &str = "PresenceSecret";
/// The relay transport the place's owner chose ("worker" or "ntfy").
pub const RELAY_KIND_ATTRIBUTE: &str = "RelayKind";
/// The relay base URL the place's owner set.
pub const RELAY_URL_ATTRIBUTE: &str = "RelayUrl";

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

/// The relay the place names, if any: a well-formed `RelayKind` and a
/// non-empty `RelayUrl` attribute on the folder. `None` (the owner set
/// nothing, or it is malformed) means the caller should fall back to
/// [`RelayConfig::community`].
pub fn read_relay(dom: &WeakDom) -> Option<RelayConfig> {
    let folder = find_folder(dom, find_service(dom)?)?;
    let attributes = dom.get(folder)?.attributes();
    let kind = match attributes.get(RELAY_KIND_ATTRIBUTE) {
        Some(Variant::String(word)) => RelayKind::parse(word)?,
        _ => return None,
    };
    match attributes.get(RELAY_URL_ATTRIBUTE) {
        Some(Variant::String(url)) if !url.trim().is_empty() => Some(RelayConfig {
            kind,
            url: url.trim().to_owned(),
        }),
        _ => None,
    }
}

/// Writes the relay config into the place (reusing or creating the folder),
/// the way [`write`] writes the secret. `None` when the place has no
/// `ServerStorage`.
pub fn write_relay(dom: &mut WeakDom, config: &RelayConfig) -> Option<Ref> {
    write_attributes(
        dom,
        &[
            (RELAY_KIND_ATTRIBUTE, config.kind.as_str().to_owned()),
            (RELAY_URL_ATTRIBUTE, config.url.trim().to_owned()),
        ],
    )
}

/// Removes the relay attributes from the place (if the folder exists),
/// falling the place back to the community default. Returns the folder when
/// it changed anything. A no-op when there is nothing to clear.
pub fn clear_relay(dom: &mut WeakDom) -> Option<Ref> {
    let folder = find_folder(dom, find_service(dom)?)?;
    let mut attributes = dom.get(folder)?.attributes();
    let had = attributes.contains_key(RELAY_KIND_ATTRIBUTE)
        || attributes.contains_key(RELAY_URL_ATTRIBUTE);
    if !had {
        return None;
    }
    attributes.remove(RELAY_KIND_ATTRIBUTE);
    attributes.remove(RELAY_URL_ATTRIBUTE);
    let raw = attributes::encode(&attributes, |_| None)?;
    dom.set_property(
        folder,
        "AttributesSerialize",
        Variant::Unknown { type_id: 0x01, raw },
    )
    .ok()?;
    Some(folder)
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
    write_attributes(dom, &[(ATTRIBUTE, secret.to_owned())])
}

/// Sets each `(name, value)` string attribute on the folder (reused or
/// created under `ServerStorage`), preserving the folder's other attributes.
/// Returns the folder, or `None` when the place has no `ServerStorage`.
fn write_attributes(dom: &mut WeakDom, pairs: &[(&str, String)]) -> Option<Ref> {
    let service = find_service(dom)?;
    let folder = match find_folder(dom, service) {
        Some(f) => f,
        None => dom.new_instance("Folder", FOLDER_NAME, Some(service)),
    };
    let mut attributes = dom.get(folder)?.attributes();
    for (name, value) in pairs {
        attributes.insert((*name).to_string(), Variant::String(value.clone()));
    }
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
    fn relay_config_round_trips_beside_the_secret() {
        let mut dom = place();
        let secret = generate();
        write(&mut dom, &secret).unwrap();
        assert_eq!(read_relay(&dom), None);
        let config = RelayConfig {
            kind: RelayKind::Ntfy,
            url: "https://ntfy.sh".to_owned(),
        };
        write_relay(&mut dom, &config).unwrap();
        assert_eq!(read_relay(&dom), Some(config));
        // Writing the relay keeps the secret.
        assert_eq!(read(&dom), Some(secret));
        let service = find_service(&dom).unwrap();
        assert_eq!(dom.get(service).unwrap().children().len(), 1);
    }

    #[test]
    fn malformed_relay_reads_none() {
        let mut dom = place();
        write_relay(
            &mut dom,
            &RelayConfig {
                kind: RelayKind::Worker,
                url: "https://r.example".to_owned(),
            },
        )
        .unwrap();
        // A kind word that is not one we know reads as no relay.
        let folder = find_folder(&dom, find_service(&dom).unwrap()).unwrap();
        let mut attrs = dom.get(folder).unwrap().attributes();
        attrs.insert(
            RELAY_KIND_ATTRIBUTE.to_string(),
            Variant::String("carrier-pigeon".to_owned()),
        );
        let raw = attributes::encode(&attrs, |_| None).unwrap();
        dom.set_property(
            folder,
            "AttributesSerialize",
            Variant::Unknown { type_id: 0x01, raw },
        )
        .unwrap();
        assert_eq!(read_relay(&dom), None);
    }

    #[test]
    fn no_server_storage_writes_none() {
        let mut dom = WeakDom::new();
        dom.new_instance("Workspace", "Workspace", None);
        assert_eq!(write(&mut dom, &generate()), None);
        assert_eq!(read(&dom), None);
    }
}
