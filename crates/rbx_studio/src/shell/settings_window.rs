//! Studio Settings (`File › Studio Settings…`, `Alt+S`): every preference
//! the editor keeps, on ten pages behind a nav, the way real Studio's own
//! `File > Studio Settings` gathers its preferences into one dialog.
//! Keyboard shortcuts stay a window of their own, as real Studio's
//! `File > Customize Shortcuts` is.
//!
//! **This window holds no copy of any setting.** Each row reads the value
//! off [`Shell`] and writes it through the same setter the View menu, the
//! Viewport dock or the Snap popover calls, so they all show one value and
//! `settings.json` is written by the one path that already writes it.
//! Settings apply as they change; there is no Save.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::Shell;

mod kit;
mod nav;
mod viewport;

use kit::{text, Section};
use nav::Page;

const WIDTH: f32 = 1040.;
const HEIGHT: f32 = 720.;
const MIN_WIDTH: f32 = 760.;
const MIN_HEIGHT: f32 = 560.;

/// `RBX_STUDIO_SETTINGS_PAGE=<page>`: the page the window opens on, for a
/// capture. `RBX_STUDIO_SETTINGS=1` opens the window with the editor.
const PAGE_VARIABLE: &str = "RBX_STUDIO_SETTINGS_PAGE";
pub(super) const OPEN_VARIABLE: &str = "RBX_STUDIO_SETTINGS";
/// `RBX_STUDIO_SETTINGS_ADVANCED=1` opens Viewport › Advanced, and
/// `RBX_STUDIO_SETTINGS_SCROLL=<px>` scrolls the page that far down.
const ADVANCED_VARIABLE: &str = "RBX_STUDIO_SETTINGS_ADVANCED";
const SCROLL_VARIABLE: &str = "RBX_STUDIO_SETTINGS_SCROLL";

pub(crate) struct SettingsWindow {
    shell: Entity<Shell>,
    page: Page,
    search: Entity<InputState>,
    sliders: viewport::Sliders,
    /// Viewport › Advanced's disclosure.
    advanced_open: bool,
    page_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// Brings the Settings window forward, opening it if it isn't.
    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.settings_window {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        // Deferred: opening a window renders it at once, and its first
        // render reads this `Shell`, which is still being updated here.
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = SettingsWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.settings_window = opened);
        });
    }
}

impl SettingsWindow {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: true,
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Settings".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| SettingsWindow::new(shell, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (sliders, mut subscriptions) = viewport::Sliders::new(&shell, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        // Anything that changes a setting elsewhere — the dock, a menu —
        // notifies the shell; this window has nothing of its own to redraw
        // from.
        subscriptions.push(cx.observe(&shell, |_, _, cx| cx.notify()));
        let page = std::env::var(PAGE_VARIABLE)
            .ok()
            .and_then(|name| Page::from_key(&name))
            .unwrap_or(Page::Appearance);
        SettingsWindow {
            shell,
            page,
            search,
            sliders,
            advanced_open: std::env::var(ADVANCED_VARIABLE).is_ok(),
            page_scroll: {
                let scroll = ScrollHandle::new();
                if let Some(y) = std::env::var(SCROLL_VARIABLE)
                    .ok()
                    .and_then(|y| y.parse::<f32>().ok())
                {
                    scroll.set_offset(point(px(0.), px(-y)));
                }
                scroll
            },
            _subscriptions: subscriptions,
        }
    }

    /// Runs `f` on the shell when clicked.
    fn set(
        &self,
        f: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let shell = self.shell.clone();
        move |_, _, cx| shell.update(cx, |shell, cx| f(shell, cx))
    }

    fn sections(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        match self.page {
            Page::Viewport => self.viewport(window, cx),
            _ => Vec::new(),
        }
    }

    fn page_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sections = self.sections(window, cx);
        let resets: Vec<_> = sections
            .iter()
            .flat_map(|section| section.rows.iter().filter_map(|row| row.reset.clone()))
            .collect();
        let shell = self.shell.clone();
        let reset_page = h_flex()
            .id("reset-page")
            .flex_none()
            .h(px(28.))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded(px(5.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(kit::icon("rotate-ccw", 12.))
            .child("Reset page")
            .map(|this| {
                if resets.is_empty() {
                    this.text_color(tokens::text3())
                } else {
                    this.text_color(tokens::text2())
                        .cursor_pointer()
                        .hover(|this| this.bg(tokens::hover()).text_color(tokens::text()))
                        .on_click(move |_, _, cx| {
                            shell.update(cx, |shell, cx| {
                                for reset in &resets {
                                    reset(shell, cx);
                                }
                            })
                        })
                }
            });
        let (title, subtitle) = self.page.heading();
        let shell = self.shell.clone();
        v_flex()
            .id("settings-page")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.page_scroll)
            .child(
                v_flex()
                    .pt(px(26.))
                    .pr(px(40.))
                    .pb(px(40.))
                    .pl(px(36.))
                    .gap(px(22.))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(16.))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(4.))
                                    .child(
                                        text(20., 26.).font_weight(FontWeight::BOLD).child(title),
                                    )
                                    .child(
                                        text(12.5, 18.).text_color(tokens::text2()).child(subtitle),
                                    ),
                            )
                            .child(reset_page),
                    )
                    .children(
                        sections
                            .into_iter()
                            .enumerate()
                            .map(|(i, section)| kit::section(i, section, &shell)),
                    ),
            )
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page_view(window, cx);
        v_flex()
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_size(px(13.))
            .text_color(tokens::text())
            .child(super::chrome::window_topbar(
                "Settings".into(),
                true,
                |window, _| window.remove_window(),
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.nav(cx))
                    .child(page),
            )
    }
}

impl SettingsWindow {
    fn search_field(&self) -> impl IntoElement {
        h_flex()
            .h(px(32.))
            .flex_none()
            .gap(px(8.))
            .pl(px(10.))
            .pr(px(8.))
            .items_center()
            .border_1()
            .border_color(tokens::border())
            .rounded(px(6.))
            .bg(tokens::field_select())
            .text_color(tokens::text3())
            .child(kit::icon("search", 14.))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search)
                        .appearance(false)
                        .px_0()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(tokens::text()),
                ),
            )
            .child(kit::key_hint("Ctrl F"))
    }
}
