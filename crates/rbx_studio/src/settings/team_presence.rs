//! Team presence's persisted choices (see `shell::team_presence`).

/// Off until the user turns it on: sharing presence sends the selection,
/// camera and open script to a relay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TeamPresenceSettings {
    pub(crate) enabled: bool,
    /// The relay's base URL; empty until set. Anyone can run their own.
    pub(crate) relay_url: String,
}

impl TeamPresenceSettings {
    pub(super) fn read(value: &serde_json::Value) -> Self {
        let Some(saved) = value.get("team_presence") else {
            return Self::default();
        };
        TeamPresenceSettings {
            enabled: saved
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            relay_url: saved
                .get("relay_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
        }
    }

    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": self.enabled, "relay_url": self.relay_url })
    }
}
