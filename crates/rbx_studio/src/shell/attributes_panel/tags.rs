//! The Tags section: `CollectionService` tags as removable chips, plus the
//! section's own "add tag" row.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};

use crate::properties::attributes as attrs;
use crate::shell::chrome;
use crate::shell::rows::{field_box, section_header};
use crate::shell::Shell;
use crate::tokens;

use super::TAGS_CATEGORY;

impl Shell {
    /// Filtered by tag name through the same [`crate::properties::matches`]
    /// the ordinary property rows and the Attributes section above use; the
    /// "add tag" row is never filtered out, for the same reason
    /// `attribute_section`'s "add attribute" row isn't.
    pub(super) fn tag_section(
        &mut self,
        reference: Ref,
        filter: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let filtering = !filter.trim().is_empty();
        let open = filtering || !self.is_category_collapsed(TAGS_CATEGORY);
        let current = attrs::tags_matching(&self.dom, reference, filter);
        let error = self.attribute_edits.tag_error.clone();

        let chips: Vec<AnyElement> = current.iter().map(|tag| self.tag_chip(tag, cx)).collect();

        let handle = cx.entity();
        v_flex()
            .w_full()
            .child(self.properties_nav.claim(
                section_header(SharedString::from(TAGS_CATEGORY), open, {
                    let handle = handle.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.toggle_category(TAGS_CATEGORY, cx));
                    }
                }),
                cx,
            ))
            .when(open, |this| {
                this.child(
                    v_flex()
                        .w_full()
                        .pt(tokens::section_gap())
                        .pb(tokens::group_gap())
                        .gap(tokens::row_gap())
                        .pl(crate::shell::rows::name_indent(0))
                        .pr(tokens::row_padding())
                        .child(
                            h_flex()
                                .w_full()
                                .flex_wrap()
                                .gap(tokens::label_gap())
                                .children(chips),
                        )
                        .child(self.add_tag_row(reference, window, cx))
                        .when_some(error, |this, message| {
                            this.child(
                                div()
                                    .text_size(tokens::text_sm())
                                    .line_height(tokens::line_sm())
                                    .text_color(tokens::text_error())
                                    .child(SharedString::from(message)),
                            )
                        }),
                )
            })
    }

    fn tag_chip(&mut self, tag: &str, cx: &mut Context<Self>) -> AnyElement {
        let handle = cx.entity();
        let removed = tag.to_owned();

        h_flex()
            .id(SharedString::from(format!("tag-{tag}")))
            .flex_none()
            .items_center()
            .gap(tokens::label_gap())
            .h(tokens::hit_target())
            .px(tokens::input_padding())
            .rounded(tokens::RADIUS)
            .bg(tokens::field_select())
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text_strong())
            .child(SharedString::from(tag.to_owned()))
            .child(
                self.properties_nav.claim(
                    // `chrome::icon_button` rather than a hand-rolled div: it
                    // already carries WCAG 2.5.8's 24x24 target, a focus ring
                    // and a tooltip — a smaller one-off `x` here would fail
                    // the same target-size bar this panel is held to
                    // elsewhere (see `UX_GUIDELINES.md` §4).
                    chrome::icon_button(
                        SharedString::from(format!("tag-remove-{tag}")),
                        IconName::X,
                        "Remove tag",
                    )
                    .on_click(move |_, _, cx| {
                        let removed = removed.clone();
                        handle.update(cx, |shell, cx| shell.remove_tag(&removed, cx));
                    }),
                    cx,
                ),
            )
            .into_any_element()
    }

    fn add_tag_row(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = match &self.attribute_edits.new_tag {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("New tag"));
                let subscription = cx.subscribe(&input, move |shell, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        shell.commit_add_tag(cx);
                    }
                });
                self.attribute_edits.new_tag = Some(input.clone());
                self.attribute_edits._subscriptions.push(subscription);
                input
            }
        };

        let handle = cx.entity();
        h_flex()
            .w_full()
            .items_center()
            .gap(tokens::label_gap())
            .child(
                field_box().flex_1().child(
                    Input::new(&input)
                        .appearance(false)
                        .with_size(tokens::field_size())
                        .h_full()
                        .tab_index(self.tab_order.next()),
                ),
            )
            .child(
                self.properties_nav.claim(
                    chrome::icon_button(
                        SharedString::from(format!("tag-add-{}", reference.value())),
                        IconName::Plus,
                        "Add tag",
                    )
                    .on_click(move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.commit_add_tag(cx));
                    }),
                    cx,
                ),
            )
            .into_any_element()
    }

    fn commit_add_tag(&mut self, cx: &mut Context<Self>) {
        let Some(reference) = self.selected() else {
            return;
        };
        let Some(input) = self.attribute_edits.new_tag.clone() else {
            return;
        };
        let tag = input.read(cx).value().trim().to_owned();

        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = attrs::add_tag(&mut dom, reference, &tag);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);

        match result {
            Ok(()) => {
                self.attribute_edits.tag_error = None;
                // Same reasoning as `commit_add_attribute`'s identical line:
                // dropping the entity makes the next render build a fresh,
                // empty one rather than needing a `Window` to clear this one
                // in place.
                self.attribute_edits.new_tag = None;
            }
            Err(message) => self.attribute_edits.tag_error = Some(message),
        }
        cx.notify();
    }

    fn remove_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        let Some(reference) = self.selected() else {
            return;
        };
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = attrs::remove_tag(&mut dom, reference, tag);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);

        match result {
            Ok(()) => self.attribute_edits.tag_error = None,
            Err(message) => self.attribute_edits.tag_error = Some(message),
        }
        cx.notify();
    }
}
