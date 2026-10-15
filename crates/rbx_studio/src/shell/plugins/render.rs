//! Renders plugin widgets off the editor thread with a second
//! `rbx_viewer::Headless` built from each widget's own small DOM
//! (`PluginHost::widget_dom`), created on first use.

use std::collections::VecDeque;
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
}

struct Job {
    widget: UiId,
    revision: u64,
    dom: WeakDom,
    root: Ref,
    size: (u32, u32),
    backdrop: [f32; 3],
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
        let loaded = match headless.as_mut() {
            Some(viewer) => viewer.reload(&job.dom),
            None => Headless::from_dom(&job.dom, true).map(|viewer| {
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
        match viewer.render_gui(job.root, size, job.backdrop) {
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
                };
                if done.send((job.widget, drawn)).is_err() {
                    return;
                }
            }
            Err(err) => eprintln!("rbxstudio: plugin widget: {err}"),
        }
    }
}
