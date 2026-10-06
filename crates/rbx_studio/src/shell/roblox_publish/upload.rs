//! The upload itself, off the UI thread, and what its answer means: the
//! classes Roblox's API leaves alone, the capture mock, and Roblox's own
//! words for each refusal.

use std::collections::BTreeSet;

use rbx_cloud::{ApiKey, Client, CloudError, PublishMode};
use rbx_dom::WeakDom;

use crate::command_bar::Feedback;

use super::{Failure, Target, MOCK_VARIABLE};

/// Classes Roblox's place-publishing API leaves as they were
/// (`creator-docs`, `cloud/guides/usage-place-publishing.md`: EditableImage,
/// EditableMesh, PartOperation, SurfaceAppearance, BaseWrap): edits to them
/// only go live when published from Roblox Studio.
const NOT_UPDATED_BY_PUBLISH: [&str; 9] = [
    "EditableImage",
    "EditableMesh",
    "PartOperation",
    "UnionOperation",
    "NegateOperation",
    "IntersectOperation",
    "SurfaceAppearance",
    "WrapLayer",
    "WrapTarget",
];

/// The warning a successful upload adds when the place holds any of
/// [`NOT_UPDATED_BY_PUBLISH`], naming the ones it holds.
pub(super) fn not_updated(dom: &WeakDom) -> Option<String> {
    let mut found = BTreeSet::new();
    let mut stack = dom.root_refs().to_vec();
    while let Some(instance) = stack.pop().and_then(|r| dom.get(r)) {
        if let Some(class) = NOT_UPDATED_BY_PUBLISH
            .iter()
            .find(|c| **c == instance.class())
        {
            found.insert(*class);
        }
        stack.extend_from_slice(instance.children());
    }
    (!found.is_empty()).then(|| {
        format!(
            "Note: Roblox doesn\u{2019}t update {} instances through this upload \u{2014} changes to them only go live when published from Roblox Studio.",
            found.into_iter().collect::<Vec<_>>().join(", ")
        )
    })
}

fn mock() -> Option<String> {
    std::env::var(MOCK_VARIABLE).ok()
}

/// What a mocked upload answers instead of Roblox.
pub(super) fn mocked(which: &str) -> Result<u64, CloudError> {
    match which {
        "ok" => Ok(7),
        "network" => Err(CloudError::Transport("connection refused".to_string())),
        status => Err(CloudError::Http {
            status: status.parse().unwrap_or(500),
            url: "https://apis.roblox.com/universes/v1/…/versions?<redacted>".to_string(),
        }),
    }
}

/// Blocking: one upload, through the stored key.
pub(super) fn upload(target: Target, bytes: &[u8], mode: PublishMode) -> Result<u64, Failure> {
    if let Some(which) = mock() {
        return upload_with(target, bytes, mode, |_, _, _, _| mocked(&which));
    }
    let Some(key) = ApiKey::from_env_or_config() else {
        return Err(Failure::before_sending(describe(&CloudError::NoApiKey)));
    };
    let client = Client::new(Some(key));
    upload_with(target, bytes, mode, |universe, place, bytes, mode| {
        client.publish_place(universe, place, bytes, mode)
    })
}

/// The seam the tests drive: `publish` stands in for `Client::publish_place`.
pub(super) fn upload_with(
    target: Target,
    bytes: &[u8],
    mode: PublishMode,
    publish: impl FnOnce(u64, u64, &[u8], PublishMode) -> Result<u64, CloudError>,
) -> Result<u64, Failure> {
    publish(target.universe_id, target.place_id, bytes, mode).map_err(|err| Failure {
        message: describe(&err),
        unchanged: refused(&err),
    })
}

/// Whether `err` means Roblox definitely didn't take the upload: no key to
/// send it with, or a 4xx answer. A 5xx is left out with the network
/// errors — a gateway timing out may sit in front of a publish that landed.
pub(super) fn refused(err: &CloudError) -> bool {
    match err {
        CloudError::NoApiKey | CloudError::RateLimited { .. } => true,
        CloudError::Http { status, .. } | CloudError::Refused { status, .. } => {
            (400..500).contains(status)
        }
        _ => false,
    }
}

/// Roblox's own reasons for each status the endpoint documents
/// (`creator-docs`, `reference/cloud/universes-api/v1.json`), ahead of the
/// raw error, so a refusal says what to fix.
pub(super) fn describe(err: &CloudError) -> String {
    let status = match err {
        CloudError::Http { status, .. } | CloudError::Refused { status, .. } => Some(*status),
        _ => None,
    };
    let reason = match (status, err) {
        (Some(400), _) => "Roblox rejected the place file.",
        (Some(401), _) => {
            "The API key isn\u{2019}t valid for this place: it needs universe-places:write on this experience, or it may have expired or been revoked \u{2014} Home \u{203a} Manage key."
        }
        (Some(403), _) => "Publishing isn\u{2019}t allowed on this place.",
        (Some(404), _) => "The place or its experience doesn\u{2019}t exist.",
        (Some(409), _) => "The place isn\u{2019}t part of that experience.",
        (_, CloudError::NoApiKey) => {
            return "No Open Cloud API key is set up. Add one from Home \u{203a} Manage key.".to_string()
        }
        _ => return err.to_string(),
    };
    format!("{reason} ({err})")
}

/// The ing-form and past tense each mode's messages use.
pub(super) fn verb(mode: PublishMode) -> (&'static str, &'static str) {
    match mode {
        PublishMode::Saved => ("Saving to Roblox", "Saved to Roblox"),
        PublishMode::Published => ("Publishing to Roblox", "Published to Roblox"),
    }
}

pub(super) fn outcome(
    target: Target,
    mode: PublishMode,
    result: &Result<u64, Failure>,
) -> Feedback {
    match result {
        Ok(version) => Feedback::Output(format!(
            "{} as version {version} of place {}",
            verb(mode).1,
            target.place_id
        )),
        Err(failure) => Feedback::Error(format!(
            "{} failed for place {}: {}",
            verb(mode).0,
            target.place_id,
            failure.message
        )),
    }
}
