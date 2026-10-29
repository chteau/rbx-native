//! Which setup steps a launch needs, from the state of the two credentials:
//! the Roblox sign-in (session cookie) and the Open Cloud API key.

/// One credential as a launch finds it. A check that could not reach
/// Roblox counts as `Present`: only a definite refusal is `Expired`.
/// `EnvRefused` is a key Roblox refused that came from `RBX_API_KEY`: it
/// wins over the keychain, so saving a new key would change nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cred {
    Missing,
    Present,
    Expired,
    EnvRefused,
}

/// The setup steps to show, and why they are shown again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SetupPlan {
    /// Show the "Sign in to Roblox" step.
    pub(crate) sign_in: bool,
    /// Show the API key steps (Welcome, Create, Paste, Done).
    pub(crate) key: bool,
    pub(crate) session_expired: bool,
    pub(crate) key_expired: bool,
    /// The key from `RBX_API_KEY` was refused; never true with `key_expired`.
    pub(crate) env_key_refused: bool,
}

impl SetupPlan {
    /// Only the key steps: Home's "set up a key".
    pub(crate) const KEY_ONLY: SetupPlan = SetupPlan {
        sign_in: false,
        key: true,
        session_expired: false,
        key_expired: false,
        env_key_refused: false,
    };

    /// The short lines saying why setup opened again, in display order:
    /// "Your Roblox sign-in expired.", "Your API key expired or was
    /// revoked." then "The API key from RBX_API_KEY was refused." Empty when
    /// nothing expired.
    pub(crate) fn reasons(&self) -> Vec<&'static str> {
        let mut lines = Vec::new();
        if self.session_expired {
            lines.push("Your Roblox sign-in expired.");
        }
        if self.key_expired {
            lines.push("Your API key expired or was revoked.");
        }
        if self.env_key_refused {
            lines.push("The API key from RBX_API_KEY was refused.");
        }
        lines
    }
}

/// The key as routing sees it: a missing key the user already skipped past
/// counts as `Present`, unless `--setup` (`forced`) asks for the wizard.
pub(crate) fn remembering_skip(key: Cred, skipped: bool, forced: bool) -> Cred {
    if key == Cred::Missing && skipped && !forced {
        Cred::Present
    } else {
        key
    }
}

/// The setup a launch without a place path needs, or `None` for Home.
/// `forced` is `--setup`. Rules: a missing or expired key, or `forced`,
/// shows the key steps, with the sign-in step too unless the session is
/// `Present`; a present key with an expired session shows the sign-in step
/// alone; a present key with a present or missing session needs nothing
/// (a never-signed-in user is not nagged). `EnvRefused` routes like
/// `Present`, since no saved key could replace it. The `*_expired` flags
/// mirror `Cred::Expired`; `env_key_refused` mirrors `Cred::EnvRefused`.
pub(crate) fn plan(key: Cred, session: Cred, forced: bool) -> Option<SetupPlan> {
    let session_expired = session == Cred::Expired;
    let key_expired = key == Cred::Expired;
    let env_key_refused = key == Cred::EnvRefused;
    let key_steps = !matches!(key, Cred::Present | Cred::EnvRefused) || forced;
    let sign_in = if key_steps {
        session != Cred::Present
    } else {
        session_expired
    };
    (key_steps || sign_in).then_some(SetupPlan {
        sign_in,
        key: key_steps,
        session_expired,
        key_expired,
        env_key_refused,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use Cred::{EnvRefused, Expired, Missing, Present};

    #[test]
    fn a_skipped_missing_key_goes_home_unless_forced() {
        let key = remembering_skip(Missing, true, false);
        assert_eq!(plan(key, Missing, false), None);
        assert_eq!(plan(key, Present, false), None);
        let forced = plan(remembering_skip(Missing, true, true), Present, true).unwrap();
        assert!(forced.key);
        assert!(plan(remembering_skip(Missing, false, false), Present, false).is_some());
    }

    #[test]
    fn a_skip_does_not_hide_an_expired_key_or_session() {
        assert_eq!(remembering_skip(Expired, true, false), Expired);
        let key = remembering_skip(Missing, true, false);
        let sign_in = plan(key, Expired, false).unwrap();
        assert!(sign_in.sign_in && !sign_in.key);
    }

    #[test]
    fn a_present_key_needs_nothing_unless_the_session_expired() {
        assert_eq!(plan(Present, Present, false), None);
        assert_eq!(plan(Present, Missing, false), None);
        let only_sign_in = plan(Present, Expired, false).unwrap();
        assert!(only_sign_in.sign_in && !only_sign_in.key);
        assert!(only_sign_in.session_expired && !only_sign_in.key_expired);
    }

    #[test]
    fn a_bad_key_shows_the_key_steps_and_sign_in_unless_present() {
        for key in [Missing, Expired] {
            let with_session = plan(key, Present, false).unwrap();
            assert!(with_session.key && !with_session.sign_in);
            assert_eq!(with_session.key_expired, key == Expired);
            for session in [Missing, Expired] {
                let both = plan(key, session, false).unwrap();
                assert!(both.key && both.sign_in);
                assert_eq!(both.session_expired, session == Expired);
            }
        }
    }

    #[test]
    fn forcing_shows_the_key_steps_for_a_present_key() {
        let present = plan(Present, Present, true).unwrap();
        assert!(present.key && !present.sign_in);
        let missing = plan(Present, Missing, true).unwrap();
        assert!(missing.key && missing.sign_in);
        let expired = plan(Present, Expired, true).unwrap();
        assert!(expired.key && expired.sign_in && expired.session_expired);
    }

    #[test]
    fn an_env_refused_key_routes_like_a_present_one() {
        assert_eq!(plan(EnvRefused, Present, false), None);
        assert_eq!(plan(EnvRefused, Missing, false), None);
        let sign_in = plan(EnvRefused, Expired, false).unwrap();
        assert!(sign_in.sign_in && !sign_in.key);
        assert!(sign_in.env_key_refused && !sign_in.key_expired);
        let forced = plan(EnvRefused, Present, true).unwrap();
        assert!(forced.key && forced.env_key_refused);
    }

    #[test]
    fn env_refusal_reason_comes_last() {
        let plan = SetupPlan {
            session_expired: true,
            env_key_refused: true,
            ..SetupPlan::default()
        };
        assert_eq!(
            plan.reasons(),
            [
                "Your Roblox sign-in expired.",
                "The API key from RBX_API_KEY was refused."
            ]
        );
    }

    #[test]
    fn reasons_follow_display_order() {
        assert!(SetupPlan::KEY_ONLY.reasons().is_empty());
        let both = SetupPlan {
            session_expired: true,
            key_expired: true,
            ..SetupPlan::default()
        };
        assert_eq!(
            both.reasons(),
            [
                "Your Roblox sign-in expired.",
                "Your API key expired or was revoked."
            ]
        );
        let key = SetupPlan {
            key_expired: true,
            ..SetupPlan::default()
        };
        assert_eq!(key.reasons(), ["Your API key expired or was revoked."]);
    }
}
