//! The parts of `rbxstudio`'s command line that answer before a window, a GPU
//! or a place file are ever needed — `--help`, a usage error, and a `--run`
//! script that isn't there.
//!
//! Unit tests cover the parser itself (`src/cli/tests.rs`); what they cannot
//! reach is `main`'s side of the contract, which is what a CI job or a wrapper
//! script actually depends on: which stream each of these lands on, and which
//! exit code comes back. Everything past that point opens a window, so it has
//! no place in a headless test suite.

use std::process::{Command, Output};

fn rbxstudio(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rbxstudio"))
        .args(arguments)
        .output()
        .expect("rbxstudio is built alongside this test")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn help_prints_the_usage_text_on_stdout_and_succeeds() {
    let output = rbxstudio(&["--help"]);

    assert_eq!(output.status.code(), Some(0));
    let printed = stdout(&output);
    assert!(
        printed.starts_with("usage: rbxstudio"),
        "unexpected --help output: {printed}"
    );
    // A caller piping `--help` into a pager should not have to merge streams
    // to read it.
    assert!(stderr(&output).is_empty());
}

#[test]
fn a_usage_error_prints_on_stderr_and_exits_two() {
    for arguments in [&[][..], &["--bogus", "place.rbxl"][..]] {
        let output = rbxstudio(arguments);

        assert_eq!(output.status.code(), Some(2), "for {arguments:?}");
        assert!(
            stderr(&output).contains("usage: rbxstudio"),
            "for {arguments:?}: {}",
            stderr(&output)
        );
    }
}

// The place file below does not exist either: this has to fail on the script
// first, before anything tries to open a window to draw the place in.
#[test]
fn a_run_script_that_cannot_be_read_stops_before_the_place_is_opened() {
    let output = rbxstudio(&["--run", "no/such/script.luau", "no/such/place.rbxl"]);

    assert_eq!(output.status.code(), Some(1));
    let reported = stderr(&output);
    assert!(
        reported.contains("no/such/script.luau"),
        "the failure should name the script: {reported}"
    );
    assert!(
        !reported.contains("no/such/place.rbxl"),
        "the place should not have been reached yet: {reported}"
    );
}
