//! What the Remote inspector holds: the captured calls, their rates per
//! remote and per player, the filters and the exports. No GPUI here.

use std::collections::{BTreeMap, VecDeque};

use rbx_sim::remote_log::{Arg, Direction, Event, Fate, Kind, RemoteCall, Reply, LONG_INVOKE};

/// Calls kept, the oldest dropped past it.
const KEPT: usize = 5000;
/// Seconds of history a sparkline shows, one bar a second.
pub(super) const SPARK: usize = 30;
/// `UnreliableRemoteEvent` packets a server takes from one client a second.
pub(super) const UNRELIABLE_PER_SECOND: u32 = 500;

pub(crate) struct Row {
    pub(super) call: RemoteCall,
    pub(super) reply: Option<Reply>,
}

impl Row {
    /// A word on what went wrong, `None` when nothing did; `true` when it
    /// is an error rather than a warning.
    pub(super) fn flag(&self, now: f64) -> Option<(String, bool)> {
        match self.call.fate {
            Fate::OverLimit => return Some(("over 1000 B: dropped".into(), true)),
            Fate::RateLimited => return Some(("over 500/s: dropped".into(), true)),
            Fate::Lost => return Some(("lost".into(), true)),
            Fate::Resent(n) => return Some((format!("resent ×{n}"), false)),
            Fate::Delivered => {}
        }
        if self.call.kind != Kind::Invoke {
            return None;
        }
        match &self.reply {
            Some(Reply {
                error: Some(error), ..
            }) => Some((format!("error: {error}"), true)),
            Some(reply) if reply.seconds > LONG_INVOKE => {
                Some((format!("yielded {:.2} s", reply.seconds), false))
            }
            Some(_) => None,
            None if now - self.call.time > LONG_INVOKE => {
                Some((format!("waiting {:.1} s", now - self.call.time), false))
            }
            None => None,
        }
    }

    pub(super) fn summary(&self) -> String {
        let text = self
            .call
            .args
            .iter()
            .map(arg_summary)
            .collect::<Vec<_>>()
            .join(", ");
        let mut text = if text.chars().count() > 120 {
            text.chars().take(119).collect::<String>() + "…"
        } else {
            text
        };
        if let Some(reply) = &self.reply {
            text += &format!("  →  {:.0} ms", reply.seconds * 1000.0);
        }
        text
    }

    pub(super) fn caller(&self) -> String {
        self.call
            .caller
            .as_ref()
            .map(|c| {
                let name = c.script.rsplit('.').next().unwrap_or(&c.script);
                format!("{name}:{}", c.line)
            })
            .unwrap_or_default()
    }
}

fn arg_summary(arg: &Arg) -> String {
    match arg.kind.as_str() {
        "table" => format!("{{{}}}", arg.children.len()),
        "string" | "number" | "boolean" | "nil" | "Instance" | "EnumItem" => arg.text.clone(),
        kind => format!("{kind}({})", arg.text),
    }
}

/// Calls and bytes a second, a bucket per server second.
#[derive(Default)]
pub(crate) struct Rate {
    seconds: VecDeque<(u64, u32, usize)>,
    /// Unreliable packets to the server, per second (the server's limit).
    unreliable: VecDeque<(u64, u32)>,
}

impl Rate {
    fn add(&mut self, time: f64, bytes: usize, unreliable_up: bool) {
        let second = time.max(0.0) as u64;
        match self.seconds.back_mut() {
            Some((at, calls, total)) if *at == second => {
                *calls += 1;
                *total += bytes;
            }
            _ => self.seconds.push_back((second, 1, bytes)),
        }
        if unreliable_up {
            match self.unreliable.back_mut() {
                Some((at, calls)) if *at == second => *calls += 1,
                _ => self.unreliable.push_back((second, 1)),
            }
        }
        let oldest = second.saturating_sub(SPARK as u64);
        while self.seconds.front().is_some_and(|(at, ..)| *at < oldest) {
            self.seconds.pop_front();
        }
        while self.unreliable.front().is_some_and(|(at, _)| *at < oldest) {
            self.unreliable.pop_front();
        }
    }

    fn add_bytes(&mut self, time: f64, bytes: usize) {
        let second = time.max(0.0) as u64;
        match self.seconds.back_mut() {
            Some((at, _, total)) if *at == second => *total += bytes,
            _ => self.seconds.push_back((second, 0, bytes)),
        }
    }

    /// Calls and bytes in the last whole second before `now`.
    pub(super) fn per_second(&self, now: f64) -> (u32, usize) {
        let last = (now.max(0.0) as u64).saturating_sub(1);
        self.seconds
            .iter()
            .find(|(at, ..)| *at == last)
            .map_or((0, 0), |(_, calls, bytes)| (*calls, *bytes))
    }

    /// The most unreliable packets to the server in any second kept.
    pub(super) fn unreliable_peak(&self) -> u32 {
        self.unreliable.iter().map(|(_, n)| *n).max().unwrap_or(0)
    }

    /// Calls in each of the last [`SPARK`] whole seconds, oldest first.
    pub(super) fn spark(&self, now: f64) -> [u32; SPARK] {
        let mut bars = [0; SPARK];
        let end = now.max(0.0) as u64;
        for (at, calls, _) in &self.seconds {
            if let Some(back) = end.checked_sub(*at + 1) {
                if (back as usize) < SPARK {
                    bars[SPARK - 1 - back as usize] = *calls;
                }
            }
        }
        bars
    }
}

#[derive(Clone, Default, PartialEq)]
pub(crate) struct Filter {
    pub(super) name: String,
    pub(super) player: Option<String>,
    pub(super) direction: Option<Direction>,
}

impl Filter {
    pub(super) fn keeps(&self, call: &RemoteCall) -> bool {
        let name = self.name.trim().to_lowercase();
        (name.is_empty() || call.path.to_lowercase().contains(&name))
            && self.player.as_ref().is_none_or(|p| *p == call.player)
            && self.direction.is_none_or(|d| d == call.direction)
    }
}

/// The session's captured calls, kept by the window running it.
#[derive(Default)]
pub(crate) struct Store {
    rows: VecDeque<Row>,
    pub(super) paused: bool,
    pub(super) by_remote: BTreeMap<String, Rate>,
    pub(super) by_player: BTreeMap<String, Rate>,
    /// Server seconds of the newest frame seen.
    pub(super) now: f64,
}

impl Store {
    /// Takes in what the session logged up to server second `now`; paused,
    /// it is let go.
    pub(crate) fn take(&mut self, events: Vec<Event>, now: f64) {
        self.now = now;
        if self.paused {
            return;
        }
        for event in events {
            match event {
                Event::Call(call) => {
                    let unreliable_up =
                        call.kind == Kind::Unreliable && call.direction == Direction::ToServer;
                    for rate in [
                        self.by_remote.entry(call.path.clone()).or_default(),
                        self.by_player.entry(call.player.clone()).or_default(),
                    ] {
                        rate.add(call.time, call.bytes, unreliable_up);
                    }
                    self.rows.push_back(Row { call, reply: None });
                    if self.rows.len() > KEPT {
                        self.rows.pop_front();
                    }
                }
                Event::Fate(id, fate) => {
                    if let Some(row) = self.row_mut(id) {
                        row.call.fate = fate;
                    }
                }
                Event::Replied(id, reply) => {
                    let Some(row) = self.row_mut(id) else {
                        continue;
                    };
                    let (path, player) = (row.call.path.clone(), row.call.player.clone());
                    let (time, bytes) = (row.call.time + reply.seconds, reply.bytes);
                    row.reply = Some(reply);
                    for rate in [
                        self.by_remote.entry(path).or_default(),
                        self.by_player.entry(player).or_default(),
                    ] {
                        rate.add_bytes(time, bytes);
                    }
                }
            }
        }
    }

    fn row_mut(&mut self, id: u64) -> Option<&mut Row> {
        let at = self.rows.binary_search_by_key(&id, |r| r.call.id).ok()?;
        self.rows.get_mut(at)
    }

    pub(crate) fn clear(&mut self) {
        let now = self.now;
        *self = Store {
            paused: self.paused,
            now,
            ..Store::default()
        };
    }

    pub(super) fn rows(&self) -> impl DoubleEndedIterator<Item = &Row> {
        self.rows.iter()
    }

    pub(super) fn row(&self, id: u64) -> Option<&Row> {
        let at = self.rows.binary_search_by_key(&id, |r| r.call.id).ok()?;
        self.rows.get(at)
    }

    pub(super) fn players(&self) -> Vec<String> {
        self.by_player
            .keys()
            .filter(|p| *p != "all")
            .cloned()
            .collect()
    }
}

fn fate_text(fate: Fate) -> String {
    match fate {
        Fate::Delivered => "delivered".into(),
        Fate::Resent(n) => format!("resent {n}"),
        Fate::Lost => "lost".into(),
        Fate::OverLimit => "over limit".into(),
        Fate::RateLimited => "rate limited".into(),
    }
}

fn kind_text(kind: Kind) -> &'static str {
    match kind {
        Kind::Event => "RemoteEvent",
        Kind::Unreliable => "UnreliableRemoteEvent",
        Kind::Invoke => "RemoteFunction",
    }
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

/// The rows `filter` keeps as CSV, a header line first.
pub(super) fn csv<'a>(rows: impl Iterator<Item = &'a Row>) -> String {
    let mut out = String::from(
        "time,direction,remote,class,player,bytes,fate,script,line,arguments,reply_seconds,error\n",
    );
    for row in rows {
        let call = &row.call;
        let (script, line) = call
            .caller
            .as_ref()
            .map_or((String::new(), String::new()), |c| {
                (c.script.clone(), c.line.to_string())
            });
        let fields = [
            format!("{:.3}", call.time),
            call.direction.label().into(),
            call.path.clone(),
            kind_text(call.kind).into(),
            call.player.clone(),
            call.bytes.to_string(),
            fate_text(call.fate),
            script,
            line,
            call.args
                .iter()
                .map(arg_summary)
                .collect::<Vec<_>>()
                .join(", "),
            row.reply
                .as_ref()
                .map_or(String::new(), |r| format!("{:.3}", r.seconds)),
            row.reply
                .as_ref()
                .and_then(|r| r.error.clone())
                .unwrap_or_default(),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        out += &line.join(",");
        out.push('\n');
    }
    out
}

fn arg_json(arg: &Arg) -> serde_json::Value {
    let mut value = serde_json::json!({ "type": arg.kind, "value": arg.text });
    if let Some(key) = &arg.key {
        value["key"] = key.clone().into();
    }
    if !arg.children.is_empty() {
        value["entries"] = arg.children.iter().map(arg_json).collect();
    }
    value
}

/// The rows as JSON, each with its whole argument tree.
pub(super) fn json<'a>(rows: impl Iterator<Item = &'a Row>) -> String {
    let rows: Vec<serde_json::Value> = rows
        .map(|row| {
            let call = &row.call;
            serde_json::json!({
                "time": call.time,
                "direction": call.direction.label(),
                "remote": call.path,
                "class": kind_text(call.kind),
                "player": call.player,
                "bytes": call.bytes,
                "fate": fate_text(call.fate),
                "caller": call.caller.as_ref().map(|c| serde_json::json!({ "script": c.script, "line": c.line })),
                "arguments": call.args.iter().map(arg_json).collect::<Vec<_>>(),
                "reply": row.reply.as_ref().map(|r| serde_json::json!({
                    "seconds": r.seconds,
                    "bytes": r.bytes,
                    "error": r.error,
                })),
            })
        })
        .collect();
    serde_json::to_string_pretty(&rows).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use rbx_dom::Ref;
    use rbx_lua::server::Caller;

    use super::*;

    fn call(id: u64, time: f64, path: &str, player: &str, kind: Kind, bytes: usize) -> Event {
        Event::Call(RemoteCall {
            id,
            time,
            direction: Direction::ToServer,
            kind,
            remote: Ref::new(1),
            path: path.into(),
            player: player.into(),
            args: vec![Arg {
                key: None,
                kind: "string".into(),
                text: "\"a, b\"".into(),
                children: Vec::new(),
            }],
            bytes,
            caller: Some(Caller {
                script: "ServerScriptService.Main".into(),
                line: 4,
            }),
            fate: Fate::Delivered,
        })
    }

    #[test]
    fn rates_count_the_last_whole_second_per_remote_and_player() {
        let mut store = Store::default();
        let mut events = Vec::new();
        for i in 0..30 {
            let path = if i % 3 == 0 { "RS.A" } else { "RS.B" };
            let player = if i % 2 == 0 { "P1" } else { "P2" };
            events.push(call(
                i,
                2.0 + i as f64 / 30.0,
                path,
                player,
                Kind::Event,
                10,
            ));
        }
        store.take(events, 3.5);
        assert_eq!(store.by_remote["RS.A"].per_second(3.5), (10, 100));
        assert_eq!(store.by_remote["RS.B"].per_second(3.5), (20, 200));
        assert_eq!(store.by_player["P1"].per_second(3.5), (15, 150));
        let spark = store.by_remote["RS.A"].spark(3.5);
        assert_eq!(spark[SPARK - 1], 10);
        assert_eq!(spark[SPARK - 2], 0);
    }

    #[test]
    fn unreliable_peaks_show_the_servers_limit() {
        let mut store = Store::default();
        let events = (0..600)
            .map(|i| {
                call(
                    i,
                    1.0 + i as f64 / 1000.0,
                    "RS.U",
                    "P1",
                    Kind::Unreliable,
                    12,
                )
            })
            .collect();
        store.take(events, 2.0);
        assert_eq!(store.by_player["P1"].unreliable_peak(), 600);
        assert!(store.by_player["P1"].unreliable_peak() > UNRELIABLE_PER_SECOND);
    }

    #[test]
    fn fates_and_replies_land_on_their_row_and_flag_it() {
        let mut store = Store::default();
        store.take(
            vec![
                call(0, 1.0, "RS.U", "P1", Kind::Unreliable, 1009),
                call(1, 1.0, "RS.F", "P1", Kind::Invoke, 20),
                call(2, 1.0, "RS.F", "P1", Kind::Invoke, 20),
                call(3, 1.0, "RS.F", "P1", Kind::Invoke, 20),
                call(4, 1.0, "RS.E", "P1", Kind::Event, 20),
                Event::Fate(0, Fate::OverLimit),
                Event::Fate(4, Fate::Resent(2)),
                Event::Replied(
                    1,
                    Reply {
                        seconds: 1.6,
                        error: None,
                        bytes: 17,
                    },
                ),
                Event::Replied(
                    2,
                    Reply {
                        seconds: 0.1,
                        error: Some("nope".into()),
                        bytes: 12,
                    },
                ),
            ],
            3.0,
        );
        let flags: Vec<_> = store.rows().map(|r| r.flag(3.0)).collect();
        assert_eq!(
            flags,
            [
                Some(("over 1000 B: dropped".into(), true)),
                Some(("yielded 1.60 s".into(), false)),
                Some(("error: nope".into(), true)),
                Some(("waiting 2.0 s".into(), false)),
                Some(("resent ×2".into(), false)),
            ]
        );
        assert_eq!(store.row(1).map(Row::caller), Some("Main:4".into()));
    }

    #[test]
    fn filters_pause_and_clear() {
        let mut store = Store::default();
        store.take(
            vec![
                call(0, 1.0, "ReplicatedStorage.Fire", "P1", Kind::Event, 9),
                call(1, 1.0, "ReplicatedStorage.Shoot", "P2", Kind::Event, 9),
            ],
            1.0,
        );
        let filter = Filter {
            name: "fire".into(),
            ..Filter::default()
        };
        assert_eq!(store.rows().filter(|r| filter.keeps(&r.call)).count(), 1);
        let filter = Filter {
            player: Some("P2".into()),
            direction: Some(Direction::ToClient),
            ..Filter::default()
        };
        assert_eq!(store.rows().filter(|r| filter.keeps(&r.call)).count(), 0);
        assert_eq!(store.players(), ["P1", "P2"]);

        store.paused = true;
        store.take(vec![call(2, 2.0, "RS.X", "P1", Kind::Event, 9)], 2.0);
        assert_eq!(store.rows().count(), 2);
        store.clear();
        assert_eq!(store.rows().count(), 0);
        assert!(store.paused && store.by_remote.is_empty());
    }

    #[test]
    fn exports_carry_every_column() {
        let mut store = Store::default();
        store.take(vec![call(0, 1.25, "RS.A", "P1", Kind::Event, 15)], 2.0);
        let csv = csv(store.rows());
        let mut lines = csv.lines();
        assert!(lines.next().unwrap().starts_with("time,direction,remote"));
        assert_eq!(
            lines.next().unwrap(),
            "1.250,client → server,RS.A,RemoteEvent,P1,15,delivered,ServerScriptService.Main,4,\"\"\"a, b\"\"\",,"
        );
        let json: serde_json::Value = serde_json::from_str(&json(store.rows())).unwrap();
        assert_eq!(json[0]["caller"]["line"], 4);
        assert_eq!(json[0]["arguments"][0]["type"], "string");
    }
}
