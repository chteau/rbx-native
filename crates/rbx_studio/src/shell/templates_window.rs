//! Script Templates (`File › Script Templates…`, the ribbon's Script menu ›
//! "Manage templates…"): the user's starter scripts, listed by class, so
//! adding, renaming or deleting one needs no file manager.
//!
//! **This window holds no copy of the templates.** It reads the list off
//! [`Shell`] every frame, writes through `script_templates`' store, and has
//! `Shell` reload the folder right after — the same reload a hand edit gets
//! from the poll (see `shell::templates_live`). What it keeps is only its
//! own view state: the filter and which row is selected.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::*;

use crate::script_templates::{ScriptTemplates, CLASSES};
use crate::tokens;

use super::Shell;

mod editor;
mod list;
mod pane;
mod status;

const WIDTH: f32 = 1040.;
const HEIGHT: f32 = 720.;
const MIN_WIDTH: f32 = 720.;
const MIN_HEIGHT: f32 = 480.;

/// `RBX_STUDIO_TEMPLATES=1` opens the window with the editor, and
/// `RBX_STUDIO_TEMPLATES_SELECT=<Class>/<name>` selects that row (`Default`
/// for the starter, a skipped file by its file name), for a capture.
pub(super) const OPEN_VARIABLE: &str = "RBX_STUDIO_TEMPLATES";
const SELECT_VARIABLE: &str = "RBX_STUDIO_TEMPLATES_SELECT";

/// One row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Selected {
    /// A class's starter: built-in, or the user's `Default.luau`.
    Starter(&'static str),
    Template {
        class: &'static str,
        name: String,
    },
    Skipped {
        class: &'static str,
        file_name: String,
    },
}

impl Selected {
    /// Whether this row still exists in `templates`.
    fn exists(&self, templates: &ScriptTemplates) -> bool {
        match self {
            Selected::Starter(_) => true,
            Selected::Template { class, name } => templates
                .extras()
                .iter()
                .any(|t| t.class == *class && t.name == *name),
            Selected::Skipped { class, file_name } => templates
                .skipped()
                .iter()
                .any(|s| s.class == *class && s.file_name == *file_name),
        }
    }

    /// `<Class>/<name>`, as [`SELECT_VARIABLE`] spells a row.
    fn parse(spec: &str, templates: &ScriptTemplates) -> Option<Selected> {
        let (class, name) = spec.split_once('/')?;
        let class = *CLASSES.iter().find(|c| **c == class)?;
        let row = if name == crate::script_templates::DEFAULT_STEM {
            Selected::Starter(class)
        } else if name.ends_with(".luau") {
            Selected::Skipped {
                class,
                file_name: name.to_owned(),
            }
        } else {
            Selected::Template {
                class,
                name: name.to_owned(),
            }
        };
        row.exists(templates).then_some(row)
    }
}

/// The first of the user's own templates: what the window opens on. With
/// none, nothing is selected and the empty state shows.
fn first_template(templates: &ScriptTemplates) -> Option<Selected> {
    templates.extras().first().map(|t| Selected::Template {
        class: t.class,
        name: t.name.clone(),
    })
}

pub(crate) struct TemplatesWindow {
    shell: Entity<Shell>,
    filter: Entity<InputState>,
    selected: Option<Selected>,
    /// The selected template's editor; `None` for a skipped file or no
    /// selection.
    editor: Option<editor::TemplateEditor>,
    list_scroll: ScrollHandle,
    /// The window's own focus, so its keys reach it before anything inside
    /// it has been clicked.
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// Brings the Script Templates window forward, opening it if it isn't.
    pub(crate) fn open_script_templates(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.templates_window {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        // Deferred: the window's first render reads this `Shell`, which is
        // still being updated here.
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = TemplatesWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.templates_window = opened);
        });
    }
}

impl TemplatesWindow {
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
                title: Some("Script Templates".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| TemplatesWindow::new(shell, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        let subscriptions = vec![
            // A reload — the poll, or this window's own write — notifies
            // the shell; the list is drawn from it.
            cx.observe_in(&shell, window, |this, _, window, cx| {
                this.keep_selection(cx);
                this.follow_disk(window, cx);
                cx.notify();
            }),
            cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let templates = &shell.read(cx).script_templates;
        let selected = std::env::var(SELECT_VARIABLE)
            .ok()
            .and_then(|spec| Selected::parse(&spec, templates))
            .or_else(|| first_template(templates));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = TemplatesWindow {
            shell,
            filter,
            selected,
            editor: None,
            list_scroll: ScrollHandle::new(),
            focus,
            _subscriptions: subscriptions,
        };
        this.open_editor(window, cx);
        this
    }

    /// A selected row that vanished on disk (deleted or renamed by hand)
    /// falls back to the first template, or to nothing.
    fn keep_selection(&mut self, cx: &App) {
        let templates = &self.shell.read(cx).script_templates;
        if self.selected.as_ref().is_some_and(|s| !s.exists(templates)) {
            self.selected = first_template(templates);
        }
    }

    fn query(&self, cx: &App) -> String {
        self.filter.read(cx).value().trim().to_lowercase()
    }

    fn select(&mut self, row: Selected, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_ref() == Some(&row) {
            return;
        }
        self.selected = Some(row);
        self.open_editor(window, cx);
        cx.notify();
    }
}

impl Render for TemplatesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("templates-window")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens::dock())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Esc clears a typed filter before anything else.
                if event.keystroke.key == "escape" && !this.query(cx).is_empty() {
                    this.filter
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .font_family(tokens::FONT_FAMILY_UI)
            .text_size(px(13.))
            .text_color(tokens::text())
            .child(super::chrome::window_topbar(
                "Script Templates".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.list(window, cx))
                    .child(self.pane(window, cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{first_template, Selected};
    use crate::script_templates::ScriptTemplates;

    fn fixture() -> ScriptTemplates {
        // One directory per call: the tests run in parallel.
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rbx-native-templates-window-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, bytes) in [
            ("LocalScript/Camera Shake.luau", &b"x"[..]),
            ("Script/Notes.luau", &[0xff][..]),
        ] {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        ScriptTemplates::load_from(&dir)
    }

    #[test]
    fn a_row_is_spelled_class_slash_name_and_must_exist() {
        let templates = fixture();
        assert_eq!(
            Selected::parse("LocalScript/Camera Shake", &templates),
            Some(Selected::Template {
                class: "LocalScript",
                name: "Camera Shake".into()
            })
        );
        assert_eq!(
            Selected::parse("Script/Default", &templates),
            Some(Selected::Starter("Script"))
        );
        assert_eq!(
            Selected::parse("Script/Notes.luau", &templates),
            Some(Selected::Skipped {
                class: "Script",
                file_name: "Notes.luau".into()
            })
        );
        assert_eq!(Selected::parse("Script/Gone", &templates), None);
        assert_eq!(Selected::parse("Part/Camera Shake", &templates), None);
    }

    #[test]
    fn the_window_opens_on_the_first_template_or_nothing() {
        assert_eq!(
            first_template(&fixture()),
            Some(Selected::Template {
                class: "LocalScript",
                name: "Camera Shake".into()
            })
        );
        assert_eq!(first_template(&ScriptTemplates::default()), None);
    }
}
