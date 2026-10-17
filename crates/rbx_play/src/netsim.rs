//! Network simulation for Play: latency, jitter and packet loss on the real
//! client's UDP game traffic, in each direction, the way Studio's Network
//! Simulator (`studio/network-simulator.md`) shapes a playtest.
//!
//! Only UDP is shaped: Roblox replication is RakNet over UDP, and Studio's
//! simulator touches replication, not HTTP, so asset downloads (TCP) keep
//! their real speed here too. The editor's live view is not shaped either:
//! the probe reaches the editor from the game server through the tunnel,
//! outside the client's network.
//!
//! [`NetConditions`] and the presets are plain data on every platform; the
//! [`Shaper`] that applies them is Linux-only (a user + network namespace,
//! `slirp4netns` for internet, `tc netem` for the shaping).

#[cfg(target_os = "linux")]
mod shaper;
#[cfg(target_os = "linux")]
pub use shaper::{probe, Shaper, ShaperError};

/// Studio's ranges (`studio/network-simulator.md`, "Configure custom
/// conditions"): 0–1000 ms in 0.1 ms steps, loss 0.00–0.50 % in 0.01 steps.
pub const MAX_DELAY_MS: f64 = 1000.0;
pub const MAX_LOSS_PERCENT: f64 = 0.5;

/// One direction's conditions. Jitter is variation *on top of* latency, so
/// the smallest delay a packet gets is `latency_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OneWay {
    pub latency_ms: f64,
    pub jitter_ms: f64,
    pub loss_percent: f64,
}

impl OneWay {
    pub const fn new(latency_ms: f64, jitter_ms: f64, loss_percent: f64) -> Self {
        OneWay {
            latency_ms,
            jitter_ms,
            loss_percent,
        }
    }

    /// Into range and onto Studio's display precision; NaN becomes 0.
    pub fn clamped(self) -> Self {
        let step = |value: f64, max: f64, per_unit: f64| {
            let value = if value.is_nan() { 0.0 } else { value };
            (value.clamp(0.0, max) * per_unit).round() / per_unit
        };
        OneWay {
            latency_ms: step(self.latency_ms, MAX_DELAY_MS, 10.0),
            jitter_ms: step(self.jitter_ms, MAX_DELAY_MS, 10.0),
            loss_percent: step(self.loss_percent, MAX_LOSS_PERCENT, 100.0),
        }
    }

    /// The three integers the shaper's holder takes for this direction:
    /// netem's mean delay and its ± spread in microseconds, and the loss in
    /// hundredths of a percent. netem's default spread is uniform, so a
    /// mean of `latency + jitter/2` ± `jitter/2` keeps every packet between
    /// `latency` and `latency + jitter`.
    pub fn netem_numbers(self) -> [u32; 3] {
        let c = self.clamped();
        let us = |ms: f64| (ms * 1000.0).round() as u32;
        [
            us(c.latency_ms + c.jitter_ms / 2.0),
            us(c.jitter_ms / 2.0),
            (c.loss_percent * 100.0).round() as u32,
        ]
    }

    /// The `netem` arguments [`OneWay::netem_numbers`] stand for, as the
    /// holder spells them.
    pub fn netem_args(self) -> String {
        let [delay, spread, loss] = self.netem_numbers();
        format!(
            "netem delay {delay}us {spread}us loss {}.{:02}%",
            loss / 100,
            loss % 100
        )
    }
}

/// Server→client (`inbound`) and client→server (`outbound`) conditions.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NetConditions {
    pub inbound: OneWay,
    pub outbound: OneWay,
}

impl NetConditions {
    pub fn clamped(self) -> Self {
        NetConditions {
            inbound: self.inbound.clamped(),
            outbound: self.outbound.clamped(),
        }
    }

    /// The `tc` commands the shaper runs inside its namespace for these
    /// conditions: outbound on `tap0`'s UDP band, inbound on `ifb0`, where
    /// `tap0`'s incoming UDP is redirected.
    pub fn tc_commands(self) -> [String; 2] {
        [
            format!(
                "tc qdisc change dev tap0 parent 1:3 handle 30: {}",
                self.outbound.netem_args()
            ),
            format!(
                "tc qdisc change dev ifb0 root {}",
                self.inbound.netem_args()
            ),
        ]
    }

    /// The lines sent to the holder's stdin: a direction and three
    /// integers each, nothing it would have to parse as a command. Only the
    /// Linux shaper sends them.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn holder_lines(self) -> String {
        let line = |dir: &str, way: OneWay| {
            let [delay, spread, loss] = way.netem_numbers();
            format!("{dir} {delay} {spread} {loss}\n")
        };
        line("out", self.outbound) + &line("in", self.inbound)
    }
}

/// A built-in preset: its name, the short label the Network pill shows,
/// and its conditions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub pill: &'static str,
    pub conditions: NetConditions,
}

const fn preset(
    name: &'static str,
    pill: &'static str,
    inbound: OneWay,
    outbound: OneWay,
) -> Preset {
    Preset {
        name,
        pill,
        conditions: NetConditions { inbound, outbound },
    }
}

/// Studio's built-ins, in its menu order (`studio/network-simulator.md`,
/// "Apply a preset"). The first is Studio's default.
pub const PRESETS: [Preset; 5] = [
    preset(
        "Ideal Fiber",
        "LAN",
        OneWay::new(8.0, 0.0, 0.0),
        OneWay::new(8.0, 0.0, 0.0),
    ),
    preset(
        "Wired Broadband",
        "LAN",
        OneWay::new(25.0, 3.0, 0.0),
        OneWay::new(25.0, 3.0, 0.0),
    ),
    preset(
        "Home Wi-Fi",
        "Wifi",
        OneWay::new(30.0, 12.0, 0.2),
        OneWay::new(30.0, 15.0, 0.3),
    ),
    preset(
        "Standard Mobile (4G/LTE)",
        "4G",
        OneWay::new(45.0, 20.0, 0.4),
        OneWay::new(55.0, 30.0, 0.5),
    ),
    preset(
        "Bad Connection (3G)",
        "3G",
        OneWay::new(150.0, 70.0, 0.5),
        OneWay::new(180.0, 90.0, 0.5),
    ),
];

/// Names a saved preset may not take: the built-ins and "Custom".
pub fn is_reserved_name(name: &str) -> bool {
    let name = name.trim();
    name.eq_ignore_ascii_case("Custom") || PRESETS.iter().any(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamping_keeps_studio_ranges_and_precision() {
        let way = OneWay::new(1234.56, -3.0, 0.777).clamped();
        assert_eq!(way, OneWay::new(1000.0, 0.0, 0.5));
        let way = OneWay::new(12.345, 0.04, 0.123).clamped();
        assert_eq!(way, OneWay::new(12.3, 0.0, 0.12));
        assert_eq!(OneWay::new(f64::NAN, 1.0, 0.0).clamped().latency_ms, 0.0);
    }

    #[test]
    fn jitter_is_added_on_top_of_latency() {
        // 100 ms + up to 20 ms: netem's uniform 110 ± 10 ms.
        let way = OneWay::new(100.0, 20.0, 0.25);
        assert_eq!(way.netem_numbers(), [110_000, 10_000, 25]);
        assert_eq!(way.netem_args(), "netem delay 110000us 10000us loss 0.25%");
        assert_eq!(
            OneWay::new(0.1, 0.0, 0.0).netem_args(),
            "netem delay 100us 0us loss 0.00%"
        );
    }

    #[test]
    fn tc_commands_shape_outbound_on_tap_and_inbound_on_ifb() {
        let conditions = PRESETS[4].conditions;
        assert_eq!(
            conditions.tc_commands(),
            [
                "tc qdisc change dev tap0 parent 1:3 handle 30: netem delay 225000us 45000us loss 0.50%",
                "tc qdisc change dev ifb0 root netem delay 185000us 35000us loss 0.50%",
            ]
        );
        assert_eq!(
            conditions.holder_lines(),
            "out 225000 45000 50\nin 185000 35000 50\n"
        );
    }

    #[test]
    fn built_in_and_custom_names_are_reserved() {
        assert!(is_reserved_name(" home wi-fi "));
        assert!(is_reserved_name("custom"));
        assert!(!is_reserved_name("Office VPN"));
        assert_eq!(PRESETS[0].name, "Ideal Fiber");
    }
}
