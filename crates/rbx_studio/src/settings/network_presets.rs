//! The Network Simulator's saved presets (see `shell::play::network`).
//! They outlive places and sessions, as Studio's do; the applied values do
//! not, so they are not here.

use rbx_play::netsim::{NetConditions, OneWay};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedNetworkPreset {
    pub(crate) name: String,
    pub(crate) conditions: NetConditions,
}

fn way_json(way: OneWay) -> Value {
    json!({
        "latency_ms": way.latency_ms,
        "jitter_ms": way.jitter_ms,
        "loss_percent": way.loss_percent,
    })
}

fn read_way(value: &Value) -> OneWay {
    let number = |key: &str| value.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    OneWay::new(
        number("latency_ms"),
        number("jitter_ms"),
        number("loss_percent"),
    )
    .clamped()
}

/// The saved list; an entry without a name, or with a reserved one, is
/// dropped rather than shown under a name it may not have.
pub(super) fn read(value: &Value) -> Vec<SavedNetworkPreset> {
    let Some(list) = value.get("network_presets").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?.trim();
            if name.is_empty() || rbx_play::netsim::is_reserved_name(name) {
                return None;
            }
            Some(SavedNetworkPreset {
                name: name.to_owned(),
                conditions: NetConditions {
                    inbound: read_way(entry.get("inbound")?),
                    outbound: read_way(entry.get("outbound")?),
                },
            })
        })
        .collect()
}

pub(super) fn json(presets: &[SavedNetworkPreset]) -> Value {
    presets
        .iter()
        .map(|preset| {
            json!({
                "name": preset.name,
                "inbound": way_json(preset.conditions.inbound),
                "outbound": way_json(preset.conditions.outbound),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_presets_round_trip_and_drop_reserved_names() {
        let saved = vec![SavedNetworkPreset {
            name: "Office VPN".to_owned(),
            conditions: NetConditions {
                inbound: OneWay::new(40.5, 2.0, 0.05),
                outbound: OneWay::new(60.0, 0.0, 0.5),
            },
        }];
        let mut value = json!({ "network_presets": json(&saved) });
        assert_eq!(read(&value), saved);
        value["network_presets"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "name": "Ideal Fiber", "inbound": {}, "outbound": {} }));
        assert_eq!(read(&value), saved);
        assert!(read(&json!({})).is_empty());
    }
}
