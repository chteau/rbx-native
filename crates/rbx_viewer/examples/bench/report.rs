//! The two outputs: a table to read now, and a JSON file to diff later.
//!
//! The JSON is hand-written rather than derived. The shape is fixed and shallow,
//! the values are numbers and short strings, and `rbx_viewer` has no serializer
//! among its dependencies — adding one so a developer tool can emit forty lines
//! of object would be the heavier choice, not the lighter one.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::args::Args;
use crate::measure::{Frames, Measured, Phase};
use crate::stats::Samples;

/// Bumped when a field changes meaning, so a diff script comparing two runs can
/// refuse a pair it would misread rather than print a nonsense delta.
const SCHEMA: &str = "rbx-native-bench/1";

pub(crate) struct Run {
    pub(crate) adapter: String,
    pub(crate) commit: String,
    /// Whether the working tree had uncommitted changes. A number recorded off
    /// a dirty tree cannot be attributed to the commit it names.
    pub(crate) dirty: bool,
    pub(crate) recorded_unix: u64,
    pub(crate) fixtures: Vec<Fixture>,
}

pub(crate) struct Fixture {
    pub(crate) path: PathBuf,
    pub(crate) outcome: Outcome,
}

pub(crate) enum Outcome {
    Measured(Box<Measured>),
    /// A fixture this machine does not have. Reported, never fatal: the
    /// interesting place file deliberately lives outside the repository.
    Skipped(String),
}

pub(crate) fn table(run: &Run, args: &Args) {
    println!();
    println!("adapter:  {}", run.adapter);
    println!(
        "commit:   {}{}",
        run.commit,
        if run.dirty {
            " (dirty working tree)"
        } else {
            ""
        }
    );
    println!(
        "settings: {}x{}, textures {}, quality levels {}",
        args.size.0,
        args.size.1,
        if args.textures { "on" } else { "off" },
        args.levels
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );

    for fixture in &run.fixtures {
        println!();
        match &fixture.outcome {
            Outcome::Skipped(reason) => {
                println!("{} — skipped: {reason}", fixture.path.display());
            }
            Outcome::Measured(measured) => {
                println!(
                    "{} — {} instances",
                    fixture.path.display(),
                    measured.instances
                );
                phases(&measured.phases);
                frames(&measured.frames);
                for note in &measured.notes {
                    println!("  note: {note}");
                }
            }
        }
    }
    println!();
}

fn phases(phases: &[Phase]) {
    println!(
        "  {:<16}{:>5}{:>13}{:>11}{:>15}{:>11}{:>10}",
        "operation", "n", "call med", "call p95", "readable med", "p95", "spread"
    );
    for phase in phases {
        println!(
            "  {:<16}{:>5}{:>13}{:>11}{:>15}{:>11}{:>10}",
            phase.name,
            phase.call.len(),
            millis(phase.call.median()),
            millis(phase.call.p95()),
            millis(phase.frame.median()),
            millis(phase.frame.p95()),
            percent(phase.frame.spread()),
        );
    }
}

fn frames(frames: &[Frames]) {
    if frames.is_empty() {
        return;
    }
    println!();
    println!(
        "  {:<16}{:>5}{:>13}{:>11}{:>15}{:>11}{:>10}",
        "frame @ level", "n", "frame med", "frame p95", "draw med", "readback", "fps med"
    );
    for level in frames {
        let median = level.wall.median();
        println!(
            "  {:<16}{:>5}{:>13}{:>11}{:>15}{:>11}{:>10}",
            format!("Level{:02}", level.level),
            level.wall.len(),
            millis(median),
            millis(level.wall.p95()),
            millis(level.render.median()),
            millis(level.readback.median()),
            if median > 0.0 {
                format!("{:.0}", 1000.0 / median)
            } else {
                "-".to_string()
            },
        );
    }
}

fn millis(value: f64) -> String {
    format!("{value:.2} ms")
}

fn percent(value: f64) -> String {
    format!("{value:.0}%")
}

pub(crate) fn json(run: &Run, args: &Args, output: &Path) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {parent:?}: {err}"))?;
    }
    std::fs::write(output, render_json(run, args))
        .map_err(|err| format!("failed to write {output:?}: {err}"))
}

fn render_json(run: &Run, args: &Args) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    let _ = writeln!(out, "  \"schema\": {},", quote(SCHEMA));
    let _ = writeln!(out, "  \"commit\": {},", quote(&run.commit));
    let _ = writeln!(out, "  \"dirty\": {},", run.dirty);
    let _ = writeln!(out, "  \"recorded_unix\": {},", run.recorded_unix);
    let _ = writeln!(out, "  \"adapter\": {},", quote(&run.adapter));
    let _ = writeln!(out, "  \"os\": {},", quote(std::env::consts::OS));
    // Always "release": main refuses to run anything else.
    let _ = writeln!(out, "  \"profile\": \"release\",");
    let _ = writeln!(out, "  \"textures\": {},", args.textures);
    let _ = writeln!(out, "  \"width\": {},", args.size.0);
    let _ = writeln!(out, "  \"height\": {},", args.size.1);
    let _ = writeln!(
        out,
        "  \"iterations\": {{ \"load\": {}, \"reload\": {}, \"patch\": {}, \"frame\": {}, \"frame_warmup\": {} }},",
        args.load_iters, args.reload_iters, args.patch_iters, args.frame_iters, args.frame_warmup
    );
    out.push_str("  \"fixtures\": [\n");
    let fixtures: Vec<String> = run.fixtures.iter().map(fixture_json).collect();
    out.push_str(&fixtures.join(",\n"));
    out.push_str("\n  ]\n}\n");
    out
}

fn fixture_json(fixture: &Fixture) -> String {
    let path = quote(&fixture.path.display().to_string());
    match &fixture.outcome {
        Outcome::Skipped(reason) => format!(
            "    {{ \"path\": {path}, \"status\": \"skipped\", \"reason\": {} }}",
            quote(reason)
        ),
        Outcome::Measured(measured) => {
            let mut out = String::new();
            let _ = writeln!(out, "    {{ \"path\": {path}, \"status\": \"measured\",");
            let _ = writeln!(out, "      \"instances\": {},", measured.instances);
            let notes: Vec<String> = measured.notes.iter().map(|note| quote(note)).collect();
            let _ = writeln!(out, "      \"notes\": [{}],", notes.join(", "));
            let phases: Vec<String> = measured.phases.iter().map(phase_json).collect();
            let _ = writeln!(out, "      \"phases\": [\n{}\n      ],", phases.join(",\n"));
            let frames: Vec<String> = measured.frames.iter().map(frames_json).collect();
            let _ = write!(
                out,
                "      \"frames\": [\n{}\n      ] }}",
                frames.join(",\n")
            );
            out
        }
    }
}

fn phase_json(phase: &Phase) -> String {
    format!(
        "        {{ \"name\": {}, \"n\": {},\n          \"call_ms\": {},\n          \"readable_ms\": {} }}",
        quote(phase.name),
        phase.call.len(),
        samples_json(&phase.call),
        samples_json(&phase.frame)
    )
}

fn frames_json(frames: &Frames) -> String {
    format!(
        "        {{ \"quality\": {}, \"n\": {},\n          \"frame_ms\": {},\n          \"draw_ms\": {},\n          \"readback_ms\": {} }}",
        frames.level,
        frames.wall.len(),
        samples_json(&frames.wall),
        samples_json(&frames.render),
        samples_json(&frames.readback)
    )
}

/// Every sample alongside its summary: two numbers are enough to read a run,
/// never enough to re-test one.
fn samples_json(samples: &Samples) -> String {
    let taken: Vec<String> = samples
        .taken()
        .iter()
        .map(|value| format!("{value:.3}"))
        .collect();
    format!(
        "{{ \"median\": {:.3}, \"p95\": {:.3}, \"min\": {:.3}, \"max\": {:.3}, \"spread_percent\": {:.1}, \"samples\": [{}] }}",
        samples.median(),
        samples.p95(),
        samples.min(),
        samples.max(),
        samples.spread(),
        taken.join(", ")
    )
}

/// A JSON string literal. Paths are the only untrusted input here and a Windows
/// one is full of backslashes, so escaping is not optional even for a file
/// nothing but a diff script reads.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < ' ' => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn paths_and_control_characters_survive_quoting() {
        assert_eq!(quote(r"C:\places\a.rbxl"), r#""C:\\places\\a.rbxl""#);
        assert_eq!(quote("say \"hi\""), r#""say \"hi\"""#);
        assert_eq!(quote("a\nb"), r#""a\nb""#);
        assert_eq!(quote("\u{1}"), r#""\u0001""#);
    }
}
