//! A headless Local Run smoke test: a place's server and one player, driven
//! by a fixed bot for a few seconds, with everything both sides printed and
//! the player's leaderstats at the end. Hermetic: DataStores live in memory
//! only, no web lookups, `game.PlaceId` 0, so any place (a private game
//! included) runs without touching anything real.

use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::{Input, LineKind, Peer, Session, FRAME_HZ};

/// The bot's viewport; [`CLICK`] is its middle.
pub const VIEWPORT: [f32; 2] = [800.0, 600.0];
pub const CLICK: [f32; 2] = [400.0, 300.0];

/// What the bot does when, in seconds: walk forward 2 s, jump, press `One`
/// (equips the first tool), then click the middle of the screen three times
/// (a held tool activates, a button there is clicked).
const BOT: &[(f64, Action)] = &[
    (2.0, Action::Key("W", true)),
    (4.0, Action::Key("W", false)),
    (4.2, Action::Key("Space", true)),
    (4.4, Action::Key("Space", false)),
    (5.0, Action::Key("One", true)),
    (5.1, Action::Key("One", false)),
    (5.5, Action::Click(true)),
    (5.6, Action::Click(false)),
    (6.0, Action::Click(true)),
    (6.1, Action::Click(false)),
    (6.5, Action::Click(true)),
    (6.6, Action::Click(false)),
];

#[derive(Clone, Copy)]
enum Action {
    Key(&'static str, bool),
    Click(bool),
}

pub struct Line {
    pub time: f64,
    pub peer: Peer,
    pub kind: LineKind,
    pub text: String,
}

pub struct Report {
    pub lines: Vec<Line>,
    /// The player's `leaderstats` on the server: name → value as text.
    pub leaderstats: BTreeMap<String, String>,
    pub kicked: Vec<String>,
}

impl Report {
    pub fn errors(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|l| l.kind == LineKind::Error)
    }

    pub fn printed(&self, text: &str) -> bool {
        self.lines.iter().any(|l| l.text.contains(text))
    }

    /// Every way the run fell short of `expect` (printed lines) and `stats`
    /// (leaderstats), script errors and kicks first.
    pub fn failures(&self, expect: &[&str], stats: &[(&str, &str)]) -> Vec<String> {
        let mut out: Vec<String> = self
            .errors()
            .map(|l| format!("script error ({}): {}", side(l.peer), l.text))
            .chain(self.kicked.iter().map(|k| format!("kicked: {k}")))
            .collect();
        out.extend(
            expect
                .iter()
                .filter(|e| !self.printed(e))
                .map(|e| format!("never printed {e:?}")),
        );
        for (name, want) in stats {
            match self.leaderstats.get(*name) {
                Some(got) if got == want => {}
                got => out.push(format!("leaderstats.{name} is {got:?}, expected {want:?}")),
            }
        }
        out
    }

    pub fn transcript(&self) -> String {
        let mut text = String::new();
        for l in &self.lines {
            let tag = match l.kind {
                LineKind::Print => "",
                LineKind::Warn => "warn: ",
                LineKind::Error => "error: ",
            };
            text.push_str(&format!(
                "[{:6.2} {}] {tag}{}\n",
                l.time,
                side(l.peer),
                l.text
            ));
        }
        for (name, value) in &self.leaderstats {
            text.push_str(&format!("leaderstats.{name} = {value}\n"));
        }
        text
    }
}

fn side(peer: Peer) -> &'static str {
    match peer {
        Peer::Server => "S",
        Peer::Client(_) => "C",
    }
}

/// Runs `dom` for `seconds` with the bot playing.
pub fn run(dom: WeakDom, seconds: f64) -> Result<Report, String> {
    let mut session = Session::new(dom)?;
    session.set_services(rbx_lua::server::Services::default());
    session.start();
    session.join("Player1", 1, None)?;
    if let Some(sim) = session.sim_mut(Peer::Client(0)) {
        sim.set_viewport(VIEWPORT);
    }
    let mut report = Report {
        lines: Vec::new(),
        leaderstats: BTreeMap::new(),
        kicked: Vec::new(),
    };
    let mut bot = BOT.iter().peekable();
    let frames = (seconds * FRAME_HZ).round() as u64;
    for frame in 0..frames {
        let time = frame as f64 / FRAME_HZ;
        while let Some((_, action)) = bot.next_if(|(at, _)| *at <= time) {
            act(&mut session, *action);
        }
        session.step();
        report
            .kicked
            .extend(session.kicked.drain(..).map(|(n, m)| format!("{n}: {m}")));
        for (peer, output) in session.take_output() {
            for (text, kind) in output.lines().iter().zip(output.kinds()) {
                report.lines.push(Line {
                    time,
                    peer,
                    kind: *kind,
                    text: text.clone(),
                });
            }
        }
        if session.clients.is_empty() {
            break;
        }
    }
    report.leaderstats = leaderstats(&session.server.dom());
    Ok(report)
}

fn act(session: &mut Session, action: Action) {
    if session.clients.is_empty() {
        return;
    }
    match action {
        Action::Key(code, down) => session.input(
            0,
            Input::Key {
                code: code.into(),
                down,
            },
        ),
        Action::Click(down) => {
            session.input(
                0,
                Input::Move {
                    position: CLICK,
                    delta: [0.0, 0.0],
                },
            );
            session.input(0, Input::Button { button: 1, down });
        }
    }
}

fn child(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
}

fn leaderstats(dom: &WeakDom) -> BTreeMap<String, String> {
    let players = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Players"));
    let Some(stats) = players
        .and_then(|p| child(dom, p, "Player1"))
        .and_then(|p| child(dom, p, "leaderstats"))
    else {
        return BTreeMap::new();
    };
    dom.get(stats)
        .map(|s| s.children())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| dom.get(*c))
        .map(|stat| {
            let value = match stat.properties().get("Value") {
                Some(Variant::Int64(v)) => v.to_string(),
                Some(Variant::Int32(v)) => v.to_string(),
                Some(Variant::Float64(v)) => v.to_string(),
                Some(Variant::Float32(v)) => v.to_string(),
                Some(Variant::String(v)) => v.clone(),
                Some(Variant::Bool(v)) => v.to_string(),
                other => format!("{other:?}"),
            };
            (stat.name().to_owned(), value)
        })
        .collect()
}
