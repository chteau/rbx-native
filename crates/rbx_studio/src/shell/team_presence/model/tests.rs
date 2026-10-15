//! Statuses, ordering, edit merging and the mock room.

use super::*;

const NOW: u64 = 1_000_000_000;

fn dom() -> WeakDom {
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    dom.new_instance("Part", "Baseplate", Some(ws));
    dom.new_instance("SpawnLocation", "SpawnLocation", Some(ws));
    dom
}

fn rows(own_id: u64, mock: &Mock, dom: &WeakDom) -> Vec<Collaborator> {
    collaborators(
        Some(&mock.snapshot),
        Some(&mock.members),
        &mock.departed,
        own_id,
        dom,
        NOW,
    )
}

#[test]
fn statuses_and_order_from_fixture() {
    let mock = load_mock("fixture", NOW).unwrap();
    let list = rows(99, &mock, &dom());
    let got: Vec<(&str, Status)> = list.iter().map(|c| (c.name.as_str(), c.status)).collect();
    assert_eq!(
        got,
        vec![
            ("Ana", Status::Sending),
            ("Cleo", Status::Sending),
            ("Dee", Status::InSessionWithoutPlugin),
            ("Eli", Status::NotInStudio),
        ]
    );
    assert_eq!(list[0].selection.len(), 1);
    assert_eq!(list[0].unresolved, 0);
    assert_eq!(list[0].client, Some(Client::Studio));
    assert_eq!(list[0].edits[0].at, NOW - 4_000);
    assert!(list[0].script.as_ref().unwrap().1.is_none());
    assert_eq!(list[2].client, None);
}

#[test]
fn everyone_gets_a_distinct_assigned_color() {
    let mock = load_mock("fixture", NOW).unwrap();
    let list = rows(0, &mock, &dom());
    let got: Vec<(&str, (u8, u8, u8))> = list.iter().map(|c| (c.name.as_str(), c.color)).collect();
    let hex = |s: &str| parse_color(s).unwrap();
    assert_eq!(
        got,
        vec![
            ("Ana", hex("#7048e8")),
            ("Cleo", hex("#2f9e44")),
            ("Dee", hex("#0c8599")),
            ("Eli", hex("#e03131")),
        ]
    );
}

#[test]
fn expired_sender_drops_out_and_member_takes_over() {
    let mut mock = load_mock("fixture", NOW).unwrap();
    mock.snapshot.users[1].seen = NOW - schema::EXPIRY_MS - 1;
    let list = rows(99, &mock, &dom());
    let cleo = list.iter().find(|c| c.user_id == 3).unwrap();
    assert_eq!(cleo.status, Status::InSessionWithoutPlugin);
}

#[test]
fn own_id_is_skipped() {
    let mock = load_mock("fixture", NOW).unwrap();
    let list = rows(3, &mock, &dom());
    assert!(list.iter().all(|c| c.user_id != 3));
    assert_eq!(list.len(), 3);
}

#[test]
fn unresolved_paths_are_counted() {
    let mut mock = load_mock("fixture", NOW).unwrap();
    mock.snapshot.users[0]
        .selection
        .push("Workspace.Gone".into());
    let list = rows(99, &mock, &dom());
    assert_eq!(list[0].selection.len(), 1);
    assert_eq!(list[0].unresolved, 1);
}

#[test]
fn old_departures_are_forgotten() {
    let mut mock = load_mock("fixture", NOW).unwrap();
    mock.departed[0].left_at = NOW - DEPARTED_KEEP_MS - 1;
    let list = rows(99, &mock, &dom());
    assert!(list.iter().all(|c| c.status != Status::NotInStudio));
}

#[test]
fn departures_lists_missing_senders() {
    let mock = load_mock("fixture", NOW).unwrap();
    let mut next = mock.snapshot.clone();
    next.users.remove(1);
    let gone = departures(&mock.snapshot, &next, 1234567, NOW);
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].user_id, 3);
    assert_eq!(gone[0].left_at, NOW);
    let none = departures(&mock.snapshot, &next, 3, NOW);
    assert!(none.is_empty());
}

#[test]
fn edit_log_merges_limits_and_expires() {
    let mut log = EditLog::default();
    log.note("A".into(), "CFrame".into(), "1".into(), 1_000);
    log.note("A".into(), "CFrame".into(), "2".into(), 2_500);
    let recent = log.recent(3_000);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].value, "2");
    assert_eq!(recent[0].at, 2_500);

    log.note("A".into(), "CFrame".into(), "3".into(), 10_000);
    assert_eq!(log.recent(10_000).len(), 2);

    log.note("B".into(), "Size".into(), "x".repeat(200), 10_000);
    let last = log.recent(10_000).pop().unwrap();
    assert_eq!(last.value.chars().count(), 80);
    assert!(last.value.ends_with('…'));

    assert_eq!(log.recent(2_500 + EDIT_KEEP_MS).len(), 2);
    assert_eq!(log.recent(10_000 + EDIT_KEEP_MS).len(), 0);

    let mut many = EditLog::default();
    for i in 0..40 {
        many.note(format!("P{i}"), "Name".into(), "v".into(), 5_000);
    }
    let kept = many.recent(5_000);
    assert_eq!(kept.len(), schema::MAX_EDITS);
    assert_eq!(kept[0].path, "P20");
}

#[test]
fn parse_color_accepts_either_case_only() {
    assert_eq!(parse_color("#e8590c"), Some((0xe8, 0x59, 0x0c)));
    assert_eq!(parse_color("#E8590C"), Some((0xe8, 0x59, 0x0c)));
    assert_eq!(parse_color("e8590c"), None);
    assert_eq!(parse_color("#e8590"), None);
    assert_eq!(parse_color("#gg0000"), None);
    assert_eq!(parse_color("#é85900"), None);
}

#[test]
fn bad_color_falls_back_to_palette() {
    let mut mock = load_mock("fixture", NOW).unwrap();
    mock.snapshot.users[1].color = "nope".into();
    let list = rows(99, &mock, &dom());
    let cleo = list.iter().find(|c| c.user_id == 3).unwrap();
    assert_eq!(cleo.color, parse_color(schema::color_for(3)).unwrap());
}

#[test]
fn load_mock_rejects_missing_file() {
    assert!(load_mock("/nonexistent/room.json", NOW).is_err());
}

#[test]
fn own_state_is_clamped() {
    let selection: Vec<String> = (0..150).map(|i| format!("P{i}")).collect();
    let edits: Vec<Edit> = (0..30)
        .map(|i| Edit {
            path: format!("P{i}"),
            property: "Name".into(),
            value: "v".into(),
            at: i,
        })
        .collect();
    let state = own_state(10, "Me", selection, None, Some("s".repeat(600)), edits);
    assert_eq!(state.client, Client::RbxNative);
    assert_eq!(state.color, schema::color_for(10));
    assert_eq!(state.seen, 0);
    assert_eq!(state.selection.len(), schema::MAX_SELECTION);
    assert_eq!(state.edits.len(), schema::MAX_EDITS);
    assert_eq!(state.script.unwrap().len(), schema::MAX_TEXT);
}
