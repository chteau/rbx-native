//! Avatar › Face Preview: the selected dynamic head with a slider per FACS
//! control, written into its `FaceControls` so the viewport deforms the
//! head live the way Studio does, the 5 validation expressions one click
//! away, and the eye and mouth landmarks drawn over the face.

use std::collections::HashMap;

use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::Root;
use gpui_kit::*;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_ugc::head::{find_landmarks, LandmarkPoints, ACTIONS};
use rbx_ugc::{Finding, Geometry};

use super::super::Shell;
use super::collect::{self, HeadMesh};
use crate::tokens;

mod overlay;
mod view;

const WIDTH: f32 = 760.;
const HEIGHT: f32 = 780.;
const MIN_WIDTH: f32 = 600.;
const MIN_HEIGHT: f32 = 520.;

/// The face values and the head's `CFrame` an overlay was drawn for.
type Drawn = (Vec<(String, f32)>, [f32; 12]);

/// A head read for previewing.
struct Face {
    part: Ref,
    name: String,
    mesh: HeadMesh,
    /// The rest mesh in the part's space, what landmarks project onto.
    geometry: Geometry,
    /// The `FaceControls` properties the mesh has a pose for.
    controls: Vec<String>,
    landmarks: Vec<LandmarkPoints>,
    /// The dynamic head checks for it (see `rbx_ugc::head`).
    findings: Vec<Finding>,
}

enum Load {
    Idle(&'static str),
    Loading,
    Ready(Box<Face>),
    Failed(String),
}

struct FaceWindow {
    shell: Entity<Shell>,
    load: Load,
    /// Bumped by every load, so a late one is dropped.
    generation: u64,
    sliders: HashMap<&'static str, Entity<SliderState>>,
    /// The control whose slider is mid-drag: its first step opened the
    /// undo entry the rest of the drag lands in.
    dragging: Option<&'static str>,
    show_landmarks: bool,
    /// What the overlay was last drawn for: the face values and the head's
    /// `CFrame`.
    drawn: Option<Drawn>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

/// The non-zero `FaceControls` values under `part`, in table order.
fn values(dom: &WeakDom, part: Ref) -> Vec<(String, f32)> {
    let Some(controls) = face_controls(dom, part).and_then(|c| dom.get(c)) else {
        return Vec::new();
    };
    rbx_mesh::CONTROL_NAMES
        .iter()
        .filter_map(|(name, _)| match controls.properties().get(*name) {
            Some(Variant::Float32(v)) if *v != 0.0 => Some((name.to_string(), *v)),
            _ => None,
        })
        .collect()
}

fn face_controls(dom: &WeakDom, part: Ref) -> Option<Ref> {
    dom.get(part)?.children().iter().copied().find(|c| {
        dom.get(*c)
            .is_some_and(|instance| instance.class() == "FaceControls")
    })
}

impl Shell {
    /// Brings Face Preview forward, opening it on the selection if it isn't.
    pub(crate) fn open_face_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = self.ugc_face {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = FaceWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.ugc_face = opened);
        });
    }

    /// Sets `values` on the `FaceControls` under `part` (made if it has
    /// none) and every other control to 0 when `only` is set, as one undo
    /// step when `push`, else into the step a drag opened.
    fn write_face(
        &mut self,
        part: Ref,
        values: &[(&str, f32)],
        only: bool,
        push: bool,
        cx: &mut Context<Self>,
    ) {
        let before = push.then(|| self.history_snapshot());
        let controls = face_controls(&self.dom, part).unwrap_or_else(|| {
            self.dom
                .new_instance("FaceControls", "FaceControls", Some(part))
        });
        let names = rbx_mesh::CONTROL_NAMES.iter().map(|(name, _)| *name);
        let all: Vec<(&str, f32)> = match only {
            true => names
                .map(|name| {
                    let set = values.iter().find(|(n, _)| *n == name);
                    (name, set.map_or(0.0, |(_, v)| *v))
                })
                .collect(),
            false => values.to_vec(),
        };
        for (name, value) in all {
            let _ = self
                .dom
                .set_property(controls, name, Variant::Float32(value.clamp(0.0, 1.0)));
        }
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        match before {
            Some(before) => self.commit_history_step(before, changes),
            None => self.record_history_change(changes),
        }
        cx.notify();
    }
}

impl FaceWindow {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
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
                title: Some("Face Preview".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| FaceWindow::new(shell, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&shell, |view: &mut FaceWindow, _, cx| {
            view.redraw_overlay(cx);
            cx.notify();
        });
        let release = cx.on_release(|view: &mut FaceWindow, cx| {
            view.shell.update(cx, |shell, cx| {
                shell.ugc_face = None;
                shell
                    .viewport
                    .update(cx, |v, _| v.set_face_landmarks(Vec::new()));
            });
        });
        let mut view = FaceWindow {
            shell,
            load: Load::Idle("Select a dynamic head, then press Use Selection."),
            generation: 0,
            sliders: HashMap::new(),
            dragging: None,
            show_landmarks: true,
            drawn: None,
            scroll: ScrollHandle::new(),
            _subscriptions: vec![observe, release],
        };
        view.use_selection(cx);
        view
    }

    /// Previews the selected head: a `MeshPart` or a `Model` with a `Head`.
    fn use_selection(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let plan = {
            let s = self.shell.read(cx);
            s.selected()
                .and_then(|root| collect::head_plan(&s.dom, &s.database, root))
        };
        let Some(plan) = plan else {
            self.load = Load::Idle("Select a dynamic head, then press Use Selection.");
            self.drawn = None;
            self.clear_overlay(cx);
            cx.notify();
            return;
        };
        self.load = Load::Loading;
        cx.notify();
        let task = cx.background_spawn(async move {
            let (head, mesh) = collect::resolve_head(&plan, &collect::fetch_cached);
            let Some((mesh, geometry)) = mesh.zip(head.mesh.clone()) else {
                return Err("The head\u{2019}s mesh could not be loaded.".to_string());
            };
            let landmarks = head
                .cage
                .as_ref()
                .map_or_else(Vec::new, |cage| find_landmarks(cage, &geometry));
            Ok(Face {
                part: plan.part,
                name: head.name.clone(),
                controls: head
                    .facs
                    .as_ref()
                    .map_or_else(Vec::new, |f| f.controls.clone()),
                findings: rbx_ugc::head::check(&head),
                mesh,
                geometry,
                landmarks,
            })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                view.load = match result {
                    Ok(face) => Load::Ready(Box::new(face)),
                    Err(message) => Load::Failed(message),
                };
                view.drawn = None;
                view.redraw_overlay(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn face(&self) -> Option<&Face> {
        match &self.load {
            Load::Ready(face) => Some(face),
            _ => None,
        }
    }

    fn slider(&mut self, control: &'static str, cx: &mut Context<Self>) -> Entity<SliderState> {
        if let Some(state) = self.sliders.get(control) {
            return state.clone();
        }
        let state = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(1.0)
                .step(0.01)
                .default_value(0.0)
        });
        let subscription = cx.subscribe(
            &state,
            move |view, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(SliderValue::Single(v)) => {
                    let push = view.dragging != Some(control);
                    view.dragging = Some(control);
                    view.write(&[(control, *v)], false, push, cx);
                }
                SliderEvent::Release(_) => view.dragging = None,
                SliderEvent::Change(SliderValue::Range(..)) => {}
            },
        );
        self._subscriptions.push(subscription);
        self.sliders.insert(control, state.clone());
        state
    }

    fn write(&mut self, values: &[(&str, f32)], only: bool, push: bool, cx: &mut Context<Self>) {
        let Some(part) = self.face().map(|f| f.part) else {
            return;
        };
        self.shell.update(cx, |shell, cx| {
            shell.write_face(part, values, only, push, cx)
        });
    }

    /// One of the page's 5 expressions, every other control back at 0.
    fn expression(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let values: Vec<(&str, f32)> = index
            .and_then(|i| ACTIONS.get(i))
            .map_or_else(Vec::new, |a| a.controls.to_vec());
        self.write(&values, true, true, cx);
    }

    fn toggle_landmarks(&mut self, cx: &mut Context<Self>) {
        self.show_landmarks = !self.show_landmarks;
        self.drawn = None;
        self.redraw_overlay(cx);
        cx.notify();
    }

    fn check_item(&mut self, cx: &mut Context<Self>) {
        let Some(part) = self.face().map(|f| f.part) else {
            return;
        };
        self.shell.update(cx, |shell, cx| {
            let Some(inst) = shell.dom.get(part) else {
                return;
            };
            let source = super::ItemSource::Instance {
                root: part,
                name: inst.name().to_string(),
                class: inst.class().to_string(),
            };
            let kind = super::ItemKind {
                asset: rbx_ugc::AssetType::DynamicHead,
                scale: rbx_ugc::BodyScale::Normal,
            };
            shell.open_ugc_validator(Some(source), Some(kind), cx);
        });
    }
}
