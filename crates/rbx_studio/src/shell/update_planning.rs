//! File › Update Planning… (beta): countdown icons on autopilot. A sidebar
//! of the user's experiences; for the selected one either the planner
//! (current and new icon, update time, timers, the timeline of composites,
//! Start update) or, once started, the steps view (each step's review and
//! state, the "update in" countdown, Cancel). The rules, files and runner
//! are `crate::update_planning`; this is only the window.
//!
//! While it is open it also runs due steps and polls Roblox's review of
//! the uploads; the OS helper covers the time it is closed.
//!
//! `RBX_STUDIO_UPDATE_PLAN_MOCK=1` fills it with drawn experiences and a
//! fake Roblox (see `update_planning::api`); `RBX_STUDIO_ROBLOX=update-planning`
//! opens it with the editor; `RBX_STUDIO_UPDATE_PLAN_FILES=<current>,<new>`
//! stands in for the file picker, which a nested X server has no portal for.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Root;
use gpui_kit::*;
use image::RgbaImage;
use rbx_cloud::Experience;

use crate::tokens;
use crate::update_planning::plan::{Plan, Timer};
use crate::update_planning::{api, mock, store, when};

use super::Shell;

mod run;
mod view;
mod work;

const WIDTH: f32 = 1180.;
const HEIGHT: f32 = 820.;

/// A picked icon: the 512×512 pixels composites are made from, and what
/// the window paints.
#[derive(Clone)]
pub(super) struct Picked {
    pub(super) pixels: RgbaImage,
    pub(super) preview: Arc<RenderImage>,
    /// "From Roblox", or the file's name.
    pub(super) source: String,
}

/// One timeline card: a composite (or the new icon) and when it shows.
pub(super) struct Preview {
    pub(super) label: String,
    /// The timer it shows, `None` for the new icon.
    pub(super) timer: Option<usize>,
    pub(super) minutes_before: Option<u32>,
    pub(super) image: Arc<RenderImage>,
    pub(super) pixels: RgbaImage,
}

pub(crate) struct UpdatePlanningWindow {
    root: Option<PathBuf>,
    experiences: Vec<Experience>,
    thumbs: HashMap<u64, Arc<RenderImage>>,
    selected: Option<u64>,
    /// The selected experience's timers, as saved.
    timers: Vec<Timer>,
    /// Each timer's overlay alone, for its row.
    timer_thumbs: Vec<Option<Arc<RenderImage>>>,
    current: Option<Picked>,
    new_icon: Option<Picked>,
    /// The language the current icon was fetched for; the plan posts to it.
    language: String,
    when: Entity<InputState>,
    timer_label: Entity<InputState>,
    timer_hours: Entity<InputState>,
    previews: Vec<Preview>,
    plans: HashMap<u64, Plan>,
    step_images: HashMap<(u64, String), Arc<RenderImage>>,
    /// The planner shows even though the experience has a plan (a finished
    /// one, after "Plan a new update").
    replan: bool,
    /// The banner under the header: `(is_error, text)`.
    status: Option<(bool, String)>,
    /// What is running in the background, shown with a spinner.
    busy: Option<String>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _ticker: Task<()>,
}

impl Shell {
    /// Brings Update Planning forward, opening it if it isn't.
    pub(crate) fn open_update_planning(&mut self, cx: &mut Context<Self>) {
        if !crate::beta::enabled(crate::update_planning::BETA_ID) {
            return;
        }
        if let Some(existing) = &self.update_planning {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        let linked = crate::home::link_of(&self.path);
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = UpdatePlanningWindow::open(linked, cx);
            shell.update(cx, |shell, _| shell.update_planning = opened);
        });
    }
}

impl UpdatePlanningWindow {
    fn open(linked: Option<crate::home::Link>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: true,
            window_min_size: Some(size(px(960.), px(640.))),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Update Planning".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| UpdatePlanningWindow::new(linked, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(linked: Option<crate::home::Link>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = chrono::Utc::now().timestamp();
        let mut subscriptions = Vec::new();
        let mut line = |placeholder: &'static str, value: String| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            });
            subscriptions.push(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.status = None;
                    this.refresh_previews(cx);
                }
            }));
            input
        };
        let when = line("YYYY-MM-DD HH:MM", when::default_text(now));
        let timer_label = line("Label, e.g. 30 Minutes", String::new());
        let timer_hours = line("Hours before", String::new());
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = UpdatePlanningWindow {
            root: store::root(),
            experiences: experiences(linked),
            thumbs: HashMap::new(),
            selected: None,
            timers: Vec::new(),
            timer_thumbs: Vec::new(),
            current: None,
            new_icon: None,
            language: "en".to_string(),
            when,
            timer_label,
            timer_hours,
            previews: Vec::new(),
            plans: HashMap::new(),
            step_images: HashMap::new(),
            replan: false,
            status: None,
            busy: None,
            focus,
            _subscriptions: subscriptions,
            _ticker: Task::ready(()),
        };
        this.thumbs = this
            .experiences
            .iter()
            .filter_map(|game| Some((game.universe_id, thumb(game.universe_id)?)))
            .collect();
        this.reload_plans();
        let first = this.experiences.first().map(|g| g.universe_id);
        if let Some(id) = first {
            this.select(id, cx);
        }
        this._ticker = this.start_ticker(cx);
        this
    }

    pub(super) fn experience(&self) -> Option<&Experience> {
        let id = self.selected?;
        self.experiences.iter().find(|g| g.universe_id == id)
    }

    /// The selected experience's plan, if one was ever started.
    pub(super) fn plan(&self) -> Option<&Plan> {
        self.plans.get(&self.selected?)
    }
}

/// The mock's games, else Home's cached listing, with the open place's
/// experience first when it is linked.
fn experiences(linked: Option<crate::home::Link>) -> Vec<Experience> {
    if api::mock_enabled() {
        return mock::experiences();
    }
    let mut games = crate::launcher::cached_games();
    if let Some(link) = linked {
        let at = games.iter().position(|g| g.universe_id == link.universe_id);
        let game = match at {
            Some(at) => games.remove(at),
            None => Experience {
                universe_id: link.universe_id,
                root_place_id: link.place_id,
                name: link
                    .name
                    .unwrap_or_else(|| format!("Universe {}", link.universe_id)),
                visibility: rbx_cloud::Visibility::Other(String::new()),
                owner: rbx_cloud::Owner::User(0),
            },
        };
        games.insert(0, game);
    }
    games
}

fn thumb(universe_id: u64) -> Option<Arc<RenderImage>> {
    if api::mock_enabled() {
        return render(&mock::icon(universe_id)?);
    }
    render(
        &image::load_from_memory(&crate::launcher::cached_icon(universe_id)?)
            .ok()?
            .to_rgba8(),
    )
}

/// `image` as GPUI paints it.
pub(super) fn render(image: &RgbaImage) -> Option<Arc<RenderImage>> {
    let (w, h) = image.dimensions();
    crate::render_image::to_render_image(image.as_raw().clone(), w, h)
}
