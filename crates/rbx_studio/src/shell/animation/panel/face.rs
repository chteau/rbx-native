//! The panel's Face section: a region picker and one slider per FACS
//! channel in it, each showing the channel's value at the playhead and
//! keying it there when moved, as Roblox's Face Animation Editor does. The
//! value plays back onto the head's face only in Roblox; the viewer cannot
//! deform a dynamic head (its FACS mapping is not exposed).

use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_animation::facs;
use rbx_animation::sample::{sample_face, wrap};

use super::super::widgets::{picker, section};
use super::Shell;
use crate::shell::menu::{self, MenuId};
use crate::tokens;

impl Shell {
    fn face_slider(
        &mut self,
        channel: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<SliderState>> {
        let fields = self.anim.fields.as_mut()?;
        if let Some(s) = fields.face_sliders.get(channel) {
            return Some(s.clone());
        }
        let state = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(1.0)
                .step(0.01)
                .default_value(0.0)
        });
        let name = channel.to_owned();
        let subscription = cx.subscribe(&state, move |shell, _, event: &SliderEvent, cx| {
            match event {
                SliderEvent::Change(SliderValue::Single(v)) => {
                    shell.anim.editor.key_face(&name, *v, true)
                }
                SliderEvent::Release(_) => shell.anim.editor.end_face_drag(),
                SliderEvent::Change(SliderValue::Range(..)) => {}
            }
            cx.notify();
        });
        let fields = self.anim.fields.as_mut()?;
        fields._subscriptions.push(subscription);
        fields
            .face_sliders
            .insert(channel.to_owned(), state.clone());
        Some(state)
    }

    pub(in crate::shell::animation) fn anim_face_section(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let region = self.anim.fields.as_ref().map_or(0, |f| f.face_region);
        let (region_name, channels) = facs::REGIONS[region.min(facs::REGIONS.len() - 1)];
        let items = facs::REGIONS
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                menu::item(*name)
                    .checked(i == region)
                    .on_click(move |shell, cx| {
                        if let Some(f) = &mut shell.anim.fields {
                            f.face_region = i;
                        }
                        cx.notify();
                    })
            })
            .collect();
        let at = wrap(&self.anim.editor.clip, self.anim.editor.time);
        let mut rows = Vec::new();
        for channel in channels {
            let Some(state) = self.face_slider(channel, cx) else {
                continue;
            };
            let value = sample_face(&self.anim.editor.clip, channel, at).unwrap_or(0.0);
            let shown = match state.read(cx).value() {
                SliderValue::Single(v) => v,
                SliderValue::Range(_, v) => v,
            };
            // Follows the playhead unless this very slider is being dragged.
            if self.anim.editor.selected.first().map(|k| k.0.as_str())
                != Some(&format!("face:{channel}"))
                && (shown - value).abs() > 1e-4
            {
                state.update(cx, |s, cx| s.set_value(value, window, cx));
            }
            rows.push(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(
                        div()
                            .w(px(118.))
                            .flex_none()
                            .truncate()
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_label())
                            .child(*channel),
                    )
                    .child(crate::shell::rows::rail_slider(&state, cx))
                    .child(
                        div()
                            .w(px(30.))
                            .flex_none()
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_muted())
                            .child(format!("{value:.2}")),
                    ),
            );
        }
        section("Face")
            .child(picker(
                self,
                MenuId::AnimFaceRegion,
                "anim-face-region",
                region_name,
                items,
                cx,
            ))
            .child(v_flex().gap(px(2.)).children(rows))
    }
}
