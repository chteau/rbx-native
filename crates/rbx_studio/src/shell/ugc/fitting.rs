//! Avatar › Accessory: the Accessory Fitting Tool. One window walks Studio's
//! steps (pick the item, fit it, test it, generate it) over a preview in the
//! place: a test body in `Workspace.AccessoryFittingPreview` wearing a
//! working copy of the item, shown in the main 3D view. The preview is not
//! undo history: no step keeps it, every undo or redo takes it out and the
//! window builds it again. It goes when the window closes or the item is
//! generated; Generate writes the real `Accessory` as one undo step, and
//! undoing that brings the preview back in its place.
//!
//! The item is checked against `rbx_ugc` after every change, through the
//! same check as Check Item. Layered items get inner and outer cages made
//! from the Mannequin's body cages (`rbx_autorig::item_cages`), written as
//! local meshes; uploading such an item sends a glTF holding the mesh and
//! both cages, the form Roblox's importer reads cages from.
//!
//! Scripted launches for screenshots: `RBX_STUDIO_UGC=fit` opens the tool
//! on the selection, `fit:<name>` on the first instance of that name, with
//! `RBX_STUDIO_UGC_TYPE=<type>` picking the asset type as Check Item reads
//! it. Then `RBX_STUDIO_UGC_FIT_STEP=item|fit|test|generate` goes to a step,
//! `RBX_STUDIO_UGC_FIT_BODY=classic|normal|slender|player:<id>|me` picks
//! the test body, `RBX_STUDIO_UGC_FIT_ANIM=idle|walk|run|jump|climb|fall`
//! plays an animation, and `RBX_STUDIO_UGC_FIT_MOVE=x,y,z` /
//! `RBX_STUDIO_UGC_FIT_TURN=x,y,z` set a rigid item's placement.

use std::sync::Arc;

use gpui_kit::component::Root;
use gpui_kit::*;
use rbx_animation::Clip;
use rbx_dom::{Ref, WeakDom};
use rbx_ugc::AssetType;

use super::super::Shell;
use super::check::Check;
use crate::explorer;
use crate::tokens;

mod cage_jobs;
mod cages;
mod export;
mod file;
mod generate;
mod inputs;
mod launch;
mod layers;
mod motion;
mod preview;
mod source;
mod state;
mod sync;
mod view;
mod view_fit;
mod view_item;
mod view_test;
mod widgets;

use inputs::Inputs;
use layers::Layer;
use source::Source;
use state::{Body, Motion, Settings, Step};

const WIDTH: f32 = 500.;
const HEIGHT: f32 = 880.;
const MIN_WIDTH: f32 = 440.;
const MIN_HEIGHT: f32 = 600.;
const TITLE: &str = "Accessory Fitting Tool";

/// Work done off the UI thread.
enum Job<T> {
    Idle,
    Busy,
    Ready(T),
    Failed(String),
}

impl<T> Job<T> {
    fn ready(&self) -> Option<&T> {
        match self {
            Job::Ready(v) => Some(v),
            _ => None,
        }
    }
}

/// The preview's instances, as last built.
struct Live {
    folder: Ref,
    rig: Ref,
    /// What the rig was built from, to tell when it needs building again.
    body: sync::BodyKey,
    /// Instances rebuilt on every change: the working item, the size box,
    /// the cage overlays and the other items worn.
    dressing: Vec<Ref>,
    item: Option<Ref>,
    /// The size box's outline, recoloured by the check.
    outline: Option<Ref>,
}

/// The cages of a layered item: as made, as edited, their files and the
/// template they came from.
struct Cages {
    made: rbx_autorig::ItemCages,
    now: rbx_autorig::ItemCages,
    uris: generate::CageUris,
    template: rbx_autorig::Mesh,
    /// Body parts as the cages were made against, for nudge centres.
    placed: Vec<cages::Placed>,
}

/// A downloaded test avatar.
struct Avatar {
    fetched: Arc<crate::shell::rig::Fetched>,
}

/// The generated accessory, and where its upload comes from.
struct Generated {
    accessory: Ref,
    name: String,
    upload: Job<super::ItemSource>,
}

struct FittingWindow {
    shell: Entity<Shell>,
    step: Step,
    source: Option<Source>,
    /// Why the last pick could not be the item.
    source_error: Option<String>,
    settings: Settings,
    body: Body,
    scales: Option<crate::shell::rig::Scales>,
    /// A slider moved and the body rebuild is waiting for it to settle.
    scale_pending: bool,
    /// The sliders show another body's scales; render puts them right, as
    /// it is the one place with a `Window` to hand.
    scales_stale: bool,
    avatar: Job<Avatar>,
    motion: Option<Motion>,
    playing: bool,
    clip: Job<Arc<Clip>>,
    time: f32,
    layers: Vec<Layer>,
    cages: Job<Cages>,
    show_inner: bool,
    show_outer: bool,
    cage_opacity: f32,
    nudge: view_fit::Nudge,
    live: Option<Live>,
    /// Why the working item could not be worn.
    wear_error: Option<String>,
    /// The test body's joints, read once per body for playback.
    anim_rig: Option<(Ref, rbx_animation::Rig)>,
    check: Check,
    /// Whether the size box is drawn red, to recolour it when the check says
    /// otherwise.
    box_over: bool,
    generated: Option<Generated>,
    /// A generated item an undo took out, for the redo that brings it back.
    undone: Option<Generated>,
    /// `Shell::history_installs` as last followed.
    installs: u64,
    /// The next preview rebuild leaves the camera where it is.
    keep_camera: bool,
    inputs: Inputs,
    scroll: ScrollHandle,
    ticker: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

fn slot(view: &mut FittingWindow) -> &mut Check {
    &mut view.check
}

impl Shell {
    /// Brings the Accessory Fitting Tool forward, opening it if it isn't;
    /// `source` (else the selection) is the item, `asset` its type.
    pub(crate) fn open_ugc_fitting(
        &mut self,
        source: Option<Ref>,
        asset: Option<AssetType>,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self.ugc_fitting {
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
            let opened = FittingWindow::open(shell.clone(), source, asset, cx);
            shell.update(cx, |shell, _| shell.ugc_fitting = opened);
        });
    }

    /// Commits preview edits outside the undo history.
    fn commit_preview(&mut self, tree: bool, cx: &mut Context<Self>) {
        let changes = self.dom.take_changes();
        if changes.is_empty() {
            return;
        }
        if tree {
            self.rebuild_explorer(cx);
        }
        self.reflect_changes(&changes, cx);
        cx.notify();
    }
}

/// [`preview::strip`], for the undo history.
pub(crate) fn strip_preview(dom: &mut WeakDom) -> Vec<rbx_dom::Change> {
    preview::strip(dom)
}

fn workspace(dom: &WeakDom) -> Option<Ref> {
    explorer::find_by_name(dom, "Workspace")
}

impl FittingWindow {
    fn open(
        shell: Entity<Shell>,
        source: Option<Ref>,
        asset: Option<AssetType>,
        cx: &mut App,
    ) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            app_id: Some(crate::APP_ID.to_owned()),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: true,
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some(TITLE.into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| FittingWindow::new(shell, source, asset, window, cx));
            cx.new(|cx| Root::new(crate::window_blur::wrap(view, cx), window, cx))
        })
        .ok()
    }

    fn new(
        shell: Entity<Shell>,
        source: Option<Ref>,
        asset: Option<AssetType>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (inputs, mut subscriptions) = Inputs::new(window, cx);
        // The preview goes with the window, however it closes.
        let owner = shell.clone();
        subscriptions.push(cx.on_release(move |_, cx| {
            owner.update(cx, |shell, cx| {
                if let Some(ws) = workspace(&shell.dom) {
                    preview::clear(&mut shell.dom, ws);
                }
                shell.commit_preview(true, cx);
            });
        }));
        subscriptions.push(cx.observe_self(|view, cx| view.recolour_box(cx)));
        subscriptions.push(cx.observe(&shell, |view, _, cx| view.follow_history(cx)));
        let installs = shell.read(cx).history_installs;
        let mut view = FittingWindow {
            shell,
            step: Step::Item,
            source: None,
            source_error: None,
            settings: Settings::default(),
            body: Body::Preset(rbx_ugc::BodyScale::Classic),
            scales: None,
            scale_pending: false,
            scales_stale: true,
            avatar: Job::Idle,
            motion: None,
            playing: false,
            clip: Job::Idle,
            time: 0.0,
            layers: Vec::new(),
            cages: Job::Idle,
            show_inner: true,
            show_outer: true,
            cage_opacity: 0.6,
            nudge: view_fit::Nudge::default(),
            live: None,
            wear_error: None,
            anim_rig: None,
            check: Check::new(None, super::ItemKind::default()),
            box_over: false,
            generated: None,
            undone: None,
            installs,
            keep_camera: false,
            inputs,
            scroll: ScrollHandle::new(),
            ticker: None,
            _subscriptions: subscriptions,
        };
        if let Some(asset) = asset {
            view.settings.set_mode(state::mode_of(asset));
            view.settings.set_asset(asset);
        }
        let picked = source.or_else(|| view.shell.read(cx).selected());
        if let Some(root) = picked {
            view.pick(root, window, cx);
        }
        view.sync(cx);
        launch::apply(&mut view, window, cx);
        view
    }

    /// Goes to `step` when it can be reached.
    fn go(&mut self, step: Step, cx: &mut Context<Self>) {
        if !step.reachable(self.source.is_some()) || step == self.step {
            return;
        }
        if step != Step::Test {
            self.stop_motion(cx);
        } else {
            self.scales_stale = true;
            if self.layers.is_empty() {
                self.load_layers(cx);
            }
        }
        self.step = step;
        self.scroll.set_offset(point(px(0.), px(0.)));
        if step == Step::Fit && self.settings.layered() && matches!(self.cages, Job::Idle) {
            self.make_cages(cx);
        }
        self.sync(cx);
    }

    /// After an undo or redo: the step took the preview out (see
    /// `Shell::history_copy`), so it is built again where the camera is,
    /// unless the step brought a generated item back.
    fn follow_history(&mut self, cx: &mut Context<Self>) {
        let installs = self.shell.read(cx).history_installs;
        if installs == self.installs {
            return;
        }
        self.installs = installs;
        let dom = &self.shell.read(cx).dom;
        state::follow_generated(&mut self.generated, &mut self.undone, |g| {
            dom.get(g.accessory).is_some()
        });
        self.live = None;
        self.anim_rig = None;
        self.keep_camera = true;
        if self.generated.is_some() {
            self.restart_check(cx);
        } else {
            self.sync(cx);
        }
        cx.notify();
    }

    fn close(window: &mut Window, cx: &mut App) {
        window.defer(cx, |window, _| window.remove_window());
    }
}
