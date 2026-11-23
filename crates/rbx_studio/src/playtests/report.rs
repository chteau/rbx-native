//! The "Copy bug report" text: a recording as a short Markdown summary that
//! reads well pasted into Discord or a GitHub issue.

use super::{clock, EventKind, Recording};

/// Rows a section lists before "…and N more": a whole report stays under
/// Discord's 2000-character message limit in the common case.
const SECTION_ROWS: usize = 8;
/// Longest a listed line's text gets before it is cut.
const LINE_CHARS: usize = 160;

fn cut(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match one_line.char_indices().nth(LINE_CHARS) {
        Some((at, _)) => format!("{}…", &one_line[..at]),
        None => one_line,
    }
}

fn section(out: &mut String, title: &str, rows: &[String]) {
    if rows.is_empty() {
        return;
    }
    out.push_str(&format!("\n**{title}**\n"));
    for row in rows.iter().take(SECTION_ROWS) {
        out.push_str(&format!("- {row}\n"));
    }
    if rows.len() > SECTION_ROWS {
        out.push_str(&format!("- …and {} more\n", rows.len() - SECTION_ROWS));
    }
}

/// `note` is the sender's own words, first when given. Times are UTC so
/// they read the same wherever the report is pasted.
pub(crate) fn bug_report(recording: &Recording, note: &str) -> String {
    let name_of = |user: u64| {
        recording
            .players
            .iter()
            .find(|player| player.user == user)
            .map_or_else(|| format!("player {user}"), |player| player.name.clone())
    };
    let started = chrono::DateTime::from_timestamp(recording.started as i64, 0)
        .map(|at| at.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default();
    let summary = recording.summary();
    let players = recording
        .players
        .iter()
        .map(|player| player.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = format!("**Playtest: {}**\n", cut(&recording.name));
    let note = note.trim();
    if !note.is_empty() {
        out.push_str(&format!("> {}\n", cut(note)));
    }
    out.push_str(&format!(
        "{started} · {} · {} player{}{} · {} death{}\n",
        clock(recording.duration),
        summary.players,
        if summary.players == 1 { "" } else { "s" },
        if players.is_empty() {
            String::new()
        } else {
            format!(" ({players})")
        },
        summary.deaths,
        if summary.deaths == 1 { "" } else { "s" },
    ));
    let at = |t: f32| format!("`{}`", clock(t));
    let rows = |pick: &dyn Fn(&super::Event) -> Option<String>| -> Vec<String> {
        recording.events.iter().filter_map(pick).collect()
    };
    section(
        &mut out,
        "Errors and warnings",
        &rows(&|event| match event.kind {
            EventKind::Error => Some(format!("{} Error: {}", at(event.t), cut(&event.label))),
            EventKind::Warning => Some(format!("{} Warning: {}", at(event.t), cut(&event.label))),
            _ => None,
        }),
    );
    section(
        &mut out,
        "Deaths",
        &rows(&|event| {
            (event.kind == EventKind::Death)
                .then(|| format!("{} {}", at(event.t), name_of(event.user)))
        }),
    );
    section(
        &mut out,
        "Markers",
        &rows(&|event| {
            (event.kind == EventKind::Mark).then(|| match event.data.trim() {
                "" | "null" => format!("{} {}", at(event.t), cut(&event.label)),
                data => format!("{} {} `{}`", at(event.t), cut(&event.label), cut(data)),
            })
        }),
    );
    out
}
