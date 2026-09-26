//! The Appearance page: the accent (presets, a custom colour and its
//! guard), the theme, the interface's icon pack and scale, and the
//! transform tools' colours.

use gpui_kit::assets::IconName;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, Icon, IndexPath, Sizable as _};
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::theme;
use crate::tokens;

use super::dragger::number;
use super::kit::{self, icon, readout, ticked_slider, Reset, Row, Section};
use super::SettingsWindow;

mod accent_card;
mod picker;
mod preview;
mod theme_cards;

pub(super) use picker::{Picker, Target};

type Choices = SearchableVec<SharedString>;

/// The transform tools, by the key their colour is stored under and the
/// name their chip shows, in ribbon order.
const TOOLS: [(&str, &str); 7] = [
    ("select", "Select"),
    ("move", "Move"),
    ("scale", "Scale"),
    ("rotate", "Rotate"),
    ("align", "Align"),
    ("local", "Local"),
    ("sun", "Sun"),
];

/// The Appearance page's controls that keep state: the two dropdowns and
/// the UI scale slider.
pub(super) struct AppearanceControls {
    icon_pack: Entity<SelectState<Choices>>,
    /// What each row of the icon pack dropdown picks.
    icon_packs: Vec<(Option<String>, IconPack)>,
    theme: Entity<SelectState<Choices>>,
    themes: Vec<String>,
    ui_scale: Entity<SliderState>,
    /// The Script font size field, on the roadmap.
    script_font: Entity<gpui_kit::component::input::InputState>,
}

impl AppearanceControls {
    pub(super) fn new(
        shell: &Entity<super::super::Shell>,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> (Self, Vec<Subscription>) {
        let (installed, pack) = {
            let shell = shell.read(cx);
            let (installed, chosen) = shell.installed_icon_packs();
            (
                installed.to_vec(),
                (chosen.map(str::to_owned), shell.icon_pack()),
            )
        };
        let mut icon_packs = vec![(None, IconPack::Dark), (None, IconPack::Light)];
        icon_packs.extend(
            installed
                .into_iter()
                .map(|name| (Some(name), IconPack::Dark)),
        );
        let pack_labels: Vec<SharedString> = icon_packs
            .iter()
            .map(|(name, pack)| match (name, pack) {
                (Some(name), _) => name.clone().into(),
                (None, IconPack::Dark) => "Default dark".into(),
                (None, IconPack::Light) => "Default light".into(),
            })
            .collect();
        let pack_row = icon_packs
            .iter()
            .position(|(name, kind)| match (&pack.0, name) {
                (Some(chosen), Some(name)) => chosen == name,
                (None, None) => *kind == pack.1,
                _ => false,
            });
        let icon_pack = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(pack_labels),
                pack_row.map(IndexPath::new),
                window,
                cx,
            )
        });

        let installed_themes = theme::themes_dir()
            .map(|dir| theme::installed(&dir))
            .unwrap_or_default();
        let current = shell
            .read(cx)
            .appearance
            .theme
            .clone()
            .unwrap_or_else(|| theme::DEFAULT_ID.to_owned());
        let theme_labels: Vec<SharedString> = installed_themes
            .iter()
            .map(|(id, manifest)| {
                if id == theme::DEFAULT_ID {
                    "dark-soft (built-in)".into()
                } else {
                    manifest.name.clone().into()
                }
            })
            .collect();
        let themes: Vec<String> = installed_themes.into_iter().map(|(id, _)| id).collect();
        let theme_row = themes.iter().position(|id| *id == current);
        let theme = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(theme_labels),
                theme_row.map(IndexPath::new),
                window,
                cx,
            )
        });

        let (low, high) = tokens::FONT_SCALE_RANGE;
        let ui_scale = cx.new(|_| {
            SliderState::new()
                .min(low)
                .max(high)
                .step(0.05)
                .default_value(tokens::font_scale())
        });
        let script_font = cx
            .new(|cx| gpui_kit::component::input::InputState::new(window, cx).default_value("13"));

        let subscriptions = vec![
            cx.subscribe(
                &icon_pack,
                |this, state, event: &SelectEvent<Choices>, cx| {
                    let SelectEvent::Confirm(Some(_)) = event else {
                        return;
                    };
                    let Some(row) = state.read(cx).selected_index(cx).map(|index| index.row) else {
                        return;
                    };
                    let Some((name, kind)) = this.appearance.icon_packs.get(row).cloned() else {
                        return;
                    };
                    this.shell.update(cx, |shell, cx| {
                        shell.set_user_icon_pack(name.clone(), cx);
                        if name.is_none() {
                            shell.set_icon_pack(kind, cx);
                        }
                    });
                },
            ),
            cx.subscribe(&theme, |this, state, event: &SelectEvent<Choices>, cx| {
                let SelectEvent::Confirm(Some(_)) = event else {
                    return;
                };
                let row = state.read(cx).selected_index(cx).map(|index| index.row);
                if let Some(id) = row.and_then(|row| this.appearance.themes.get(row).cloned()) {
                    this.shell.update(cx, |shell, cx| shell.pick_theme(&id, cx));
                }
            }),
            cx.subscribe(&ui_scale, |this, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(SliderValue::Single(value)) = event {
                    let scale = (value * 20.).round() / 20.;
                    this.shell
                        .update(cx, |shell, cx| shell.set_font_scale(scale, cx));
                }
            }),
        ];
        (
            AppearanceControls {
                icon_pack,
                icon_packs,
                theme,
                themes,
                ui_scale,
                script_font,
            },
            subscriptions,
        )
    }
}

/// A 30-tall dropdown in the page's own box, `width` wide.
fn select(state: &Entity<SelectState<Choices>>, width: f32) -> impl IntoElement {
    h_flex()
        .w(px(width))
        .h(px(30.))
        .px(px(10.))
        .items_center()
        .border_1()
        .border_color(tokens::border2())
        .rounded(px(6.))
        .bg(tokens::dock())
        .text_size(px(12.))
        .line_height(px(16.))
        .child(
            Select::new(state)
                .appearance(false)
                .with_size(tokens::field_size())
                .w_full()
                .py_0()
                .px_0()
                .icon(
                    Icon::new(IconName::ChevronDown)
                        .size(px(12.))
                        .text_color(tokens::text()),
                )
                .menu_width(px(width)),
        )
}

/// A ghost "Open folder" that opens `folder` (made if missing) in the file
/// manager.
fn open_folder(id: &'static str, folder: Option<std::path::PathBuf>) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .h(px(28.))
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .rounded(px(5.))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens::text2())
        .cursor_pointer()
        .hover(|this| this.bg(tokens::hover()).text_color(tokens::text()))
        .child(icon("folder", 12.))
        .child("Open folder")
        .on_click(move |_, _, cx| {
            if let Some(folder) = &folder {
                let _ = std::fs::create_dir_all(folder);
                cx.open_with_system(folder);
            }
        })
}

impl SettingsWindow {
    pub(super) fn appearance_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Section> {
        let (scale, accent_changed, tools_changed) = {
            let shell = self.shell.read(cx);
            (
                tokens::font_scale(),
                shell.appearance.accent.is_some(),
                !shell.appearance.tools.is_empty(),
            )
        };
        if (self.appearance.ui_scale.read(cx).value().end() - scale).abs() > 1e-3 {
            self.appearance
                .ui_scale
                .update(cx, |state, cx| state.set_value(scale, window, cx));
        }
        let (low, high) = tokens::FONT_SCALE_RANGE;

        let mut accent = Section::new("Accent", Vec::new());
        accent.head = Some(self.accent_card(window, cx));
        if accent_changed {
            let reset: Reset = std::rc::Rc::new(|shell, cx| shell.set_accent(None, cx));
            accent.resets.push(reset);
        }

        let mut theme_section = Section::new(
            "Theme",
            vec![Row::new(
                "Installed themes",
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(select(&self.appearance.theme, 200.))
                    .child(open_folder("open-themes", theme::themes_dir())),
            )
            .describe("JSON files in the themes folder.")],
        );
        theme_section.head = Some(self.theme_cards(cx));

        let config = crate::settings::default_config_dir();
        let interface = Section::new(
            "Interface",
            vec![
                Row::new(
                    "Icon pack",
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(select(&self.appearance.icon_pack, 180.))
                        .child(open_folder(
                            "open-icon-packs",
                            config.as_ref().map(|dir| dir.join("icon_packs")),
                        )),
                )
                .describe("Class icons in the Explorer and Properties."),
                Row::new(
                    "UI scale",
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(ticked_slider(
                            &self.appearance.ui_scale,
                            200.,
                            &[(1. - low) / (high - low)],
                            cx,
                        ))
                        .child(readout(format!("{scale:.2}\u{d7}"))),
                )
                .describe("Everything in the window, text included.")
                .keys(&["Ctrl =", "Ctrl \u{2212}", "Ctrl 0"])
                .changed((scale - 1.).abs() > 1e-3, |shell, cx| {
                    shell.set_font_scale(1., cx)
                }),
                Row::new(
                    "Script font size",
                    number(&self.appearance.script_font, "px", false),
                )
                .describe("The Script Editor only, on top of the UI scale.")
                .soon(),
            ],
        );

        let tools = TOOLS.map(|(key, name)| {
            let color = theme::color(&format!("tool_{key}"));
            h_flex()
                .id(SharedString::from(format!("tool-{key}")))
                .h(px(34.))
                .gap(px(8.))
                .pl(px(6.))
                .pr(px(10.))
                .items_center()
                .border_1()
                .border_color(tokens::border())
                .rounded(px(6.))
                .bg(tokens::dock())
                .cursor_pointer()
                .hover(|this| this.bg(tokens::hover()))
                .child(
                    div()
                        .flex_none()
                        .size(px(22.))
                        .rounded(px(5.))
                        .bg(Rgba { a: 0.12, ..color })
                        .border(px(1.5))
                        .border_color(color),
                )
                .child(div().text_size(px(12.)).line_height(px(16.)).child(name))
                .child(
                    kit::mono(10.5, 14.)
                        .text_color(tokens::text3())
                        .child(crate::accent::hex(color)),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    let mouse = window.mouse_position();
                    let right = window.viewport_size().width - px(40.);
                    let anchor = point(
                        (mouse.x - px(140.)).min(right - px(280.)),
                        mouse.y + px(14.),
                    );
                    this.open_picker(Target::Tool(key), color, anchor, window, cx);
                }))
        });
        let mut tool_row = Row::new(
            "Tool colours",
            kit::header_button(
                "reset-tools",
                "Reset all",
                tools_changed.then_some(self.set(|shell, cx| shell.reset_tool_colors(cx))),
            ),
        )
        .describe(
            "Each transform tool\u{2019}s pastel on its ribbon button and viewport handles. \
             Picked colours must stay 3:1 on the ribbon.",
        )
        .below(div().grid().grid_cols(4).gap(px(8.)).children(tools));
        if tools_changed {
            tool_row = tool_row.changed(true, |shell, cx| shell.reset_tool_colors(cx));
        }
        let transform = Section::new("Transform tools", vec![tool_row]);

        vec![accent, theme_section, interface, transform]
    }
}
