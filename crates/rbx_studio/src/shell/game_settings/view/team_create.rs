//! The rows that run as the Roblox sign-in: Basic Info's Team Create
//! switch and who is in the place's live session, and Security's Allow
//! Mesh / Image APIs.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};

use super::super::super::settings_window::kit;
use super::super::{tokens, GameSettingsWindow, Loading};
use super::rows::{plain_row, row};

const LABEL: &str = "Team Create";
const DESCRIPTION: &str = "Enables collaboration and autosave to cloud.";
const SIGNED_OUT: &str =
    "Turning Team Create on or off needs a Roblox sign-in: Settings \u{203a} Account \u{203a} Roblox sign-in";

// Studio's own strings (Studio.GameSettings.Security.EnableMeshTextureApis*).
const MESH_LABEL: &str = "Allow Mesh / Image APIs";
const MESH_DESCRIPTION: &str = "This experience can access EditableMesh and EditableImage APIs. Using these APIs must comply with the";
const MESH_NOT_OWNER: &str = "Only the group owner can edit this setting.";
const MESH_POLICY: &str =
    "https://help.roblox.com/hc/articles/115004647846-Roblox-Terms-of-Use#creators-restrictions-on-use";
const MESH_SIGNED_OUT: &str =
    "Turning the Mesh / Image APIs on or off needs a Roblox sign-in: Settings \u{203a} Account \u{203a} Roblox sign-in";

impl GameSettingsWindow {
    /// Empty when the file isn't linked: the cloud section already says so.
    pub(super) fn team_create_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.link.is_none() {
            return Vec::new();
        }
        let tc = &self.team_create;
        if tc.web.is_none() {
            let off = kit::toggle("gs-team-create-off", false, |_, _, _| {})
                .opacity(0.4)
                .cursor_not_allowed()
                .tooltip(|window, cx| crate::shell::tooltip::text(SIGNED_OUT, window, cx));
            return vec![row(
                LABEL,
                DESCRIPTION,
                None,
                false,
                off.into_any_element(),
                None,
            )];
        }
        let control = match (&tc.loading, tc.draft.value()) {
            (Loading::Busy, _) | (Loading::Idle, None) => {
                ui::spinner("gs-team-create-loading", 16.)
            }
            (Loading::Failed(_), _) => {
                ui::button("gs-team-create-retry", "Try again", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.load_team_create(cx)))
                    .into_any_element()
            }
            (Loading::Idle, Some(on)) => kit::toggle(
                "gs-team-create",
                on,
                cx.listener(move |this, _, _, cx| this.set_team_create(!on, cx)),
            )
            .into_any_element(),
        };
        let error = match &tc.loading {
            Loading::Failed(message) => Some(message.clone()),
            _ => None,
        };
        vec![
            row(
                LABEL,
                DESCRIPTION,
                None,
                tc.draft.pending().is_some(),
                control,
                error,
            ),
            self.members_row(),
        ]
    }

    /// Security's Allow Mesh / Image APIs; empty when the file isn't linked.
    pub(super) fn mesh_apis_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.link.is_none() {
            return Vec::new();
        }
        let tc = &self.team_create;
        let description = |owner: bool| {
            if !owner {
                return kit::text(11.5, 16.)
                    .text_color(tokens::text2())
                    .child(MESH_NOT_OWNER)
                    .into_any_element();
            }
            v_flex()
                .child(MESH_DESCRIPTION)
                .child(
                    h_flex()
                        .child(
                            div()
                                .id("gs-mesh-apis-terms")
                                .text_color(tokens::accent_line())
                                .cursor_pointer()
                                .child("Terms of Use")
                                .on_click(|_, _, cx| cx.open_url(MESH_POLICY)),
                        )
                        .child("."),
                )
                .into_any_element()
        };
        let disabled = |id: &'static str, on: bool, why: &'static str| {
            kit::toggle(id, on, |_, _, _| {})
                .opacity(0.4)
                .cursor_not_allowed()
                .tooltip(move |window, cx| crate::shell::tooltip::text(why, window, cx))
                .into_any_element()
        };
        if tc.web.is_none() {
            let off = disabled("gs-mesh-apis-off", false, MESH_SIGNED_OUT);
            return vec![row(MESH_LABEL, description(true), None, false, off, None)];
        }
        let control = match (&tc.mesh_apis_loading, tc.mesh_apis.value()) {
            (Loading::Busy, _) | (Loading::Idle, None) => ui::spinner("gs-mesh-apis-loading", 16.),
            (Loading::Failed(_), _) => {
                ui::button("gs-mesh-apis-retry", "Try again", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.load_team_create(cx)))
                    .into_any_element()
            }
            (Loading::Idle, Some(on)) if !tc.mesh_apis_editable => {
                disabled("gs-mesh-apis", on, MESH_NOT_OWNER)
            }
            (Loading::Idle, Some(on)) => kit::toggle(
                "gs-mesh-apis",
                on,
                cx.listener(move |this, _, _, cx| this.set_mesh_apis(!on, cx)),
            )
            .into_any_element(),
        };
        let error = match &tc.mesh_apis_loading {
            Loading::Failed(message) => Some(message.clone()),
            _ => None,
        };
        vec![row(
            MESH_LABEL,
            description(tc.mesh_apis_editable),
            None,
            tc.mesh_apis.pending().is_some(),
            control,
            error,
        )]
    }

    fn members_row(&self) -> AnyElement {
        let quiet = |text: &'static str| {
            kit::text(11.5, 16.)
                .text_color(tokens::text3())
                .child(text)
                .into_any_element()
        };
        let body: Vec<AnyElement> = match &self.team_create.members {
            None => vec![quiet("Checking\u{2026}")],
            Some(Err(message)) => vec![kit::text(11.5, 16.)
                .text_color(ui::red())
                .child(message.clone())
                .into_any_element()],
            Some(Ok(members)) if members.is_empty() => {
                vec![quiet("Nobody is editing this place right now.")]
            }
            Some(Ok(members)) => members
                .iter()
                .map(|m| {
                    h_flex()
                        .gap(px(6.))
                        .items_baseline()
                        .child(kit::text(12.5, 18.).child(m.display_name.clone()))
                        .child(
                            kit::text(11.5, 16.)
                                .text_color(tokens::text3())
                                .child(format!("@{}", m.name)),
                        )
                        .into_any_element()
                })
                .collect(),
        };
        plain_row()
            .child(
                v_flex()
                    .gap(px(4.))
                    .child(kit::text(12.5, 18.).child("In this session"))
                    .children(body),
            )
            .into_any_element()
    }
}
