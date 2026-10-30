use crate::command_bar::Feedback;

use super::super::output::OutputLog;
use super::super::play::Phase;
use super::capture::{jpeg, scaled_size, MAX_SIDE};
use super::jobs::{console_text, play_action, PlayAction, CONSOLE_CAP, TRUNCATED};

#[test]
fn console_joins_every_row() {
    let mut log = OutputLog::default();
    log.push("a", Feedback::Output("one".to_owned()));
    log.push("b", Feedback::Idle);
    log.push("silent run", Feedback::Output(String::new()));
    log.push("c", Feedback::Error("two".to_owned()));
    log.push("d", Feedback::Warning("three".to_owned()));
    assert_eq!(console_text(&log), "one\ntwo\nthree");
}

/// Studio's rule, measured on it: 400 printed lines of 50 characters all
/// fit (the line breaks don't count), one more line of 50 drops the
/// oldest, and a single line over the cap leaves only the marker.
#[test]
fn console_keeps_whole_rows_behind_the_marker() {
    let mut log = OutputLog::default();
    let run: Vec<String> = (0..400)
        .map(|i| format!("{i:04}{}", "y".repeat(46)))
        .collect();
    log.push("a", Feedback::Output(run.join("\n")));
    let text = console_text(&log);
    assert_eq!(text.lines().count(), 400);
    assert!(!text.starts_with(TRUNCATED));
    log.push("b", Feedback::Output(format!("0400{}", "y".repeat(46))));
    let text = console_text(&log);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 401);
    assert_eq!(lines[0], TRUNCATED);
    assert!(lines[1].starts_with("0001"));
    assert!(lines[400].starts_with("0400"));
    let mut log = OutputLog::default();
    log.push("a", Feedback::Output("z".repeat(CONSOLE_CAP + 1)));
    assert_eq!(console_text(&log), TRUNCATED);
}

#[test]
fn jpeg_sizes() {
    assert_eq!(scaled_size(800, 600), (800, 600));
    assert_eq!(scaled_size(2560, 1440), (MAX_SIDE, 720));
    assert_eq!(scaled_size(1000, 4000), (320, MAX_SIDE));
    let bytes = jpeg(2, 2, vec![255; 16]).expect("encodes");
    assert_eq!(&bytes[..3], [0xff, 0xd8, 0xff]);
    assert!(jpeg(3, 3, vec![0; 4]).is_err());
}

#[test]
fn play_answers() {
    let ok = |text: &str| PlayAction::Answer(Ok(text.to_owned()));
    assert_eq!(play_action(true, &Phase::Running), ok("Game Started"));
    assert_eq!(play_action(true, &Phase::Idle), PlayAction::Start);
    assert_eq!(
        play_action(true, &Phase::Starting("x")),
        ok("Start play hasn't finished yet")
    );
    assert_eq!(play_action(false, &Phase::Idle), ok("Game Stopped"));
    assert_eq!(
        play_action(false, &Phase::Starting("x")),
        ok("Start play hasn't finished yet")
    );
    assert_eq!(play_action(false, &Phase::Running), PlayAction::Stop);
}
