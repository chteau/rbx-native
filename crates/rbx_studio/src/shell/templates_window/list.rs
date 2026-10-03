//! The left pane: a filter and `New` on top, one group per class (its
//! starter first, then the user's templates in the loader's order), the
//! files the loader refused, and the folder at the bottom.

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, icon, mono, text, Weight};
use crate::script_templates::CLASSES;
use crate::tokens;

use super::kit::{open_folder, tag};
use super::{Selected, TemplatesWindow};

const WIDTH: f32 = 288.;

/// The glyph a class's rows and header wear.
pub(super) fn class_glyph(class: &str) -> &'static str {
    if class == "ModuleScript" {
        "package"
    } else {
        "file-code"
    }
}

impl TemplatesWindow {
    pub(super) fn list(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.query(cx);
        let matches = |label: &str| query.is_empty() || label.to_lowercase().contains(&query);
        let view = cx.entity().downgrade();
        let templates = &self.shell.read(cx).script_templates;

        let mut groups: Vec<AnyElement> = Vec::new();
        for class in CLASSES {
            let mine: Vec<_> = templates
                .extras()
                .iter()
                .filter(|t| t.class == class)
                .collect();
            let mut rows: Vec<AnyElement> = Vec::new();
            if matches("Default starter") {
                let yours = templates.default_for(class).is_some();
                let row = Selected::Starter(class);
                let current = self.selected.as_ref() == Some(&row);
                rows.push(
                    self.row(
                        ElementId::Name(format!("starter-{class}").into()),
                        class_glyph(class),
                        "Default starter",
                        row,
                        &view,
                    )
                    .when(!current, |this| this.text_color(tokens::text2()))
                    .child(tag(yours))
                    .into_any_element(),
                );
            }
            for template in mine.iter().filter(|t| matches(&t.name)) {
                let row = Selected::Template {
                    class,
                    name: template.name.clone(),
                };
                if let Some(renaming) = self.rename_row(class, &template.name, class_glyph(class)) {
                    rows.push(renaming);
                    continue;
                }
                let id = ElementId::Name(format!("template-{class}-{}", template.name).into());
                rows.push(
                    self.row(id, class_glyph(class), template.name.clone(), row, &view)
                        .into_any_element(),
                );
            }
            if rows.is_empty() {
                continue;
            }
            groups.push(group(
                header(class_glyph(class), class, mine.len(), tokens::text3()),
                rows,
            ));
        }

        let skipped: Vec<AnyElement> = templates
            .skipped()
            .iter()
            .filter(|s| matches(&s.file_name))
            .map(|s| {
                let row = Selected::Skipped {
                    class: s.class,
                    file_name: s.file_name.clone(),
                };
                let id = ElementId::Name(format!("skipped-{}-{}", s.class, s.file_name).into());
                self.skipped_row(id, s.file_name.clone(), s.summary(), row, &view)
                    .into_any_element()
            })
            .collect();
        if !skipped.is_empty() {
            let count = templates.skipped().len();
            groups.push(group(
                header("triangle-alert", "Skipped", count, tokens::warning()),
                skipped,
            ));
        }

        let folder = templates.dir().map(|dir| dir.to_owned());
        let shown = folder
            .as_deref()
            .map(super::super::settings_window::tilde)
            .unwrap_or_default();

        v_flex()
            .w(px(WIDTH))
            .flex_none()
            .bg(tokens::black())
            .border_r_1()
            .border_color(tokens::border())
            .child(
                h_flex()
                    .gap(px(8.))
                    .pt(px(12.))
                    .px(px(12.))
                    .pb(px(10.))
                    .items_center()
                    .child(self.filter_field(window, cx))
                    .child(
                        ui::icon_button("new", "plus", "New", Weight::Primary, true)
                            .px(px(12.))
                            .on_click(cx.listener(|this, _, window, cx| this.open_new(window, cx))),
                    ),
            )
            .child(
                v_flex()
                    .id("template-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.list_scroll)
                    .gap(px(10.))
                    .pt(px(2.))
                    .px(px(8.))
                    .pb(px(8.))
                    .children(groups),
            )
            .child(
                v_flex()
                    .border_t_1()
                    .border_color(tokens::border())
                    .pt(px(10.))
                    .px(px(12.))
                    .pb(px(12.))
                    .gap(px(4.))
                    .child(
                        h_flex()
                            .items_center()
                            .child(
                                ui::icon_button(
                                    "open-folder",
                                    "folder",
                                    "Open templates folder",
                                    Weight::Ghost,
                                    true,
                                )
                                .on_click(move |_, _, cx| open_folder(folder.as_deref(), cx)),
                            )
                            .child(div().flex_1())
                            .child(
                                h_flex()
                                    .id("import")
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.import(window, cx)),
                                    )
                                    .flex_none()
                                    .size(px(24.))
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(5.))
                                    .text_color(tokens::text3())
                                    .cursor_pointer()
                                    .hover(|this| this.bg(ui::wash()).text_color(tokens::text()))
                                    .child(icon("upload", 13.))
                                    .tooltip(|window, cx| {
                                        super::super::tooltip::text(
                                            "Import .luau files",
                                            window,
                                            cx,
                                        )
                                    }),
                            ),
                    )
                    .child(
                        mono(10.5, 14.)
                            .px(px(8.))
                            .text_color(tokens::text3())
                            .truncate()
                            .child(shown),
                    ),
            )
    }

    fn filter_field(&self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(30.))
            .gap(px(8.))
            .pl(px(10.))
            .pr(px(8.))
            .items_center()
            .border_1()
            .border_color(tokens::border())
            .rounded(px(6.))
            .bg(tokens::field_select())
            .text_color(tokens::text3())
            .child(icon("search", 13.))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.filter)
                        .appearance(false)
                        .px_0()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(tokens::text()),
                ),
            )
    }

    /// A 34 px row: glyph, label, then whatever the caller adds.
    fn row(
        &self,
        id: impl Into<ElementId>,
        glyph: &'static str,
        label: impl Into<SharedString>,
        row: Selected,
        view: &WeakEntity<Self>,
    ) -> Stateful<Div> {
        self.row_frame(id, 34., row, view)
            .text_size(px(12.5))
            .line_height(px(17.))
            .child(
                div()
                    .flex_none()
                    .text_color(tokens::text2())
                    .child(icon(glyph, 14.)),
            )
            .child(div().flex_1().min_w_0().truncate().child(label.into()))
    }

    /// A 44 px row for a refused file: its name in mono over the reason.
    fn skipped_row(
        &self,
        id: impl Into<ElementId>,
        file_name: String,
        reason: String,
        row: Selected,
        view: &WeakEntity<Self>,
    ) -> Stateful<Div> {
        self.row_frame(id, 44., row, view)
            .child(
                div()
                    .flex_none()
                    .text_color(tokens::warning())
                    .child(icon("triangle-alert", 14.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        mono(11.5, 16.)
                            .text_color(tokens::text())
                            .truncate()
                            .child(file_name),
                    )
                    .child(text(11., 15.).text_color(tokens::text3()).child(reason)),
            )
    }

    /// What every row shares: the Settings nav's selected treatment
    /// (accent wash, a 2 px accent bar, the label in text colour).
    fn row_frame(
        &self,
        id: impl Into<ElementId>,
        height: f32,
        row: Selected,
        view: &WeakEntity<Self>,
    ) -> Stateful<Div> {
        let current = self.selected.as_ref() == Some(&row);
        h_flex()
            .id(id.into())
            .relative()
            .h(px(height))
            .flex_none()
            .gap(px(9.))
            .pl(px(10.))
            .pr(px(8.))
            .items_center()
            .rounded(px(6.))
            .text_color(tokens::text())
            .cursor_pointer()
            .map(|this| {
                if current {
                    this.bg(tokens::accent_soft())
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(2.))
                                .rounded_l(px(6.))
                                .bg(tokens::check_on()),
                        )
                } else {
                    this.font_weight(FontWeight::MEDIUM)
                        .hover(|this| this.bg(tokens::hover_subtle()))
                }
            })
            .on_click({
                let view = view.clone();
                move |event, window, cx| {
                    view.update(cx, |this, cx| {
                        this.select(row.clone(), window, cx);
                        // Double-click renames, like F2 and the pencil.
                        if let (true, Selected::Template { class, name }) =
                            (event.click_count() >= 2, &row)
                        {
                            this.start_rename(class, name.clone(), window, cx);
                        }
                    })
                    .ok();
                }
            })
    }
}

fn group(header: Div, rows: Vec<AnyElement>) -> AnyElement {
    v_flex()
        .gap(px(1.))
        .child(header)
        .children(rows)
        .into_any_element()
}

/// A group's 26 px header: glyph, uppercase name, and a count on the right.
fn header(glyph: &'static str, label: &str, count: usize, color: Rgba) -> Div {
    h_flex()
        .h(px(26.))
        .flex_none()
        .gap(px(8.))
        .px(px(10.))
        .items_center()
        .text_color(color)
        .child(icon(glyph, 11.))
        .child(
            text(10.5, 14.)
                .flex_1()
                .font_weight(FontWeight::BOLD)
                .child(label.to_uppercase()),
        )
        .child(mono(10.5, 14.).child(count.to_string()))
}
