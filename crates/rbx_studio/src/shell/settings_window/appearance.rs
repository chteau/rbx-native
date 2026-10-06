//! The Appearance page: the accent (presets, a custom colour and its
//! guard), the theme, the interface's icon pack and scale, and the
//! transform tools' colours.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, Icon, IndexPath, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::settings::{SCRIPT_FONT_SIZE, VIEWPORT_FONT_SIZE};
use crate::theme;
use crate::tokens;

use super::super::toolbar::snap::NumberField;
use super::dragger::{committed, number};
use super::kit::{
    self, ghost_icon, icon, readout, secondary_button, ticked_slider, Reset, Row, Section,
};
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

/// Where Install from GitHub is. The shell keeps it, since the install
/// outlives this window.
pub(in crate::shell) enum Install {
    Idle,
    Running,
    Failed(SharedString),
}

/// The Appearance page's controls that keep state: the two dropdowns, the
/// UI scale slider and Install from GitHub.
pub(super) struct AppearanceControls {
    icon_pack: Entity<SelectState<Choices>>,
    /// What each row of the icon pack dropdown picks.
    icon_packs: Vec<(Option<String>, IconPack)>,
    theme: Entity<SelectState<Choices>>,
    themes: Vec<String>,
    ui_scale: Entity<SliderState>,
    /// The Script font size field, in px at 1x.
    pub(super) script_font: NumberField,
    /// The Viewport font size field, in px at 1x.
    pub(super) viewport_font: NumberField,
    /// The repository link Install from GitHub takes.
    link: Entity<InputState>,
    /// Whether the shell's install was running when last seen, to clear
    /// the link and relist the themes once it ends.
    installing: bool,
}

/// The installed themes' ids, their dropdown labels, and the row of
/// `current`: what the theme dropdown lists, read afresh from the folder.
fn theme_choices(current: &str) -> (Vec<String>, Vec<SharedString>, Option<IndexPath>) {
    let installed = theme::themes_dir()
        .map(|dir| theme::installed(&dir))
        .unwrap_or_default();
    let labels = installed
        .iter()
        .map(|(id, manifest)| {
            if theme::is_reserved(id) {
                format!("{} (built-in)", manifest.name).into()
            } else {
                manifest.name.clone().into()
            }
        })
        .collect();
    let ids: Vec<String> = installed.into_iter().map(|(id, _)| id).collect();
    let row = ids.iter().position(|id| id == current).map(IndexPath::new);
    (ids, labels, row)
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

        let current = shell.read(cx).appearance.theme.clone();
        let installing = matches!(shell.read(cx).theme_install, Install::Running);
        let (themes, theme_labels, theme_row) =
            theme_choices(current.as_deref().unwrap_or(theme::DEFAULT_ID));
        let theme =
            cx.new(|cx| SelectState::new(SearchableVec::new(theme_labels), theme_row, window, cx));

        let (low, high) = tokens::FONT_SCALE_RANGE;
        let ui_scale = cx.new(|_| {
            SliderState::new()
                .min(low)
                .max(high)
                .step(0.05)
                .default_value(tokens::font_scale())
        });
        let size = shell.read(cx).script_font_size();
        // On Enter or leaving the field, clamped like a hand-edited
        // settings.json: `80` is the largest size, not the `8` typed on the way.
        let (script_font, typed) = NumberField::new(size, window, cx, |this, text, cx| {
            let range = crate::settings::SCRIPT_FONT_SIZE_RANGE;
            if let Some(size) = committed(text, range, true) {
                this.shell
                    .update(cx, |shell, cx| shell.set_script_font_size(size, cx));
            }
            this.shell.read(cx).script_font_size()
        });
        let (viewport_font, viewport_typed) = NumberField::new(
            tokens::viewport_font_size(),
            window,
            cx,
            |this, text, cx| {
                let range = crate::settings::VIEWPORT_FONT_SIZE_RANGE;
                if let Some(size) = committed(text, range, true) {
                    this.shell
                        .update(cx, |shell, cx| shell.set_viewport_font_size(size, cx));
                }
                tokens::viewport_font_size()
            },
        );
        let link = cx.new(|cx| InputState::new(window, cx).placeholder("github.com/owner/repo"));

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
            typed,
            viewport_typed,
            cx.subscribe_in(&link, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.install_theme(cx);
                }
            }),
            cx.observe_in(shell, window, |this, shell, window, cx| {
                let install = &shell.read(cx).theme_install;
                let running = matches!(install, Install::Running);
                let done = this.appearance.installing && !running;
                let succeeded = matches!(install, Install::Idle);
                this.appearance.installing = running;
                if done {
                    if succeeded {
                        this.appearance
                            .link
                            .update(cx, |state, cx| state.set_value("", window, cx));
                    }
                    this.refresh_themes(window, cx);
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
                viewport_font,
                link,
                installing,
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
    /// Re-reads the themes folder into the dropdown, after an install or
    /// an uninstall.
    fn refresh_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.shell.read(cx).appearance.theme.clone();
        let (themes, labels, row) = theme_choices(current.as_deref().unwrap_or(theme::DEFAULT_ID));
        self.appearance.themes = themes;
        self.appearance.theme.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(labels), window, cx);
            state.set_selected_index(row, window, cx);
        });
    }

    /// Hands the link to the shell's Install from GitHub.
    fn install_theme(&mut self, cx: &mut Context<Self>) {
        let link = self.appearance.link.read(cx).value().to_string();
        self.shell
            .update(cx, |shell, cx| shell.install_theme(link, cx));
    }

    pub(super) fn appearance_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Section> {
        let (scale, script_font, accent_changed, tools_changed) = {
            let shell = self.shell.read(cx);
            (
                tokens::font_scale(),
                shell.script_font_size(),
                shell.appearance.accent.is_some(),
                !shell.appearance.tools.is_empty(),
            )
        };
        if (self.appearance.ui_scale.read(cx).value().end() - scale).abs() > 1e-3 {
            self.appearance
                .ui_scale
                .update(cx, |state, cx| state.set_value(scale, window, cx));
        }
        let script_font_focused = (self.appearance.script_font.input.read(cx))
            .focus_handle(cx)
            .is_focused(window);
        let viewport_font = tokens::viewport_font_size();
        let viewport_font_focused = (self.appearance.viewport_font.input.read(cx))
            .focus_handle(cx)
            .is_focused(window);
        // A card click or an edited appearance.json changes the theme
        // without going through the dropdown.
        let current = self.shell.read(cx).appearance.theme.clone();
        let current = current.as_deref().unwrap_or(theme::DEFAULT_ID);
        let row = self
            .appearance
            .themes
            .iter()
            .position(|id| id == current)
            .map(IndexPath::new);
        if self.appearance.theme.read(cx).selected_index(cx) != row {
            self.appearance
                .theme
                .update(cx, |state, cx| state.set_selected_index(row, window, cx));
        }
        let (low, high) = tokens::FONT_SCALE_RANGE;

        let mut accent = Section::new("Accent", Vec::new());
        accent.head = Some(self.accent_card(window, cx));
        accent.keywords = &["accent", "colour", "color", "primary"];
        if accent_changed {
            let reset: Reset = std::rc::Rc::new(|shell, cx| shell.set_accent(None, cx));
            accent.resets.push(reset);
        }

        let active =
            (self.shell.read(cx).appearance.theme.clone()).filter(|id| !theme::is_reserved(id));
        let running = matches!(self.shell.read(cx).theme_install, Install::Running);
        let mut install_row = Row::new(
            "Install from GitHub",
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    h_flex()
                        .w(px(220.))
                        .h(px(30.))
                        .px(px(10.))
                        .items_center()
                        .border_1()
                        .border_color(tokens::border2())
                        .rounded(px(6.))
                        .bg(tokens::dock())
                        .child(
                            Input::new(&self.appearance.link)
                                .appearance(false)
                                .w_full()
                                .px_0()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(tokens::text()),
                        ),
                )
                .child(if running {
                    secondary_button("install-theme", "loader", "Installing\u{2026}")
                        .text_color(tokens::text3())
                        .cursor_default()
                } else {
                    secondary_button("install-theme", "download", "Install")
                        .on_click(cx.listener(|this, _, _, cx| this.install_theme(cx)))
                }),
        )
        .describe("A theme repository\u{2019}s link. Installing it again updates it.");
        if let Install::Failed(err) = &self.shell.read(cx).theme_install {
            install_row = install_row.below(
                kit::text(11.5, 16.)
                    .text_color(tokens::text_error())
                    .child(err.clone()),
            );
        }
        let mut theme_section = Section::new(
            "Theme",
            vec![
                Row::new(
                    "Installed themes",
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(select(&self.appearance.theme, 200.))
                        .when_some(active, |this, id| {
                            this.child(
                                ghost_icon("uninstall-theme", "trash", "Uninstall").on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.shell
                                            .update(cx, |shell, cx| shell.uninstall_theme(&id, cx));
                                        this.refresh_themes(window, cx);
                                    }),
                                ),
                            )
                        })
                        .child(open_folder("open-themes", theme::themes_dir())),
                )
                .describe("Folders in the themes folder, plus the three built in."),
                install_row,
            ],
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
                    number(
                        &self.appearance.script_font.input,
                        "px",
                        script_font_focused,
                    ),
                )
                .describe("The Script Editor only, 8 to 32, on top of the UI scale.")
                .changed(script_font != SCRIPT_FONT_SIZE, |shell, cx| {
                    shell.set_script_font_size(SCRIPT_FONT_SIZE, cx)
                }),
                Row::new(
                    "Viewport font size",
                    number(
                        &self.appearance.viewport_font.input,
                        "px",
                        viewport_font_focused,
                    ),
                )
                .describe(
                    "Text the editor draws over the 3D view, 8 to 24, on top of the UI scale. \
                     Not the place\u{2019}s own GUIs.",
                )
                .changed(viewport_font != VIEWPORT_FONT_SIZE, |shell, cx| {
                    shell.set_viewport_font_size(VIEWPORT_FONT_SIZE, cx)
                }),
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
