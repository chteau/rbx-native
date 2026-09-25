use std::path::PathBuf;

use rbx_viewer::QualityLevel;

use super::{parse, Arguments, Parsed};

fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// The parsed arguments, or a panic naming which of the two other outcomes
/// came back instead — every test below but the `--help` and error ones wants
/// a place to open.
fn open(values: &[&str]) -> Arguments {
    open_with_default(values, QualityLevel::Automatic)
}

fn open_with_default(values: &[&str], default_quality: QualityLevel) -> Arguments {
    match parse(&arguments(values), default_quality) {
        Ok(Parsed::Open(parsed)) => parsed,
        Ok(Parsed::Usage) => panic!("expected a place to open, got the usage text"),
        Err(message) => panic!("expected a place to open, got {message:?}"),
    }
}

fn parse_with_default(values: &[&str]) -> Result<Parsed, String> {
    parse(&arguments(values), QualityLevel::Automatic)
}

#[test]
fn a_single_file_is_the_place_to_open() {
    assert_eq!(
        open(&["place.rbxl"]).path,
        Some(PathBuf::from("place.rbxl"))
    );
}

#[test]
fn no_argument_opens_no_place() {
    assert_eq!(open(&[]).path, None);
}

#[test]
fn a_second_file_is_a_usage_error() {
    assert!(parse_with_default(&["one.rbxl", "two.rbxl"]).is_err());
}

#[test]
fn flags_are_rejected_rather_than_opened_as_files() {
    assert!(parse_with_default(&["--orbit"]).is_err());
    assert!(parse_with_default(&["--size", "800x600"]).is_err());
}

// The editor has a frame clock, so unlike `rbxview` it manages the level
// itself unless told otherwise.
#[test]
fn the_quality_is_automatic_until_a_level_is_given() {
    assert_eq!(open(&["place.rbxl"]).quality, QualityLevel::Automatic);
}

// The persisted setting is what a bare invocation should reopen at, not
// always `Automatic`.
#[test]
fn a_missing_flag_falls_back_to_the_given_default_rather_than_always_automatic() {
    let parsed = open_with_default(&["place.rbxl"], QualityLevel::Level(12));
    assert_eq!(parsed.quality, QualityLevel::Level(12));
}

#[test]
fn an_explicit_flag_overrides_the_given_default() {
    let parsed = open_with_default(
        &["--quality", "Level03", "place.rbxl"],
        QualityLevel::Level(12),
    );
    assert_eq!(parsed.quality, QualityLevel::Level(3));
}

#[test]
fn a_quality_level_is_read_the_way_rbxview_reads_it() {
    assert_eq!(
        open(&["--quality", "Level07", "place.rbxl"]).quality,
        QualityLevel::Level(7)
    );
    assert_eq!(
        open(&["place.rbxl", "--quality", "auto"]).quality,
        QualityLevel::Automatic
    );
    assert!(parse_with_default(&["--quality", "22", "place.rbxl"]).is_err());
    assert!(parse_with_default(&["place.rbxl", "--quality"]).is_err());
}

#[test]
fn nothing_is_selected_or_run_unless_asked_for() {
    let parsed = open(&["place.rbxl"]);
    assert_eq!(parsed.select, None);
    assert_eq!(parsed.run, None);
    assert!(!parsed.verbose);
}

// Kept raw on purpose: a target can only be looked up once the place has been
// read, and splitting the list is `Shell::apply_debug_select`'s job, not this
// parser's.
#[test]
fn select_keeps_its_comma_list_whole() {
    let parsed = open(&[
        "--select",
        "Workspace.Baseplate,SpawnLocation",
        "place.rbxl",
    ]);
    assert_eq!(
        parsed.select.as_deref(),
        Some("Workspace.Baseplate,SpawnLocation")
    );
}

#[test]
fn run_takes_a_script_path() {
    let parsed = open(&["--run", "scripted/build.luau", "place.rbxl"]);
    assert_eq!(parsed.run, Some(PathBuf::from("scripted/build.luau")));
}

#[test]
fn verbose_is_a_flag_with_no_value_of_its_own() {
    let parsed = open(&["--verbose", "place.rbxl"]);
    assert!(parsed.verbose);
    assert_eq!(parsed.path, Some(PathBuf::from("place.rbxl")));
}

#[test]
fn a_flag_left_dangling_at_the_end_of_the_line_is_a_usage_error() {
    assert!(parse_with_default(&["place.rbxl", "--select"]).is_err());
    assert!(parse_with_default(&["place.rbxl", "--run"]).is_err());
}

// `--help` is not an error: it prints the usage text and exits 0, so it has
// to be distinguishable from both a parse failure and a place to open.
#[test]
fn help_asks_for_the_usage_text_rather_than_a_place() {
    assert!(matches!(parse_with_default(&["--help"]), Ok(Parsed::Usage)));
    assert!(matches!(parse_with_default(&["-h"]), Ok(Parsed::Usage)));
}

// Wanting the usage text is never invalidated by whatever else is on the
// line, including a place file that does not exist.
#[test]
fn help_wins_over_the_rest_of_the_line() {
    assert!(matches!(
        parse_with_default(&["place.rbxl", "--verbose", "--help"]),
        Ok(Parsed::Usage)
    ));
}

// A flag that parses but never appears in `--help` is invisible to anyone who
// did not read this file.
#[test]
fn every_flag_the_parser_accepts_appears_in_the_usage_text() {
    for flag in ["--select", "--run", "--quality", "--verbose", "--help"] {
        assert!(
            super::USAGE.contains(flag),
            "{flag} is accepted but missing from the usage text"
        );
    }
}

#[test]
fn setup_asks_for_the_wizard_with_or_without_a_place() {
    assert!(!open(&[]).setup);
    assert!(open(&["--setup"]).setup);
    assert_eq!(open(&["--setup"]).path, None);
}
