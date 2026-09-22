// `Shell::argon_connect`/`apply_argon_changes`/etc. need a live GPUI
// `Context<Shell>` this crate has no headless harness for — the same
// boundary `shell::history`'s own tests stop at. What's tested here is the
// pure logic underneath: parsing the dock's address field.

use super::*;

#[test]
fn a_plain_host_and_port_splits_on_the_colon() {
    assert_eq!(
        parse_address("localhost:8000"),
        ("localhost".to_owned(), 8000)
    );
}

#[test]
fn a_host_with_no_port_falls_back_to_argons_own_default() {
    assert_eq!(parse_address("localhost"), ("localhost".to_owned(), 8000));
}

#[test]
fn surrounding_whitespace_is_trimmed_from_both_halves() {
    assert_eq!(
        parse_address(" localhost : 8080 "),
        ("localhost".to_owned(), 8080)
    );
}

#[test]
fn a_malformed_port_falls_back_to_argons_own_default_rather_than_refusing_to_connect() {
    assert_eq!(
        parse_address("localhost:not-a-port"),
        ("localhost".to_owned(), 8000)
    );
}
