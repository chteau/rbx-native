//! The integer line protocol the macOS and Windows helpers read from the
//! editor: `out L J P`, `in L J P`, `ports ...` and `stop`, one per line.
//!
//! The reader runs elevated, so the grammar is deliberately tiny: digits
//! only, no sign, no leading zero, single spaces. Nothing read is ever
//! interpreted as anything but a number, and [`parse`] rejects every
//! deviation instead of guessing.

use super::{NetConditions, OneWay};

// Only the Windows helper parses lines; the macOS helper is a shell script.
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_US: u32 = 1_000_000;
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_PPM: u32 = 5000;
const MAX_PORTS: usize = 64;

/// One direction's conditions in the protocol's integer units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WayUs {
    pub(crate) latency_us: u32,
    pub(crate) jitter_us: u32,
    pub(crate) loss_ppm: u32,
}

impl WayUs {
    pub(crate) fn from_way(way: OneWay) -> WayUs {
        let c = way.clamped();
        WayUs {
            latency_us: (c.latency_ms * 1000.0).round() as u32,
            jitter_us: (c.jitter_ms * 1000.0).round() as u32,
            loss_ppm: (c.loss_percent * 10_000.0).round() as u32,
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn to_way(self) -> OneWay {
        OneWay {
            latency_ms: f64::from(self.latency_us) / 1000.0,
            jitter_ms: f64::from(self.jitter_us) / 1000.0,
            loss_percent: f64::from(self.loss_ppm) / 10_000.0,
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Line {
    Out(WayUs),
    In(WayUs),
    Ports(Vec<u16>),
    Stop,
}

fn way_line(verb: &str, way: OneWay) -> String {
    let w = WayUs::from_way(way);
    format!("{verb} {} {} {}\n", w.latency_us, w.jitter_us, w.loss_ppm)
}

/// Outbound first, matching the order the helpers expect at start-up.
pub(crate) fn conditions_lines(c: &NetConditions) -> String {
    way_line("out", c.outbound) + &way_line("in", c.inbound)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn ports_line(ports: &[u16]) -> String {
    let mut line = String::from("ports");
    for port in ports.iter().take(MAX_PORTS) {
        line.push(' ');
        line.push_str(&port.to_string());
    }
    line.push('\n');
    line
}

#[cfg_attr(not(windows), allow(dead_code))]
fn number(word: &str) -> Option<u32> {
    let digits_only = !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit());
    if !digits_only || word.len() > 7 || (word.len() > 1 && word.starts_with('0')) {
        return None;
    }
    word.parse().ok()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn way(args: &[&str]) -> Option<WayUs> {
    let [l, j, p] = args else { return None };
    let way = WayUs {
        latency_us: number(l)?,
        jitter_us: number(j)?,
        loss_ppm: number(p)?,
    };
    (way.latency_us <= MAX_US && way.jitter_us <= MAX_US && way.loss_ppm <= MAX_PPM).then_some(way)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse(line: &str) -> Option<Line> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut words = line.split(' ');
    let verb = words.next()?;
    let args: Vec<&str> = words.collect();
    match verb {
        "out" => way(&args).map(Line::Out),
        "in" => way(&args).map(Line::In),
        "stop" => args.is_empty().then_some(Line::Stop),
        "ports" => {
            if args.len() > MAX_PORTS {
                return None;
            }
            args.iter()
                .map(|a| {
                    number(a)
                        .filter(|p| (1..=65535).contains(p))
                        .map(|p| p as u16)
                })
                .collect::<Option<Vec<u16>>>()
                .map(Line::Ports)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netsim::PRESETS;

    #[test]
    fn presets_round_trip() {
        for preset in PRESETS {
            let c = preset.conditions.clamped();
            let text = conditions_lines(&c);
            let mut lines = text.lines().map(|l| parse(l).unwrap());
            let Some(Line::Out(out)) = lines.next() else {
                panic!("first line must be out");
            };
            let Some(Line::In(inn)) = lines.next() else {
                panic!("second line must be in");
            };
            assert_eq!(out.to_way(), c.outbound, "{}", preset.name);
            assert_eq!(inn.to_way(), c.inbound, "{}", preset.name);
        }
    }

    #[test]
    fn conditions_text_for_home_wifi() {
        let c = PRESETS[2].conditions;
        assert_eq!(
            conditions_lines(&c),
            "out 30000 15000 3000\nin 30000 12000 2000\n"
        );
    }

    #[test]
    fn from_way_clamps() {
        let w = WayUs::from_way(OneWay::new(5000.0, -3.0, 9.0));
        assert_eq!(
            w,
            WayUs {
                latency_us: 1_000_000,
                jitter_us: 0,
                loss_ppm: 5000
            }
        );
    }

    #[test]
    fn ports_lines() {
        assert_eq!(ports_line(&[1, 2, 3]), "ports 1 2 3\n");
        assert_eq!(ports_line(&[]), "ports\n");
        let many: Vec<u16> = (1..=100).collect();
        assert_eq!(
            parse(&ports_line(&many)),
            Some(Line::Ports((1..=64).collect()))
        );
    }

    #[test]
    fn accepts_valid_lines() {
        assert_eq!(parse("stop\r\n"), Some(Line::Stop));
        assert_eq!(parse("ports"), Some(Line::Ports(vec![])));
        assert_eq!(
            parse("ports 443 65535"),
            Some(Line::Ports(vec![443, 65535]))
        );
        assert_eq!(
            parse("in 0 0 0"),
            Some(Line::In(WayUs {
                latency_us: 0,
                jitter_us: 0,
                loss_ppm: 0
            }))
        );
    }

    #[test]
    fn rejects_bad_lines() {
        let too_many = format!("ports{}", " 1".repeat(65));
        let bad = [
            "",
            "go",
            "stop now",
            "out 1 2",
            "out 1 2 3 4",
            "out 01 2 3",
            "out +1 2 3",
            "out -1 2 3",
            "out 1  2 3",
            "out 1 2 3 ",
            " out 1 2 3",
            "out 1000001 0 0",
            "out 0 1000001 0",
            "out 0 0 5001",
            "out 1 2 x",
            "out 12345678 0 0",
            "in 1.5 0 0",
            "ports 0",
            "ports 65536",
            "ports 1  2",
            "ports 01",
            "ports -1",
            "ports 1 ",
            "OUT 1 2 3",
            too_many.as_str(),
        ];
        for line in bad {
            assert_eq!(parse(line), None, "{line:?}");
        }
    }
}
