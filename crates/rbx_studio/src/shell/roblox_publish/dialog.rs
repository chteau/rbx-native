//! The publish dialogs and the failure they report.

use rbx_cloud::PublishMode;

use super::Target;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::shell) enum Dialog {
    /// Asked before every upload; `name` is the experience's, when known.
    Confirm {
        mode: PublishMode,
        target: Target,
        name: Option<String>,
    },
    Failed {
        mode: PublishMode,
        target: Target,
        failure: Failure,
    },
}

/// A failed upload. `unchanged` is whether the place is known to be as it
/// was: true only when the upload never left or Roblox answered with a
/// refusal. A dropped connection, a timeout or an unreadable answer may
/// come after Roblox took the file, so those say so instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct Failure {
    pub(in crate::shell) message: String,
    pub(in crate::shell) unchanged: bool,
}

impl Failure {
    pub(super) fn before_sending(message: String) -> Self {
        Failure {
            message,
            unchanged: true,
        }
    }
}
