use rbx_dom::Ref;

use super::{Opened, Tabs};

fn script(id: u32) -> Ref {
    Ref::new(id)
}

#[test]
fn a_fresh_editor_has_no_tabs_and_nothing_in_front() {
    let tabs = Tabs::default();
    assert!(tabs.all().is_empty());
    assert_eq!(tabs.active(), None);
}

#[test]
fn opening_a_script_adds_a_tab_and_puts_it_in_front() {
    let mut tabs = Tabs::default();
    assert_eq!(tabs.open(script(1)), Opened::New);

    assert_eq!(tabs.all(), [script(1)]);
    assert_eq!(tabs.active(), Some(script(1)));
    assert!(!tabs.all().is_empty());
}

#[test]
fn several_scripts_open_as_several_tabs_in_the_order_they_were_opened() {
    let mut tabs = Tabs::default();
    for id in [1, 2, 3] {
        assert_eq!(tabs.open(script(id)), Opened::New);
    }

    assert_eq!(tabs.all(), [script(1), script(2), script(3)]);
    assert_eq!(tabs.active(), Some(script(3)));
}

#[test]
fn reopening_a_script_focuses_its_tab_rather_than_duplicating_it() {
    let mut tabs = Tabs::default();
    tabs.open(script(1));
    tabs.open(script(2));

    assert_eq!(
        tabs.open(script(1)),
        Opened::Existing,
        "an already-open script must report Existing so its editor is not re-seeded"
    );
    assert_eq!(tabs.all(), [script(1), script(2)], "no duplicate tab");
    assert_eq!(tabs.active(), Some(script(1)), "and it comes to the front");
}

#[test]
fn closing_the_front_tab_brings_forward_the_one_to_its_right() {
    let mut tabs = Tabs::default();
    for id in [1, 2, 3] {
        tabs.open(script(id));
    }
    tabs.activate(script(1));

    tabs.close(script(1));
    assert_eq!(tabs.all(), [script(2), script(3)]);
    assert_eq!(tabs.active(), Some(script(2)));
}

#[test]
fn closing_the_last_tab_falls_back_to_the_one_on_its_left() {
    let mut tabs = Tabs::default();
    for id in [1, 2, 3] {
        tabs.open(script(id));
    }

    tabs.close(script(3));
    assert_eq!(tabs.all(), [script(1), script(2)]);
    assert_eq!(tabs.active(), Some(script(2)));
}

#[test]
fn closing_a_tab_that_is_not_in_front_leaves_the_front_one_alone() {
    let mut tabs = Tabs::default();
    for id in [1, 2, 3] {
        tabs.open(script(id));
    }

    tabs.close(script(1));
    assert_eq!(tabs.all(), [script(2), script(3)]);
    assert_eq!(tabs.active(), Some(script(3)));
}

#[test]
fn closing_the_only_tab_leaves_the_editor_empty() {
    let mut tabs = Tabs::default();
    tabs.open(script(1));

    tabs.close(script(1));
    assert!(tabs.all().is_empty());
    assert_eq!(tabs.active(), None);
}

#[test]
fn closing_a_script_that_has_no_tab_does_nothing() {
    let mut tabs = Tabs::default();
    tabs.open(script(1));

    tabs.close(script(2));
    assert_eq!(tabs.all(), [script(1)]);
    assert_eq!(tabs.active(), Some(script(1)));
}

#[test]
fn reopening_a_closed_script_opens_a_fresh_tab() {
    // Its editor must be seeded from the DOM again, so this has to report
    // New — a stale editor is exactly what the roadmap item warns against.
    let mut tabs = Tabs::default();
    tabs.open(script(1));
    tabs.close(script(1));

    assert_eq!(tabs.open(script(1)), Opened::New);
    assert_eq!(tabs.active(), Some(script(1)));
}

#[test]
fn activating_an_open_tab_brings_it_to_the_front() {
    let mut tabs = Tabs::default();
    tabs.open(script(1));
    tabs.open(script(2));

    tabs.activate(script(1));
    assert_eq!(tabs.active(), Some(script(1)));
    assert_eq!(tabs.all(), [script(1), script(2)], "the order is unchanged");
}

#[test]
fn activating_a_script_with_no_tab_is_ignored() {
    let mut tabs = Tabs::default();
    tabs.open(script(1));

    tabs.activate(script(2));
    assert_eq!(tabs.active(), Some(script(1)));
    assert!(!tabs.all().contains(&script(2)));
}

#[test]
fn retain_closes_the_tabs_of_scripts_that_are_gone() {
    // What runs after an undo or a delete removes the instance a tab was
    // editing: the tab must go with it rather than edit a dead referent.
    let mut tabs = Tabs::default();
    for id in [1, 2, 3] {
        tabs.open(script(id));
    }

    tabs.retain(|reference| reference != script(3));
    assert_eq!(tabs.all(), [script(1), script(2)]);
    assert_eq!(
        tabs.active(),
        Some(script(2)),
        "the front tab was the one removed, so its neighbour takes over"
    );
}

#[test]
fn retain_can_close_every_tab_at_once() {
    let mut tabs = Tabs::default();
    for id in [1, 2, 3] {
        tabs.open(script(id));
    }

    tabs.retain(|_| false);
    assert!(tabs.all().is_empty());
    assert_eq!(tabs.active(), None);
}

#[test]
fn retain_keeps_everything_when_nothing_was_removed() {
    let mut tabs = Tabs::default();
    for id in [1, 2, 3] {
        tabs.open(script(id));
    }

    tabs.retain(|_| true);
    assert_eq!(tabs.all(), [script(1), script(2), script(3)]);
    assert_eq!(tabs.active(), Some(script(3)));
}
