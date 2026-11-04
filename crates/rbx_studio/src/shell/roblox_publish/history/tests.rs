use std::cell::RefCell;

use rbx_cloud::{CloudError, PlaceVersion, PublishMode, VersionPage};

use super::fetch::{copy_path, describe_read, restore_with};
use super::labels;
use super::model::{date_label, unnamed, History, Since};
use super::{restored_line, Target};

const TARGET: Target = Target {
    universe_id: 6053515322,
    place_id: 17675488706,
};

fn version(n: u64, created_by: Option<u64>, contributors: &[u64]) -> PlaceVersion {
    PlaceVersion {
        version: n,
        title: None,
        description: None,
        contributors: contributors.to_vec(),
        created_by,
        is_published: false,
        created_time: "2026-01-14T04:55:21.168Z".into(),
    }
}

fn page(numbers: &[u64], next: Option<&str>) -> VersionPage {
    VersionPage {
        versions: numbers.iter().map(|n| version(*n, None, &[])).collect(),
        next_cursor: next.map(str::to_string),
    }
}

fn numbers(history: &History) -> Vec<u64> {
    history.versions.iter().map(|v| v.version).collect()
}

#[test]
fn pages_append_in_order_until_the_cursor_runs_out() {
    let mut history = History::default();
    let generation = history.reset(None);
    assert!(history.accept(generation, page(&[410, 409], Some("c1"))));
    assert_eq!(history.next_cursor.as_deref(), Some("c1"));
    // A save between the two requests pushes 409 onto the next page too.
    assert!(history.accept(generation, page(&[409, 408], None)));
    assert_eq!(numbers(&history), [410, 409, 408]);
    assert_eq!(history.next_cursor, None);
}

#[test]
fn a_page_for_an_earlier_list_is_dropped() {
    let mut history = History::default();
    let old = history.reset(None);
    let new = history.reset(Some(7));
    assert!(!history.accept(old, page(&[410], Some("stale"))));
    assert!(history.versions.is_empty());
    assert_eq!(history.next_cursor, None);
    assert!(history.accept(new, page(&[300], None)));
    assert_eq!(numbers(&history), [300]);
    assert_eq!(history.filter, Some(7));
}

#[test]
fn a_reset_clears_the_list_but_keeps_names() {
    let mut history = History::default();
    let generation = history.reset(None);
    history.accept(generation, page(&[2, 1], Some("c")));
    history.learn_names([(925308243, "Cheeteau".to_string())]);
    history.reset(None);
    assert!(history.versions.is_empty());
    assert_eq!(history.next_cursor, None);
    assert_eq!(history.name(925308243), "Cheeteau");
}

#[test]
fn only_the_top_of_the_unfiltered_list_is_the_latest() {
    let mut history = History::default();
    let generation = history.reset(None);
    history.accept(generation, page(&[410, 409], None));
    assert_eq!(history.latest(), Some(410));
    let generation = history.reset(Some(1));
    history.accept(generation, page(&[405], None));
    assert_eq!(history.latest(), None);
}

#[test]
fn the_author_is_the_saver_else_the_first_in_the_session() {
    let mut history = History::default();
    history.learn_names([(1, "Ada".to_string())]);
    assert_eq!(
        history.author(&version(1, Some(1), &[2])).as_deref(),
        Some("Ada")
    );
    assert_eq!(
        history.author(&version(1, None, &[2, 1])).as_deref(),
        Some("User 2")
    );
    assert_eq!(history.author(&version(1, None, &[])), None);
}

#[test]
fn only_unknown_ids_are_looked_up_once_each() {
    let page = VersionPage {
        versions: vec![version(2, Some(5), &[5, 6]), version(1, Some(7), &[6])],
        next_cursor: None,
    };
    assert_eq!(unnamed(&page, &[8, 5], &[6]), [5, 7, 8]);
}

#[test]
fn dates_read_as_utc_minutes() {
    assert_eq!(
        date_label("2026-01-14T04:55:21.168Z"),
        "2026-01-14 04:55 UTC"
    );
    assert_eq!(date_label("2026-01-14T04:55:21Z"), "2026-01-14 04:55 UTC");
    assert_eq!(date_label(""), "");
    assert_eq!(date_label("yesterday"), "yesterday");
}

#[test]
fn a_local_copy_never_overwrites_an_earlier_one() {
    let dir = std::env::temp_dir().join(format!("rbx-native-history-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let first = copy_path(&dir, 17675488706, 409, "rbxl");
    assert_eq!(first, dir.join("17675488706-v409.rbxl"));
    std::fs::write(&first, b"edited").unwrap();
    assert_eq!(
        copy_path(&dir, 17675488706, 409, "rbxl"),
        dir.join("17675488706-v409 2.rbxl")
    );
    assert_eq!(
        copy_path(&dir, 17675488706, 409, "rbxlx"),
        dir.join("17675488706-v409.rbxlx")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_restore_uploads_the_old_versions_own_bytes_to_the_linked_place() {
    let sent = RefCell::new(None);
    let restored = restore_with(
        TARGET,
        409,
        PublishMode::Saved,
        |place, version| {
            assert_eq!((place, version), (17675488706, 409));
            Ok(b"<roblox!v409".to_vec())
        },
        |universe, place, bytes, mode| {
            *sent.borrow_mut() = Some((universe, place, bytes.to_vec(), mode));
            Ok(411)
        },
    )
    .unwrap();
    assert_eq!(restored.version, 411);
    // Not a parseable place, so nothing to warn about.
    assert_eq!(restored.warning, None);
    assert_eq!(
        sent.into_inner(),
        Some((
            6053515322,
            17675488706,
            b"<roblox!v409".to_vec(),
            PublishMode::Saved
        ))
    );
}

#[test]
fn a_failed_download_uploads_nothing_and_leaves_the_place_alone() {
    let failure = restore_with(
        TARGET,
        409,
        PublishMode::Published,
        |_, _| {
            Err(CloudError::Http {
                status: 403,
                url: "https://apis.roblox.com/asset-delivery-api/v1/assetId/1/version/409".into(),
            })
        },
        |_, _, _, _| panic!("nothing may be uploaded after a failed download"),
    )
    .err()
    .unwrap();
    assert!(failure.unchanged);
    assert!(
        failure.message.contains("Version 409"),
        "{}",
        failure.message
    );
    assert!(
        failure.message.contains("legacy-asset:manage"),
        "{}",
        failure.message
    );
}

#[test]
fn an_upload_failure_says_whether_the_place_may_have_changed() {
    let fail_with = |err: fn() -> CloudError| {
        restore_with(
            TARGET,
            409,
            PublishMode::Saved,
            |_, _| Ok(b"<roblox!".to_vec()),
            move |_, _, _, _| Err(err()),
        )
        .err()
        .unwrap()
    };
    let refused = fail_with(|| CloudError::Http {
        status: 409,
        url: String::new(),
    });
    assert!(refused.unchanged);
    let dropped = fail_with(|| CloudError::Transport("reset".into()));
    assert!(!dropped.unchanged);
}

#[test]
fn a_refused_read_names_the_scope_it_needs() {
    let err = CloudError::Http {
        status: 403,
        url: "https://apis.roblox.com/place-version-history-api/v1/1/history".into(),
    };
    let message = describe_read(&err, "universe.place:read");
    assert!(message.contains("universe.place:read"), "{message}");
    assert!(message.contains("Manage key"), "{message}");
    assert!(describe_read(&CloudError::NoApiKey, "x").contains("No Open Cloud API key"));
}

#[test]
fn a_restore_line_says_whether_players_get_it() {
    assert_eq!(
        restored_line(1, 409, 411, PublishMode::Saved),
        "Restored version 409 of place 1 as version 411 (saved, not published)"
    );
    assert!(restored_line(1, 409, 411, PublishMode::Published).ends_with("(published)"));
}

#[test]
fn search_matches_every_word_across_date_author_and_label() {
    let mut history = History::default();
    history.learn_names([(7, "Chee".to_string())]);
    let mut v = version(12, Some(7), &[]);
    v.created_time = "2026-10-04T18:02:11.000Z".into();
    assert!(history.matches(&v, None, ""));
    assert!(history.matches(&v, None, "2026-10-04"));
    assert!(history.matches(&v, None, "oct chee"));
    assert!(history.matches(&v, None, "V12 saved"));
    assert!(!history.matches(&v, None, "published"));
    assert!(!history.matches(&v, None, "chee 2026-11"));
    assert!(history.matches(&v, Some("Before the lighting pass"), "lighting"));
    assert!(!history.matches(&v, None, "lighting"));
}

#[test]
fn labels_are_kept_per_place_and_a_blank_one_is_dropped() {
    let dir = std::env::temp_dir().join(format!("rbx-version-labels-{}", std::process::id()));
    let path = dir.join("version-labels.json");
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        labels::save_to(&path, 1, 5, "  Before lights ").unwrap()[&5],
        "Before lights"
    );
    labels::save_to(&path, 2, 5, "Other place").unwrap();
    assert_eq!(labels::load_from(&path, 1)[&5], "Before lights");
    assert!(labels::save_to(&path, 1, 5, " ").unwrap().is_empty());
    assert!(labels::load_from(&path, 1).is_empty());
    assert_eq!(labels::load_from(&path, 2)[&5], "Other place");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn date_presets_count_from_midnight_sunday_and_the_first() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-04T15:00:00Z")
        .unwrap()
        .to_utc();
    let at = |time: &str| PlaceVersion {
        created_time: time.into(),
        ..version(1, None, &[])
    };
    assert!(Since::Today.holds(&at("2026-11-04T00:00:00.000Z"), now));
    assert!(!Since::Today.holds(&at("2026-11-03T23:59:59.000Z"), now));
    // 2026-11-04 is a Wednesday: the week began on Sunday the 1st.
    assert!(Since::ThisWeek.holds(&at("2026-11-01T00:00:00.000Z"), now));
    assert!(!Since::ThisWeek.holds(&at("2026-10-31T23:00:00.000Z"), now));
    assert!(Since::ThisMonth.holds(&at("2026-11-01T00:00:00.000Z"), now));
    assert!(!Since::ThisMonth.holds(&at("2026-10-31T23:00:00.000Z"), now));
    assert!(Since::AllTime.holds(&at("2016-01-01T00:00:00.000Z"), now));
    assert!(Since::ThisMonth.holds(&at("not a date"), now));
}
