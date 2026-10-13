//! The window's network half, blocking and run off the UI thread: reading
//! the linked experience's settings and saving edits to them.
//!
//! `RBX_STUDIO_GAME_SETTINGS_MOCK=fixture` answers with a canned experience
//! (saves echo the edits back) and `=<HTTP status>` fails the load with it,
//! for scripted captures; neither touches the network.

use rbx_cloud::{ApiKey, Client, CloudError};

use super::model::{Remote, Updates};
use crate::home::Link;

const MOCK_VARIABLE: &str = "RBX_STUDIO_GAME_SETTINGS_MOCK";

/// Reads the experience, its root place and the linked place (once when
/// they are the same).
pub(super) fn load(link: &Link) -> Result<Remote, String> {
    if let Some(mock) = mock_mode() {
        return mock_load(&mock);
    }
    let client = client()?;
    let experience = client
        .experience_settings(link.universe_id)
        .map_err(|e| describe_read(&e))?;
    let root_place = client
        .place_settings(link.universe_id, experience.root_place_id)
        .map_err(|e| describe_read(&e))?;
    let place = if link.place_id == experience.root_place_id {
        root_place.clone()
    } else {
        client
            .place_settings(link.universe_id, link.place_id)
            .map_err(|e| describe_read(&e))?
    };
    Ok(Remote {
        experience,
        root_place,
        place,
        place_id: link.place_id,
    })
}

/// Sends each non-empty update in `updates` (experience, then root place,
/// then place) and returns the settings Roblox holds afterwards. The first
/// refusal stops the rest: it comes back as a sentence naming the scope
/// it needed for a 401/403 (`universe:write` or `universe.place:write`),
/// and saying the place is open in Team Create for a 409, together with
/// the settings as the earlier requests left them (boxed: large), so the caller can tell
/// what did land. The sentence lists those parts when there are any.
pub(super) fn save(
    link: &Link,
    remote: &Remote,
    updates: &Updates,
) -> Result<Remote, (Box<Remote>, String)> {
    if let Some(mock) = mock_mode() {
        return mock_save(&mock, remote, updates);
    }
    let client = client().map_err(|e| (Box::new(remote.clone()), e))?;
    let universe = link.universe_id;
    let root_id = remote.experience.root_place_id;
    let mut next = remote.clone();
    let mut saved: Vec<&str> = Vec::new();
    let refused = |next: &Remote, saved: &[&str], e: &CloudError, scope: &str| {
        let mut text = describe(e, scope);
        if !saved.is_empty() {
            text.push_str(&format!(" Saved before that: {}.", saved.join(", ")));
        }
        (Box::new(next.clone()), text)
    };
    if !updates.experience.is_empty() {
        match client.update_experience_settings(universe, &updates.experience) {
            Ok(fresh) => next.experience = fresh,
            Err(e) => return Err(refused(&next, &saved, &e, "universe:write")),
        }
        saved.push("the experience settings");
    }
    if !updates.root_place.is_empty() {
        match client.update_place_settings(universe, root_id, &updates.root_place) {
            Ok(fresh) => next.root_place = fresh,
            Err(e) => return Err(refused(&next, &saved, &e, "universe.place:write")),
        }
        next.experience.display_name = next.root_place.display_name.clone();
        next.experience.description = next.root_place.description.clone();
        saved.push("the root place");
    }
    if !updates.place.is_empty() {
        match client.update_place_settings(universe, link.place_id, &updates.place) {
            Ok(fresh) => next.place = fresh,
            Err(e) => return Err(refused(&next, &saved, &e, "universe.place:write")),
        }
    }
    if link.place_id == root_id {
        next.place = next.root_place.clone();
    }
    Ok(next)
}

fn client() -> Result<Client, String> {
    ApiKey::from_env_or_config()
        .map(|key| Client::new(Some(key)))
        .ok_or_else(|| {
            "No Open Cloud API key is set up. Add one from Home \u{203a} Manage key.".to_string()
        })
}

/// A failed read: the GETs list no write scope, so a 401/403 means the key
/// lacks access to this experience rather than a scope.
fn describe_read(err: &CloudError) -> String {
    match err {
        CloudError::Http { status: 401 | 403, .. }
        | CloudError::Refused { status: 401 | 403, .. } => format!(
            "This API key can\u{2019}t read this experience. Check it is granted access to it in Home \u{203a} Manage key. ({err})"
        ),
        _ => describe(err, ""),
    }
}

fn describe(err: &CloudError, scope: &str) -> String {
    let status = match err {
        CloudError::Http { status, .. } | CloudError::Refused { status, .. } => Some(*status),
        _ => None,
    };
    let base = match (status, err) {
        (Some(401 | 403), _) => {
            format!("This key needs {scope} on this experience \u{2014} Home \u{203a} Manage key.")
        }
        (Some(404), _) => "There is no such experience or place.".to_string(),
        (Some(409), _) => "The place is open in Team Create; close it there first.".to_string(),
        (_, CloudError::Refused { message, .. }) => message.clone(),
        _ => err.to_string(),
    };
    format!("{base} ({err})")
}

fn mock_mode() -> Option<String> {
    std::env::var(MOCK_VARIABLE).ok()
}

fn mock_load(mode: &str) -> Result<Remote, String> {
    match mode.trim().parse::<u16>() {
        Ok(status) => Err(mock_failure(status)),
        Err(_) => Ok(mock_remote()),
    }
}

fn mock_save(
    mode: &str,
    remote: &Remote,
    updates: &Updates,
) -> Result<Remote, (Box<Remote>, String)> {
    if let Ok(status) = mode.trim().parse::<u16>() {
        return Err((Box::new(remote.clone()), mock_failure(status)));
    }
    let mut next = remote.clone();
    let e = &updates.experience;
    let x = &mut next.experience;
    if let Some(v) = e.voice_chat_enabled {
        x.voice_chat_enabled = v;
    }
    if let Some(v) = e.private_server_price_robux {
        x.private_server_price_robux = v;
    }
    if let Some(v) = e.desktop_enabled {
        x.desktop_enabled = v;
    }
    if let Some(v) = e.mobile_enabled {
        x.mobile_enabled = v;
    }
    if let Some(v) = e.tablet_enabled {
        x.tablet_enabled = v;
    }
    if let Some(v) = e.console_enabled {
        x.console_enabled = v;
    }
    if let Some(v) = e.vr_enabled {
        x.vr_enabled = v;
    }
    apply_place(&mut next.root_place, &updates.root_place);
    next.experience.display_name = next.root_place.display_name.clone();
    next.experience.description = next.root_place.description.clone();
    if next.place_id == next.experience.root_place_id {
        next.place = next.root_place.clone();
    } else {
        apply_place(&mut next.place, &updates.place);
    }
    Ok(next)
}

fn apply_place(place: &mut rbx_cloud::PlaceSettings, update: &rbx_cloud::PlaceUpdate) {
    if let Some(v) = &update.display_name {
        place.display_name = v.clone();
    }
    if let Some(v) = &update.description {
        place.description = v.clone();
    }
    if let Some(v) = update.server_size {
        place.server_size = v;
    }
}

fn mock_failure(status: u16) -> String {
    describe(
        &CloudError::Http {
            status,
            url: "mock://game-settings".to_string(),
        },
        "universe:write",
    )
}

fn mock_remote() -> Remote {
    let place = rbx_cloud::PlaceSettings {
        display_name: "Obby Rush".to_string(),
        description: "Race to the top of a tower of ever-meaner obstacles.".to_string(),
        server_size: 30,
        root: true,
    };
    Remote {
        experience: rbx_cloud::ExperienceSettings {
            display_name: place.display_name.clone(),
            description: place.description.clone(),
            visibility: rbx_cloud::Visibility::Public,
            age_rating: Some("13_PLUS".to_string()),
            voice_chat_enabled: true,
            private_server_price_robux: Some(100),
            desktop_enabled: true,
            mobile_enabled: true,
            tablet_enabled: true,
            console_enabled: false,
            vr_enabled: false,
            root_place_id: 1,
        },
        root_place: place.clone(),
        place,
        place_id: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_cloud::{ExperienceUpdate, PlaceUpdate};

    #[test]
    fn mock_save_applies_updates() {
        let remote = mock_remote();
        let updates = Updates {
            experience: ExperienceUpdate {
                voice_chat_enabled: Some(false),
                private_server_price_robux: Some(None),
                vr_enabled: Some(true),
                ..Default::default()
            },
            root_place: PlaceUpdate {
                display_name: Some("Renamed".to_string()),
                server_size: Some(50),
                ..Default::default()
            },
            place: PlaceUpdate::default(),
        };
        let saved = mock_save("fixture", &remote, &updates).unwrap();
        assert!(!saved.experience.voice_chat_enabled);
        assert_eq!(saved.experience.private_server_price_robux, None);
        assert!(saved.experience.vr_enabled);
        assert_eq!(saved.experience.display_name, "Renamed");
        assert_eq!(saved.place.display_name, "Renamed");
        assert_eq!(saved.place.server_size, 50);
    }

    #[test]
    fn mock_status_fails_with_a_sentence() {
        let err = mock_load("409").unwrap_err();
        assert!(err.contains("Team Create"));
        assert!(mock_save("403", &mock_remote(), &Updates::default())
            .unwrap_err()
            .1
            .contains("universe:write"));
        let read = describe_read(&CloudError::Http {
            status: 403,
            url: String::new(),
        });
        assert!(read.contains("granted access") && !read.contains(":write"));
    }
}
