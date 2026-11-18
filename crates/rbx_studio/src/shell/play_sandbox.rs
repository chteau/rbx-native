//! Play's sandbox, made on demand: the user's own "RbxNative Sandbox"
//! experience, reused when it already exists and created private when not.
//! Open Cloud can neither create experiences nor list private ones, so both
//! run as the Roblox web sign-in. Without a sign-in only the API key's
//! listing is left, which is enough to reuse a sandbox but not to make one.

use rbx_cloud::heartbeats::SessionCookie;
use rbx_cloud::{ApiKey, Client, OwnExperience, Owner, Visibility};

use crate::settings::PlaySandbox;

pub(in crate::shell) const NAME: &str = "RbxNative Sandbox";
const DESCRIPTION: &str =
    "RbxNative's Play sandbox. Play publishes test copies here, so it is safe to leave private.";

pub(in crate::shell) const SETTING: &str = "Settings \u{203a} Account \u{203a} Play";

/// Where the experience is managed.
pub(in crate::shell) fn dashboard_url(universe_id: u64) -> String {
    format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/overview")
}

/// The Output line the session's first Play starts with, so the experience
/// a test lands in is never a mystery.
pub(in crate::shell) fn origin(sandbox: PlaySandbox) -> String {
    format!(
        "Play publishes test copies to your sandbox experience (place {}): {}. You can change it in {SETTING}.",
        sandbox.place_id,
        dashboard_url(sandbox.universe_id)
    )
}

pub(in crate::shell) struct Made {
    pub(in crate::shell) sandbox: PlaySandbox,
    /// False when an existing sandbox was reused.
    pub(in crate::shell) created: bool,
    /// Something that went wrong after the experience existed.
    pub(in crate::shell) warning: Option<String>,
}

/// Whether `description` is the one [`provision`] gives the sandbox it
/// makes: Play keeps that experience called [`NAME`] rather than naming it
/// after each test copy, so it stays findable in the user's creations.
pub(in crate::shell) fn is_made(description: &str) -> bool {
    description == DESCRIPTION
}

/// The user's own experience called [`NAME`], or carrying its description:
/// older builds renamed the sandbox to the test copy's name, so the name
/// alone may not match. The lowest universe id when there are several.
pub(in crate::shell) fn find_existing(listing: &[OwnExperience]) -> Option<&OwnExperience> {
    listing
        .iter()
        .filter(|e| {
            matches!(e.experience.owner, Owner::User(_))
                && (e.experience.name == NAME || is_made(&e.description))
        })
        .min_by_key(|e| e.experience.universe_id)
}

fn sandbox_of(OwnExperience { experience, .. }: &OwnExperience) -> PlaySandbox {
    PlaySandbox {
        universe_id: experience.universe_id,
        place_id: experience.root_place_id,
    }
}

/// Reuses the listed sandbox, else creates one. A failed listing never
/// falls through to creating: that is how a second one would appear.
pub(in crate::shell) fn provision_with(
    list: impl FnOnce() -> Result<Vec<OwnExperience>, String>,
    create: impl FnOnce() -> Result<Made, String>,
) -> Result<Made, String> {
    let listing = list().map_err(|err| {
        format!("Could not check for an existing sandbox ({err}). Paste a place link in {SETTING} instead.")
    })?;
    match find_existing(&listing) {
        Some(found) => Ok(Made {
            sandbox: sandbox_of(found),
            created: false,
            warning: None,
        }),
        None => create(),
    }
}

pub(in crate::shell) fn provision(
    key: Option<ApiKey>,
    session: Option<SessionCookie>,
) -> Result<Made, String> {
    let Some(key) = key else {
        return Err(format!(
            "Play needs an API key to make its sandbox. Add one in Settings \u{203a} Account, or paste a place link in {SETTING}."
        ));
    };
    let client = Client::new(Some(key));
    provision_with(
        || match &session {
            Some(cookie) => rbx_cloud::own_experiences(cookie).map_err(|err| err.to_string()),
            None => client
                .list_experiences()
                .map(|listing| {
                    (listing.experiences.into_iter())
                        .map(|experience| OwnExperience {
                            experience,
                            description: String::new(),
                        })
                        .collect()
                })
                .map_err(|err| err.to_string()),
        },
        || {
            let Some(cookie) = &session else {
                return Err(format!(
                    "Making the sandbox needs your Roblox sign-in. Sign in under Settings \u{203a} Account, or paste a place link in {SETTING}."
                ));
            };
            create(&client, cookie)
        },
    )
}

fn create(client: &Client, cookie: &SessionCookie) -> Result<Made, String> {
    let created = rbx_cloud::create_experience(cookie, None).map_err(|err| match err {
        rbx_cloud::CloudError::Refused { status: 401, .. } => {
            "Your Roblox sign-in has expired: sign in again in Settings \u{203a} Account."
                .to_owned()
        }
        other => format!("Roblox refused to make the sandbox ({other}). Paste a place link in {SETTING} instead."),
    })?;
    let sandbox = PlaySandbox {
        universe_id: created.universe_id,
        place_id: created.root_place_id,
    };
    // The experience exists from here on, so problems are warnings.
    let mut warning = rbx_cloud::name_experience(cookie, created, NAME, DESCRIPTION)
        .err()
        .map(|err| {
            format!(
                "Roblox made experience {} but refused its name ({err}). Rename it to \u{201c}{NAME}\u{201d} in Game Settings.",
                created.universe_id
            )
        });
    if let Ok(universe) = client.universe(created.universe_id) {
        if universe.visibility != Visibility::Private {
            let note = format!(
                "The sandbox experience {} is not private. Set it to private in Game Settings.",
                created.universe_id
            );
            warning = Some(warning.map_or(note.clone(), |w| format!("{w} {note}")));
        }
    }
    Ok(Made {
        sandbox,
        created: true,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_origin_line_names_the_place_and_where_to_manage_it() {
        let line = origin(PlaySandbox {
            universe_id: 7,
            place_id: 42,
        });
        assert!(line.contains("place 42"));
        assert!(line.contains(&dashboard_url(7)));
        assert!(line.contains(SETTING));
    }

    fn experience(universe_id: u64, name: &str, owner: Owner) -> OwnExperience {
        OwnExperience {
            experience: rbx_cloud::Experience {
                universe_id,
                root_place_id: universe_id + 1000,
                name: name.to_owned(),
                visibility: Visibility::Private,
                owner,
            },
            description: String::new(),
        }
    }

    fn made() -> Made {
        Made {
            sandbox: PlaySandbox {
                universe_id: 9,
                place_id: 99,
            },
            created: true,
            warning: None,
        }
    }

    #[test]
    fn an_empty_account_creates() {
        let got = provision_with(|| Ok(vec![]), || Ok(made())).unwrap();
        assert!(got.created);
        assert_eq!(got.sandbox.universe_id, 9);
    }

    #[test]
    fn a_named_sandbox_is_reused_and_nothing_is_created() {
        let listing = vec![
            experience(5, "My game", Owner::User(1)),
            experience(7, NAME, Owner::User(1)),
        ];
        let got = provision_with(
            || Ok(listing),
            || panic!("must not create a second sandbox"),
        )
        .unwrap();
        assert!(!got.created);
        assert_eq!(
            got.sandbox,
            PlaySandbox {
                universe_id: 7,
                place_id: 1007
            }
        );
    }

    #[test]
    fn the_oldest_of_several_wins_and_group_copies_do_not_count() {
        let listing = vec![
            experience(3, NAME, Owner::Group(4)),
            experience(8, NAME, Owner::User(1)),
            experience(6, NAME, Owner::User(1)),
        ];
        assert_eq!(find_existing(&listing).unwrap().experience.universe_id, 6);
    }

    #[test]
    fn a_sandbox_renamed_by_play_is_found_by_its_description() {
        let mut renamed = experience(4, "MyPlace (Play)", Owner::User(1));
        renamed.description = DESCRIPTION.to_owned();
        let listing = vec![experience(2, "My game", Owner::User(1)), renamed];
        assert_eq!(find_existing(&listing).unwrap().experience.universe_id, 4);
    }

    #[test]
    fn a_failed_listing_says_why_and_never_creates() {
        let err = provision_with(
            || Err("HTTP 500".into()),
            || panic!("must not create blind"),
        )
        .err()
        .unwrap();
        assert!(err.contains("HTTP 500") && err.contains("Settings"));
    }

    #[test]
    fn a_failed_creation_passes_its_reason_on() {
        let err = provision_with(|| Ok(vec![]), || Err("sign in".into()))
            .err()
            .unwrap();
        assert_eq!(err, "sign in");
    }

    #[test]
    fn no_key_names_the_setting() {
        let err = provision(None, None).err().unwrap();
        assert!(err.contains("API key") && err.contains("Play"));
    }
}
