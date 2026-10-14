#![cfg(target_os = "linux")]

use rbx_play::embed::{is_client_class, motif_no_decorations, Embedded};

#[test]
fn sober_class_matches() {
    assert!(is_client_class(b"sober\0Sober\0"));
}

#[test]
fn wine_player_class_matches() {
    assert!(is_client_class(b"Wine\0RobloxPlayerBeta.exe\0"));
}

#[test]
fn other_class_does_not_match() {
    assert!(!is_client_class(b"firefox\0Firefox\0"));
}

#[test]
fn empty_class_does_not_match() {
    assert!(!is_client_class(b""));
}

#[test]
fn motif_hints_remove_decorations() {
    assert_eq!(motif_no_decorations(), [2, 0, 0, 0, 0]);
}

#[test]
#[ignore = "needs a running X display ($DISPLAY)"]
fn existing_clients_lists_windows() {
    assert!(Embedded::existing_clients().is_ok());
}
