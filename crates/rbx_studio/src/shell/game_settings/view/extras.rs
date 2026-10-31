//! The sign-in rows `extras` reads: Basic Info's icon, thumbnails and
//! genre, Permissions' Playability, and Security's Enable Studio Access to
//! API Services.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};

use super::super::super::settings_window::kit;
use super::super::extras::ICON_IMAGE;
use super::super::media::Slot;
use super::super::{tokens, GameSettingsWindow, Loading};
use super::id;
use super::rows::row;

const SIGNED_OUT: &str =
    "Changing this needs a Roblox sign-in: Settings \u{203a} Account \u{203a} Roblox sign-in";
// Studio's own label and text (Security page).
const API_LABEL: &str = "Enable Studio Access to API Services";
const API_DESCRIPTION: &str = "Enables Studio access to experience services such as Data Stores";
const PLAYABILITY: &str = "Playability";
const PLAYABILITY_DESCRIPTION: &str = "Private experiences only let in you and people who can edit it. Going private drops everyone in its servers.";
const ICON_DESCRIPTION: &str =
    "A square PNG or JPEG, 512\u{d7}512 works best. Roblox reviews it before anyone sees it.";
const THUMBNAILS_DESCRIPTION: &str =
    "Up to 10, 1920\u{d7}1080 works best. Reorder them on the Creator Hub.";
const GENRE_DESCRIPTION: &str =
    "Set on the Creator Hub, which allows a change once every three months.";

const THUMB_W: f32 = 144.;
const THUMB_H: f32 = 81.;

impl GameSettingsWindow {
    pub(super) fn basic_info_extras(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.link.is_none() {
            return Vec::new();
        }
        vec![
            self.icon_row(cx),
            self.thumbnails_row(cx),
            self.genre_row(cx),
        ]
    }

    pub(super) fn permissions_extras(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.link.is_none() || self.team_create.web.is_none() {
            // Signed out, the read-only Open Cloud row stands in.
            return Vec::new();
        }
        let control = self.waiting("gs-public", cx).unwrap_or_else(|| {
            let public = self.extras.public.value().unwrap_or(false);
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

    pub(super) fn security_extras(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.link.is_none() {
            return Vec::new();
        }
        let control = self.waiting("gs-api-access", cx).unwrap_or_else(|| {
            let on = self.extras.api_access.value().unwrap_or(false);
            kit::toggle(
                "gs-api-access",
                on,
                cx.listener(move |this, _, _, cx| this.set_api_access(!on, cx)),
            )
            .into_any_element()
        });
        vec![row(
            API_LABEL,
            API_DESCRIPTION,
            None,
            self.extras.api_access.pending().is_some(),
            control,
            self.failure(),
        )]
    }

    /// The control while there is no value to edit: greyed out signed out,
    /// a spinner while reading, Try again after a failed read.
    fn waiting(&self, prefix: &'static str, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.team_create.web.is_none() {
            return Some(
                ui::disabled_button(id(prefix, "off"), "Sign in to change")
                    .tooltip(|window, cx| crate::shell::tooltip::text(SIGNED_OUT, window, cx))
                    .into_any_element(),
            );
        }
        match &self.extras.loading {
            Loading::Busy => Some(ui::spinner(prefix, 16.)),
            Loading::Failed(_) => Some(
                ui::button(id(prefix, "retry"), "Try again", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.load_extras(cx)))
                    .into_any_element(),
            ),
            Loading::Idle if self.extras.media.remote().is_none() => Some(ui::spinner(prefix, 16.)),
            Loading::Idle => None,
        }
    }

    fn failure(&self) -> Option<String> {
        match &self.extras.loading {
            Loading::Failed(message) => Some(message.clone()),
            _ => None,
        }
    }

    fn icon_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let media = &self.extras.media;
        let control = self.waiting("gs-icon", cx).unwrap_or_else(|| {
            let approved = media
                .remote()
                .and_then(|m| m.icon)
                .is_none_or(|i| i.approved);
            let (image, reviewing) = match &media.icon {
                Some(new) => (new.preview.clone(), false),
                None => (
                    self.extras.images.get(&ICON_IMAGE).cloned(),
                    media
                        .remote()
                        .and_then(|m| m.icon)
                        .is_some_and(|i| i.image_id.is_some())
                        && !approved,
                ),
            };
            // The review tag sits on the image like the thumbnails' do, so
            // the description keeps its width.
            let icon = div()
                .relative()
                .child(tile("gs-icon-tile", image, 64., 64.))
                .when(reviewing, |d| {
                    d.child(
                        h_flex()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(px(4.))
                            .justify_center()
                            .child(ui::tag("In review", ui::accent(), ui::wash())),
                    )
                });
            h_flex()
                .gap(px(12.))
                .items_center()
                .child(icon)
                .child(
                    kit::secondary_button("gs-icon-pick", "image-up", "Change\u{2026}")
                        .on_click(cx.listener(|this, _, _, cx| this.pick_image(true, cx))),
                )
                .into_any_element()
        });
        row(
            "Icon",
            ICON_DESCRIPTION,
            None,
            media.icon.is_some(),
            control,
            None,
        )
    }

    fn thumbnails_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let media = &self.extras.media;
        let waiting = self.waiting("gs-thumbs", cx);
        let edited = media.thumbnails_dirty();
        let head = row(
            "Thumbnails",
            THUMBNAILS_DESCRIPTION,
            None,
            edited,
            waiting.unwrap_or_else(|| {
                kit::text(11.5, 16.)
                    .text_color(tokens::text3())
                    .child(format!(
                        "{} of {}",
                        media.slots.len(),
                        rbx_cloud::MAX_THUMBNAILS
                    ))
                    .into_any_element()
            }),
            None,
        );
        if media.remote().is_none() {
            return head;
        }
        let count = media.slots.len();
        let tiles = media.slots.iter().enumerate().map(|(i, slot)| {
            let (image, reviewing) = match slot {
                Slot::Live(t) => (self.extras.images.get(&t.image_id).cloned(), !t.approved),
                Slot::New(new) => (new.preview.clone(), false),
            };
            div()
                .relative()
                .child(tile(id("gs-thumb", i), image, THUMB_W, THUMB_H))
                .child(h_flex().absolute().top(px(4.)).right(px(4.)).child(
                    overlay_button(id("gs-thumb-remove", i), "x").on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.extras.media.remove(i);
                            this.notice = None;
                            cx.notify();
                        },
                    )),
                ))
                .when(reviewing, |d| {
                    d.child(div().absolute().left(px(4.)).bottom(px(4.)).child(ui::tag(
                        "In review",
                        ui::accent(),
                        ui::wash(),
                    )))
                })
                .into_any_element()
        });
        let add = (count < rbx_cloud::MAX_THUMBNAILS).then(|| {
            v_flex()
                .id("gs-thumb-add")
                .w(px(THUMB_W))
                .h(px(THUMB_H))
                .rounded(px(6.))
                .border_1()
                .border_dashed()
                .border_color(tokens::border2())
                .items_center()
                .justify_center()
                .gap(px(4.))
                .cursor_pointer()
                .text_color(tokens::text2())
                .hover(|d| d.bg(tokens::hover()))
                .child(kit::icon("plus", 14.))
                .child(kit::text(11.5, 16.).child("Add"))
                .on_click(cx.listener(|this, _, _, cx| this.pick_image(false, cx)))
        });
        v_flex()
            .child(head)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .px(px(14.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(tokens::border())
                    .children(tiles)
                    .children(add),
            )
            .into_any_element()
    }

    fn genre_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let universe_id = self.link.as_ref().map_or(0, |l| l.universe_id);
        let control = self.waiting("gs-genre", cx).unwrap_or_else(|| {
            let genre = self.extras.genre.as_ref();
            let text = match genre.and_then(|g| g.genre.as_deref()) {
                None => "Not set".to_string(),
                Some(id) => match genre.and_then(|g| g.subgenre.as_deref()) {
                    Some(sub) => format!("{} \u{203a} {}", genre_name(id), genre_name(sub)),
                    None => genre_name(id),
                },
            };
            kit::text(12.5, 18.)
                .text_color(tokens::text2())
                .child(text)
                .into_any_element()
        });
        let description = v_flex().child(GENRE_DESCRIPTION).child(
            div()
                .id("gs-genre-manage")
                .text_color(tokens::accent_line())
                .cursor_pointer()
                .child("Manage on Creator Hub")
                .on_click(move |_, _, cx| {
                    cx.open_url(&format!(
                        "https://create.roblox.com/dashboard/creations/experiences/{universe_id}/configure"
                    ))
                }),
        );
        row("Genre", description, None, false, control, None)
    }
}

/// A rounded image well: the render, or a placeholder glyph until it arrives.
fn tile(
    id: impl Into<ElementId>,
    image: Option<Arc<RenderImage>>,
    w: f32,
    h: f32,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .w(px(w))
        .h(px(h))
        .flex_none()
        .rounded(px(6.))
        .overflow_hidden()
        .border_1()
        .border_color(tokens::border())
        .bg(tokens::dock())
        .flex()
        .items_center()
        .justify_center()
        .text_color(tokens::text3())
        .map(|d| match image {
            Some(image) => d.child(
                img(image)
                    .size_full()
                    .rounded(px(6.))
                    .object_fit(ObjectFit::Cover),
            ),
            None => d.child(kit::icon("image", 16.)),
        })
}

fn overlay_button(id: impl Into<ElementId>, glyph: &'static str) -> Stateful<Div> {
    div()
        .id(id.into())
        .size(px(20.))
        .rounded(px(4.))
        .bg(tokens::veil())
        .text_color(tokens::text())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|d| d.bg(tokens::secondary_hover()))
        .child(kit::icon(glyph, 12.))
}

/// `obby_and_platformer` as the Creator Hub writes it, `Obby & Platformer`.
fn genre_name(id: &str) -> String {
    match id {
        "rpg" => return "RPG".to_string(),
        "pve_shooter" => return "PvE Shooter".to_string(),
        _ => {}
    }
    id.split('_')
        .map(|word| match word {
            "and" => "&".to_string(),
            "rpg" => "RPG".to_string(),
            "vs" => "vs".to_string(),
            _ => {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::genre_name;

    #[test]
    fn genre_ids_read_like_the_creator_hub_s_names() {
        assert_eq!(genre_name("obby_and_platformer"), "Obby & Platformer");
        assert_eq!(genre_name("rpg"), "RPG");
        assert_eq!(genre_name("action_rpg"), "Action RPG");
        assert_eq!(genre_name("one_vs_all"), "One vs All");
        assert_eq!(genre_name("pve_shooter"), "PvE Shooter");
    }
}
