//! Permissions' Playability: public or private, as the Roblox sign-in.
//! Roblox only lets an experience go public once it passes its activation
//! gate (the Maturity & Compliance Questionnaire, and the account's own
//! checks); short of that `activate` answers 403 "You are not authorized to
//! configure this universe", so a private experience that fails the gate
//! gets a Creator Hub link instead of the switch. A public one keeps it, since
//! going private has no gate.

use std::rc::Rc;

use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::super::super::settings_window::kit;
use super::super::{tokens, GameSettingsWindow};
use super::extras::creator_hub_link;
use super::rows::row;

const PLAYABILITY: &str = "Playability";
const PLAYABILITY_DESCRIPTION: &str = "Private experiences only let in you and people who can edit it. Going private drops everyone in its servers.";

impl GameSettingsWindow {
    pub(super) fn permissions_extras(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.link.is_none() || self.team_create.web.is_none() {
            // Signed out, the read-only Open Cloud row stands in.
            return Vec::new();
        }
        let public = self.extras.public.value().unwrap_or(false);
        if let Some(why) = self
            .extras
            .eligibility
            .filter(|_| !public)
            .and_then(refusal)
        {
            let universe_id = self.link.as_ref().map_or(0, |l| l.universe_id);
            let description = v_flex()
                .child(format!("Roblox won't make this one public yet: {why}."))
                .child(creator_hub_link(
                    "gs-public-manage",
                    "Change this on the Creator Hub",
                    universe_id,
                ));
            let control = kit::text(12.5, 18.)
                .text_color(tokens::text2())
                .child("Private");
            return vec![row(
                PLAYABILITY,
                description,
                None,
                false,
                control.into_any_element(),
                None,
            )];
        }
        let control = self.waiting("gs-public", cx).unwrap_or_else(|| {
            let entity = cx.entity().downgrade();
            let pick = move |on: bool| -> kit::OnPick {
                let entity = entity.clone();
                Rc::new(move |_, cx: &mut App| {
                    let _ = entity.update(cx, |this, cx| this.set_public(on, cx));
                })
            };
            kit::segmented(
                "gs-public",
                26.,
                vec![
                    ("Public", public, pick(true)),
                    ("Private", !public, pick(false)),
                ],
            )
            .into_any_element()
        });
        vec![row(
            PLAYABILITY,
            PLAYABILITY_DESCRIPTION,
            None,
            self.extras.public.pending().is_some(),
            control,
            self.failure(),
        )]
    }
}

/// What stops the experience going public, in words; `None` when nothing.
fn refusal(gate: rbx_cloud::PublicEligibility) -> Option<&'static str> {
    match gate {
        g if g.eligible => None,
        g if !g.maturity_rated => Some("it needs the Maturity & Compliance Questionnaire first"),
        g if !g.user_eligible => {
            Some("your account hasn't passed its checks for public experiences")
        }
        _ => Some("it isn't eligible"),
    }
}

#[cfg(test)]
mod tests {
    use super::refusal;
    use rbx_cloud::PublicEligibility;

    fn gate(eligible: bool, maturity_rated: bool, user_eligible: bool) -> PublicEligibility {
        PublicEligibility {
            eligible,
            maturity_rated,
            user_eligible,
        }
    }

    #[test]
    fn the_questionnaire_is_named_before_the_account() {
        assert_eq!(refusal(gate(true, false, false)), None);
        assert!(refusal(gate(false, false, false))
            .unwrap()
            .contains("Questionnaire"));
        assert!(refusal(gate(false, true, false))
            .unwrap()
            .contains("account"));
        assert_eq!(refusal(gate(false, true, true)), Some("it isn't eligible"));
    }
}
