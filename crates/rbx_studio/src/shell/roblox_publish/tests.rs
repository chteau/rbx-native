use rbx_cloud::{CloudError, Experience, Owner, PublishMode, Visibility};

use super::upload::{describe, mocked, not_updated, refused, upload_with};
use super::{
    after_pick, confirm, confirmed, outcome, pick_landed, Dialog, Failure, Picking, Target,
};
use crate::command_bar::Feedback;

const TARGET: Target = Target {
    universe_id: 6053515322,
    place_id: 17675488706,
};

#[test]
fn save_and_publish_reach_the_client_with_their_own_mode_and_the_linked_ids() {
    for mode in [PublishMode::Saved, PublishMode::Published] {
        let mut seen = None;
        let result = upload_with(TARGET, b"<roblox!", mode, |universe, place, bytes, sent| {
            seen = Some((universe, place, bytes.to_vec(), sent));
            Ok(12)
        });
        assert_eq!(result, Ok(12));
        assert_eq!(
            seen,
            Some((
                TARGET.universe_id,
                TARGET.place_id,
                b"<roblox!".to_vec(),
                mode
            ))
        );
    }
}

#[test]
fn a_refused_upload_says_why_in_roblox_terms_and_keeps_the_raw_error() {
    let result = upload_with(TARGET, b"", PublishMode::Published, |_, _, _, _| {
        Err(CloudError::Http {
            status: 403,
            url: "https://apis.roblox.com/x".to_string(),
        })
    });
    let failure = result.unwrap_err();
    assert!(failure.unchanged, "a 4xx is a definite refusal");
    let message = failure.message;
    assert!(message.starts_with("Publishing isn\u{2019}t allowed on this place."));
    assert!(message.contains("HTTP 403"));

    let network = upload_with(TARGET, b"", PublishMode::Saved, |_, _, _, _| {
        Err(CloudError::Transport("connection refused".to_string()))
    });
    let network = network.unwrap_err();
    assert_eq!(network.message, "network error: connection refused");
    assert!(
        !network.unchanged,
        "a dropped connection may follow an upload that landed"
    );
    assert!(describe(&CloudError::NoApiKey).contains("No Open Cloud API key"));

    // Roblox's own reason, when its answer has one, is in the message.
    let said = upload_with(TARGET, b"", PublishMode::Saved, |_, _, _, _| {
        Err(CloudError::Refused {
            status: 400,
            message: "INVALID_ARGUMENT: bad place file".to_string(),
        })
    })
    .unwrap_err();
    assert!(said.unchanged);
    assert!(said.message.starts_with("Roblox rejected the place file."));
    assert!(said.message.contains("INVALID_ARGUMENT: bad place file"));
}

#[test]
fn success_is_output_and_failure_is_an_error_row() {
    assert_eq!(
        outcome(TARGET, PublishMode::Published, &Ok(7)),
        Feedback::Output("Published to Roblox as version 7 of place 17675488706".to_string())
    );
    assert_eq!(
        outcome(TARGET, PublishMode::Saved, &Ok(3)),
        Feedback::Output("Saved to Roblox as version 3 of place 17675488706".to_string())
    );
    assert!(matches!(
        outcome(TARGET, PublishMode::Saved, &Err(Failure::before_sending("nope".to_string()))),
        Feedback::Error(message) if message == "Saving to Roblox failed for place 17675488706: nope"
    ));
}

#[test]
fn the_capture_mock_answers_each_case() {
    assert_eq!(mocked("ok").unwrap(), 7);
    assert!(matches!(
        mocked("401"),
        Err(CloudError::Http { status: 401, .. })
    ));
    assert!(matches!(mocked("network"), Err(CloudError::Transport(_))));
}

fn experience(universe_id: u64, root_place_id: u64) -> Experience {
    Experience {
        universe_id,
        root_place_id,
        name: "Fragment - Demo".to_string(),
        visibility: Visibility::Public,
        owner: Owner::User(1),
    }
}

#[test]
fn a_picked_game_links_its_starting_place_and_an_added_place_links_itself() {
    // A card: Roblox's listing carries the starting place only.
    assert_eq!(
        Target::of(&experience(9828239630, 12840211733)),
        Target {
            universe_id: 9828239630,
            place_id: 12840211733
        }
    );
    // Added by the ID of another place in the same experience:
    // `experience_of_place` puts that place where the starting one goes.
    let added = experience(9828239630, 13000000001);
    assert_eq!(Target::of(&added).place_id, 13000000001);
    assert_eq!(Target::of(&added).universe_id, 9828239630);
}

#[test]
fn a_pick_for_an_upload_asks_to_confirm_it_and_a_relink_asks_nothing() {
    let name = || "Fragment - Demo".to_string();
    for mode in [PublishMode::Saved, PublishMode::Published] {
        assert_eq!(
            after_pick(Some(mode), TARGET, name()),
            Some(Dialog::Confirm {
                mode,
                target: TARGET,
                name: Some(name())
            })
        );
    }
    assert_eq!(after_pick(None, TARGET, name()), None);
}

#[test]
fn cancelling_the_confirmation_uploads_nothing() {
    // Cancel and Escape both close it (`close_roblox_dialog`).
    let mut dialog = Some(confirm(PublishMode::Published, TARGET, None));
    assert!(dialog.take().is_some());
    assert_eq!(confirmed(&mut dialog), None);

    // Enter over a failure dialog isn't a confirmation either.
    let failed = Dialog::Failed {
        mode: PublishMode::Saved,
        target: TARGET,
        failure: Failure::before_sending("nope".to_string()),
    };
    let mut dialog = Some(failed);
    assert_eq!(confirmed(&mut dialog), None);
    assert!(matches!(dialog, Some(Dialog::Failed { .. })));
}

#[test]
fn confirming_sends_that_mode_to_that_place_once() {
    for mode in [PublishMode::Saved, PublishMode::Published] {
        let mut dialog = Some(confirm(mode, TARGET, Some("A".to_string())));
        let (target, sent) = confirmed(&mut dialog).expect("a confirmation was open");
        assert!(dialog.is_none());
        assert_eq!(confirmed(&mut dialog), None, "a second Enter does nothing");
        let mut seen = None;
        upload_with(target, b"", sent, |universe, place, _, mode| {
            seen = Some((universe, place, mode));
            Ok(1)
        })
        .unwrap();
        assert_eq!(seen, Some((TARGET.universe_id, TARGET.place_id, mode)));
    }
}

#[test]
fn a_pick_from_a_replaced_picker_neither_links_nor_publishes() {
    // Picker 1 opened for Publish, then replaced by picker 2 for Save.
    let mut picking = Some(Picking {
        token: 2,
        then: Some(PublishMode::Saved),
    });
    assert_eq!(pick_landed(&mut picking, 1), None);
    assert!(picking.is_some(), "picker 2 still waits");

    // Picker 2's pick is the one acted on, with its own mode.
    assert_eq!(pick_landed(&mut picking, 2), Some(Some(PublishMode::Saved)));
    // And a late duplicate of it can't run a second upload.
    assert_eq!(pick_landed(&mut picking, 2), None);
}

#[test]
fn a_pick_after_the_picker_stopped_waiting_changes_nothing() {
    let mut picking = None;
    assert_eq!(pick_landed(&mut picking, 1), None);
}

#[test]
fn only_a_refusal_claims_the_place_was_not_changed() {
    let http = |status| CloudError::Http {
        status,
        url: String::new(),
    };
    assert!(refused(&http(401)));
    assert!(refused(&CloudError::RateLimited { retry_after: None }));
    assert!(refused(&CloudError::NoApiKey));
    assert!(!refused(&http(504)));
    assert!(!refused(&CloudError::Transport("timed out".to_string())));
    let unreadable = serde_json::from_str::<u64>("<html>").unwrap_err();
    assert!(!refused(&CloudError::Json(unreadable)));
}

#[test]
fn a_place_with_unions_warns_that_publish_leaves_them_alone() {
    let mut dom = rbx_dom::WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Model", Some(workspace));
    dom.new_instance("Part", "Part", Some(model));
    assert_eq!(not_updated(&dom), None);

    dom.new_instance("UnionOperation", "Union", Some(model));
    dom.new_instance("SurfaceAppearance", "Look", Some(workspace));
    dom.new_instance("UnionOperation", "Union", Some(workspace));
    let warning = not_updated(&dom).unwrap();
    assert!(
        warning.contains("SurfaceAppearance, UnionOperation instances"),
        "{warning}"
    );
}
