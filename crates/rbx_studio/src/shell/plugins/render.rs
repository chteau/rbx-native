//! Renders plugin widgets off the editor thread with a second
//! `rbx_viewer::Headless` built from each widget's own small DOM
//! (`PluginHost::widget_dom`), created on first use.

use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use rbx_dom::{Ref, WeakDom};
use rbx_lua::plugin::UiId;
use rbx_viewer::{GuiBox, Headless};

/// One finished render.
#[derive(Clone)]
pub(crate) struct Drawn {
    pub(crate) revision: u64,
    pub(crate) size: (u32, u32),
    /// RGBA8, straight alpha, as a GPUI image.
    pub(crate) image: std::sync::Arc<gpui_kit::RenderImage>,
    /// Paint order; `boxes[0]` is the widget's own frame.
    pub(crate) boxes: Vec<GuiBox>,
    /// Referents that take the pointer instead of letting it through.
    pub(crate) sinks: HashSet<Ref>,
}

struct Job {
    widget: UiId,
    revision: u64,
    dom: WeakDom,
    root: Ref,
    size: (u32, u32),
    backdrop: [f32; 3],
    transparent: bool,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

/// The render thread's handle.
pub(crate) struct Renderer {
    queue: Shared,
    done: Receiver<(UiId, Drawn)>,
}

impl Renderer {
    pub(crate) fn new() -> Self {
        let queue: Shared = Arc::default();
        let (sender, done) = mpsc::channel();
        let worker = queue.clone();
        let spawned = thread::Builder::new()
            .name("plugin-widgets".into())
            .spawn(move || run(&worker, &sender));
        if let Err(err) = spawned {
            eprintln!("rbxstudio: plugin widget renderer: {err}");
        }
        Renderer { queue, done }
    }

    /// Queues a render (`backdrop` = the panel's background, encoded sRGB);
    /// a newer request for the same widget replaces a queued one.
    pub(crate) fn request(
        &self,
        widget: UiId,
        revision: u64,
        dom: WeakDom,
        size: (u32, u32),
        backdrop: [f32; 3],
        transparent: bool,
    ) {
        // `PluginHost::widget_dom` roots the copy at a `ScreenGui` carrying
        // the widget's own referent.
        let Some(root) = dom.root_refs().first().copied() else {
            return;
        };
        let job = Job {
            widget,
            revision,
            dom,
            root,
            size,
            backdrop,
            transparent,
        };
        let (lock, wake) = &*self.queue;
        let Ok(mut queue) = lock.lock() else {
            return;
        };
        queue.jobs.retain(|queued| queued.widget != widget);
        queue.jobs.push_back(job);
        wake.notify_one();
    }

    /// Renders finished since the last call.
    pub(crate) fn finished(&self) -> Vec<(UiId, Drawn)> {
        self.done.try_iter().collect()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        if let Ok(mut queue) = lock.lock() {
            queue.closed = true;
        }
        wake.notify_one();
    }
}

/// A viewer refuses a place with nothing 3D in it (it frames its camera on
/// the parts), and a widget's DOM has only GUI: one invisible part gives it
/// a scene that `render_gui` never draws anyway.
fn with_stand_in_part(dom: &mut WeakDom) {
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "StandIn", Some(workspace));
    let origin = rbx_dom::Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let one = rbx_dom::Vector3Data {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    let frame = rbx_dom::CFrameData {
        position: origin,
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };
    let _ = dom.set_property(part, "size", rbx_dom::Variant::Vector3(one));
    let _ = dom.set_property(part, "CFrame", rbx_dom::Variant::CFrame(frame));
    let _ = dom.set_property(part, "Transparency", rbx_dom::Variant::Float32(1.0));
}

/// Recovers straight-alpha RGBA from the same frame drawn over black and
/// over white: a pixel's alpha is how little the backdrop shows through.
fn matte(black: &[u8], white: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(black.len());
    for (b, w) in black
        .as_chunks::<4>()
        .0
        .iter()
        .zip(white.as_chunks::<4>().0.iter())
    {
        let spread = (0..3)
            .map(|i| (f32::from(w[i]) - f32::from(b[i])) / 255.0)
            .fold(0.0_f32, f32::max);
        let alpha = (1.0 - spread).clamp(0.0, 1.0);
        for &channel in &b[..3] {
            let value = if alpha > 0.0 {
                (f32::from(channel) / alpha).round().clamp(0.0, 255.0)
            } else {
                0.0
            };
            out.push(value as u8);
        }
        out.push((alpha * 255.0).round() as u8);
    }
    out
}

/// Referents that take the pointer: buttons, text boxes, scrolling frames
/// and anything marked `Active`.
fn sinks(dom: &WeakDom) -> HashSet<Ref> {
    let mut found = HashSet::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        let active = matches!(
            instance.properties().get("Active"),
            Some(rbx_dom::Variant::Bool(true))
        );
        let class = instance.class();
        if active || ["TextButton", "ImageButton", "TextBox", "ScrollingFrame"].contains(&class) {
            found.insert(referent);
        }
        stack.extend_from_slice(instance.children());
    }
    found
}

fn run(queue: &Shared, done: &Sender<(UiId, Drawn)>) {
    let mut headless: Option<Headless> = None;
    loop {
        let job = {
            let (lock, wake) = &**queue;
            let Ok(mut guard) = lock.lock() else {
                return;
            };
            loop {
                if guard.closed {
                    return;
                }
                if let Some(job) = guard.jobs.pop_front() {
                    break job;
                }
                guard = match wake.wait(guard) {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
            }
        };
        let mut dom = job.dom;
        let sinks = sinks(&dom);
        with_stand_in_part(&mut dom);
        let loaded = match headless.as_mut() {
            Some(viewer) => viewer.reload(&dom),
            None => Headless::from_dom(&dom, true).map(|viewer| {
                headless = Some(viewer);
            }),
        };
        if let Err(err) = loaded {
            eprintln!("rbxstudio: plugin widget: {err}");
            continue;
        }
        let Some(viewer) = headless.as_mut() else {
            continue;
        };
        let size = (job.size.0.max(1), job.size.1.max(1));
        let rendered = if job.transparent {
            viewer
                .render_gui(job.root, size, [0.0; 3])
                .and_then(|black| {
                    let mut white = viewer.render_gui(job.root, size, [1.0; 3])?;
                    white.pixels = matte(&black.pixels, &white.pixels);
                    Ok(white)
                })
        } else {
            viewer.render_gui(job.root, size, job.backdrop)
        };
        match rendered {
            Ok(canvas) => {
                let (width, height) = canvas.size;
                let Some(image) =
                    crate::render_image::to_render_image(canvas.pixels, width, height)
                else {
                    continue;
                };
                let drawn = Drawn {
                    revision: job.revision,
                    size: canvas.size,
                    image,
                    boxes: canvas.boxes,
                    sinks,
                };
                if done.send((job.widget, drawn)).is_err() {
                    return;
                }
            }
            Err(err) => eprintln!("rbxstudio: plugin widget: {err}"),
        }
    }
}
