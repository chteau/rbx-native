//! The canvas's own property sidebar — Figma's Design panel, in Roblox's
//! terms. While the canvas is up it stands in for the Properties dock (see
//! `Shell::hidden_panels`), and it is built from the very same rows: the
//! same `properties::Properties` model, each row the widget
//! `Shell::property_element` builds for the Properties panel, committed
//! through the same path. Only the arrangement differs: a `GuiObject`'s
//! rows are sorted into the sections a UI designer reaches for, its
//! geometry opened out into fields, and everything else kept one click
//! away under "More properties".

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{is_gui_object, Shell};
use crate::properties::{group_by_category, PropertyRow};
use crate::tokens;

use super::super::rows::section_header;

/// The Properties rows a `GuiObject` keeps under the design fields (see
/// `ui_editor::inspector`), in the sections a UI designer reaches for, and
/// the rows each takes, in order. A row the selection does not have is
/// simply not there.
const SECTIONS: [(&str, &[&str]); 7] = [
    (
        "Text",
        &[
            "Text",
            "FontFace",
            "TextSize",
            "TextColor3",
            "TextTransparency",
            "TextScaled",
            "TextWrapped",
            "TextXAlignment",
            "TextYAlignment",
            "RichText",
        ],
    ),
    (
        "Image",
        &[
            "Image",
            "ImageColor3",
            "ImageTransparency",
            "ScaleType",
            "SliceCenter",
            "ImageRectOffset",
            "ImageRectSize",
        ],
    ),
    (
        "Input",
        &[
            "PlaceholderText",
            "PlaceholderColor3",
            "MultiLine",
            "ClearTextOnFocus",
            "TextEditable",
        ],
    ),
    (
        "Scrolling",
        &[
            "ScrollingEnabled",
            "ScrollingDirection",
            "CanvasSize",
            "AutomaticCanvasSize",
            "ElasticBehavior",
            "ScrollBarThickness",
            "ScrollBarImageColor3",
            "ScrollBarImageTransparency",
            "VerticalScrollBarPosition",
            "VerticalScrollBarInset",
            "HorizontalScrollBarInset",
        ],
    ),
    ("Group", &["GroupColor3", "GroupTransparency"]),
    ("Interaction", &["AutoButtonColor", "Modal", "Selectable"]),
    (
        "Behavior",
        &[
            "AutomaticSize",
            "SizeConstraint",
            "ZIndex",
            "LayoutOrder",
            "Active",
            "Interactable",
        ],
    ),
];

/// Everything no section above names. Shut until opened, where the other
/// sections start open: it is the long tail, kept reachable rather than
/// in the way.
const MORE: &str = "More properties";

/// How wide the sidebar stands.
const WIDTH: f32 = 280.0;

impl Shell {
    pub(super) fn ui_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(anchor) = self.selected() else {
            return sidebar(None, "Select an element on the canvas to edit it here.")
                .into_any_element();
        };
        let folder_color = self.folder_color(anchor);
        let rows = self
            .properties
            .rows(&self.dom, self.selected_all(), folder_color);
        self.properties_nav.begin(&self.tab_order, None, cx);

        let mut sections: Vec<(String, bool, Vec<AnyElement>)> = Vec::new();
        // The name heads every selection's sidebar, a screen's and a
        // modifier's as much as an element's.
        let (named, rows): (Vec<PropertyRow>, Vec<PropertyRow>) =
            rows.into_iter().partition(|row| row.name == "Name");
        let name = named
            .first()
            .map(|row| self.property_element(row, false, window, cx));
        let mut back = None;
        if is_gui_object(&self.dom, &self.database, anchor) {
            let mut rest = rows;
            for (title, wanted) in SECTIONS {
                let mut children = Vec::new();
                for &property in wanted {
                    if let Some(index) = rest.iter().position(|row| row.name == property) {
                        let row = rest.remove(index);
                        children.push(self.property_element(&row, false, window, cx));
                    }
                }
                if !children.is_empty() {
                    let open = !self.is_category_collapsed(title);
                    sections.push((title.to_owned(), open, children));
                }
            }
            let more: Vec<AnyElement> = match self.is_category_collapsed(MORE) {
                true => rest
                    .iter()
                    .map(|row| self.property_element(row, false, window, cx))
                    .collect(),
                false => Vec::new(),
            };
            sections.push((MORE.to_owned(), self.is_category_collapsed(MORE), more));
        } else {
            // A modifier the inspector opened: the way back to its element.
            if let Some(parent) = self
                .dom
                .parent(anchor)
                .filter(|&parent| is_gui_object(&self.dom, &self.database, parent))
            {
                let label = self
                    .dom
                    .get(parent)
                    .map_or_else(String::new, |instance| format!("← {}", instance.name()));
                back = Some(
                    super::super::chrome::button("ui-sidebar-back", label, false)
                        .on_click(cx.listener(move |shell, _, _, cx| shell.select(parent, cx)))
                        .into_any_element(),
                );
            }
            // A screen, a layout, a modifier: nothing here is geometry to
            // sort out, so it reads the way the Properties panel reads it.
            for (category, category_rows) in group_by_category(rows) {
                let children = category_rows
                    .iter()
                    .map(|row| self.property_element(row, false, window, cx))
                    .collect();
                let open = !self.is_category_collapsed(&category);
                sections.push((category, open, children));
            }
        }

        let design = match is_gui_object(&self.dom, &self.database, anchor) {
            true => self.inspector_sections(window, cx),
            false => Vec::new(),
        };
        let handle = cx.entity();
        let body = v_flex()
            .w_full()
            .gap(tokens::header_gap())
            .pb(tokens::panel_padding())
            .children(back)
            .children(name)
            .children(design)
            .children(sections.into_iter().map(|(title, open, children)| {
                let handle = handle.clone();
                let key = title.clone();
                v_flex()
                    .w_full()
                    .child(self.properties_nav.claim(
                        section_header(SharedString::from(title), open, move |_, _, cx| {
                            let key = key.clone();
                            handle.update(cx, |shell, cx| shell.toggle_category(&key, cx));
                        }),
                        cx,
                    ))
                    .when(open, |this| {
                        this.child(
                            v_flex()
                                .w_full()
                                .pt(tokens::section_gap())
                                .pb(tokens::group_gap())
                                .children(children),
                        )
                    })
            }));
        self.properties_nav.finish();

        let scroll = self.ui.sidebar_scroll.clone();
        sidebar(Some(self.properties_title()), "")
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.properties_nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id("ui-sidebar-rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(body)
                    .vertical_scrollbar(&scroll),
            )
            .into_any_element()
    }
}

/// The sidebar's column: the dock's own surface and hairline, a title,
/// and — with nothing to show — a line saying so.
fn sidebar(title: Option<SharedString>, empty: &'static str) -> Div {
    v_flex()
        .flex_none()
        .w(px(WIDTH))
        .h_full()
        .p(px(5.))
        .gap(px(8.))
        .bg(tokens::dock())
        .border_l(px(1.))
        .border_color(tokens::border())
        .child(
            div()
                .px(px(4.))
                .pt(px(2.))
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text_strong())
                .truncate()
                .child(title.unwrap_or_else(|| SharedString::from("Design"))),
        )
        .when(!empty.is_empty(), |this| {
            this.child(
                div()
                    .px(px(4.))
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_muted())
                    .child(empty),
            )
        })
}
