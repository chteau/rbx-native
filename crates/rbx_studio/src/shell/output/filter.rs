//! The Output panel's two filters, as `studio/output.md` lists them: by
//! message type ([`OutputFilter`]) and by context ([`ContextFilter`]).

use super::row_kind::RowKind;
use super::{game, OutputEntry};

/// Which entries [`super::OutputLog::filtered`] shows by type — Studio's own
/// Output window "filters output by type, such as **Error** or **Warning**".
/// One bucket per kind this editor produces: a `print`/successful run, an
/// info line (`TestService.Message` in a running game), a warning and an
/// error.
///
/// One bucket means exactly one kind, which is why a warning does not show
/// under `Output`: a bucket that quietly held two kinds would make the
/// warnings one unable to answer "what is *only* a warning".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum OutputFilter {
    #[default]
    All,
    Output,
    Info,
    Warnings,
    Errors,
}

impl OutputFilter {
    pub(super) const ALL: [OutputFilter; 5] = [
        OutputFilter::All,
        OutputFilter::Output,
        OutputFilter::Info,
        OutputFilter::Warnings,
        OutputFilter::Errors,
    ];

    pub(super) fn matches(self, entry: &OutputEntry) -> bool {
        match self {
            OutputFilter::All => true,
            OutputFilter::Output => entry.kind() == RowKind::Success,
            OutputFilter::Info => entry.kind() == RowKind::Info,
            OutputFilter::Warnings => entry.kind() == RowKind::Warning,
            OutputFilter::Errors => entry.is_error(),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            OutputFilter::All => "All",
            OutputFilter::Output => "Output",
            OutputFilter::Info => "Info",
            OutputFilter::Warnings => "Warnings",
            OutputFilter::Errors => "Errors",
        }
    }
}

/// Which entries show by where they came from — Studio "filters output by
/// context, such as **Client**, **Server**, or **User Plugin**". Everything
/// that is not a message from the running game (a Command Bar run, an app
/// warning) is `Edit`, Studio's name for edit-mode output. This editor runs
/// no plugins, so there is no `User Plugin` context to filter on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContextFilter {
    #[default]
    All,
    Edit,
    Server,
    Client,
}

impl ContextFilter {
    pub(super) const ALL: [ContextFilter; 4] = [
        ContextFilter::All,
        ContextFilter::Edit,
        ContextFilter::Server,
        ContextFilter::Client,
    ];

    pub(super) fn matches(self, entry: &OutputEntry) -> bool {
        match self {
            ContextFilter::All => true,
            ContextFilter::Edit => entry.context().is_none(),
            ContextFilter::Server => entry.context() == Some(game::Context::Server),
            ContextFilter::Client => entry.context() == Some(game::Context::Client),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            ContextFilter::All => "All Contexts",
            ContextFilter::Edit => "Edit",
            ContextFilter::Server => "Server",
            ContextFilter::Client => "Client",
        }
    }
}
