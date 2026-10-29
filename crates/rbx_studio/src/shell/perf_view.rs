//! The Performance view: the viewport tinted by what each part costs, one
//! of `rbx_viewer::perf`'s five measures at a time. Studio only profiles a
//! running game (the MicroProfiler); this answers "what here is expensive"
//! while the place is still being edited, and never writes to it.
//!
//! The figures come from a thread of its own, started when the view is
//! turned on and stopped and joined when it is turned off. It keeps its own
//! copy of the DOM, brought in line through the same snapshot-and-`Change`
//! log the render thread is sent (see `workspace_view::pump`'s
//! `Command::Changes`), so an edit costs a re-read of the parts it names,
//! not of the place. The first copy goes over the same way, a few
//! milliseconds of instances per frame: cloning a big place whole on the UI
//! thread stalls it for longer than a frame. Assets are
//! read from the local cache only; one still downloading is looked for again
//! every couple of seconds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use gpui_kit::*;
use rbx_assets::{AssetCache, AssetRef};
use rbx_dom::{Change, Ref, Snapshot, Variant, WeakDom};
use rbx_viewer::perf::{Metric, Metrics, Report};

use super::Shell;

/// The beta switch for the Total and Physics measures and the model
/// ranking (`rbx_viewer::perf`'s `models`).
pub(crate) const BETA_ID: &str = "perf-models";

/// How long the thread waits for an edit before looking for assets that had
/// not downloaded yet.
const RETRY: Duration = Duration::from_secs(2);
/// How often the UI side collects a finished report.
const POLL: Duration = Duration::from_millis(100);
/// How often, and for how long at most, the UI side hands over a slice of
/// the first copy.
const COPY_TICK: Duration = Duration::from_millis(16);
const COPY_BUDGET: Duration = Duration::from_millis(4);
/// Instances snapshotted between two looks at the clock.
const COPY_CHUNK: usize = 256;

enum Job {
    /// A slice of the first copy; `true` on the last one.
    Copy(Vec<Snapshot>, bool),
    Changes(Vec<Snapshot>, Vec<Change>),
    Metric(Metric),
}

/// The view while it is on.
pub(super) struct PerfView {
    pub(super) metric: Metric,
    /// The latest figures, `None` until the first report lands.
    pub(super) report: Option<Arc<Report>>,
    /// Closed by `Drop`, which the thread sees at its next look at the queue.
    jobs: Option<Sender<Job>>,
    reports: Receiver<Report>,
    /// Set by `Drop`: from then on every asset read comes back empty, so a
    /// measure still under way finishes in moments and the join is short.
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// The instances the first copy has still to hand over.
    uncopied: Vec<Ref>,
    copying: CopyStats,
}

/// What the first copy cost the UI thread, for the log line it ends with.
#[derive(Default)]
struct CopyStats {
    total: Duration,
    longest: Duration,
    slices: u32,
}

impl PerfView {
    fn spawn(dom: &WeakDom, metric: Metric) -> Self {
        let mut uncopied = Vec::new();
        let mut pending: Vec<Ref> = dom.root_refs().to_vec();
        while let Some(referent) = pending.pop() {
            if let Some(instance) = dom.get(referent) {
                uncopied.push(referent);
                pending.extend_from_slice(instance.children());
            }
        }
        // Handed over from the end: parents first.
        uncopied.reverse();
        let (jobs, inbox) = mpsc::channel();
        let (outbox, reports) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = thread::Builder::new()
            .name("rbxstudio-perf".to_string())
            .spawn(move || run(metric, &inbox, &outbox, &stopped))
            .inspect_err(|err| {
                eprintln!("rbxstudio: no performance view thread could be started: {err}");
            })
            .ok();
        PerfView {
            metric,
            report: None,
            jobs: Some(jobs),
            reports,
            stop,
            thread,
            uncopied,
            copying: CopyStats::default(),
        }
    }

    /// Hands over the next slice of the first copy, as much as fits in
    /// [`COPY_BUDGET`]; `false` once it is all over.
    fn copy_slice(&mut self, dom: &WeakDom) -> bool {
        if self.uncopied.is_empty() {
            return false;
        }
        let started = Instant::now();
        let mut snapshots = Vec::new();
        while !self.uncopied.is_empty() && started.elapsed() < COPY_BUDGET {
            let at = self.uncopied.len().saturating_sub(COPY_CHUNK);
            // A class change is the log's way of saying "this instance,
            // whole": its snapshot is the instance and the parent it hangs
            // under, nothing else.
            let wanted: Vec<Change> = self.uncopied.drain(at..).map(Change::Class).collect();
            snapshots.extend(dom.snapshot(&wanted));
        }
        let done = self.uncopied.is_empty();
        self.send(Job::Copy(snapshots, done));
        let spent = started.elapsed();
        self.copying.total += spent;
        self.copying.longest = self.copying.longest.max(spent);
        self.copying.slices += 1;
        if done {
            eprintln!(
                "rbxstudio: performance view copied the DOM in {} slices, {:.1} ms in all, the longest {:.1} ms",
                self.copying.slices,
                self.copying.total.as_secs_f64() * 1000.0,
                self.copying.longest.as_secs_f64() * 1000.0
            );
        }
        true
    }

    fn send(&self, job: Job) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(job);
        }
    }
}

impl Drop for PerfView {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(mut metric: Metric, inbox: &Receiver<Job>, outbox: &Sender<Report>, stop: &AtomicBool) {
    let cache = AssetCache::new(None).ok();
    let mut fetch = |asset: &AssetRef| match (asset, &cache) {
        _ if stop.load(Ordering::Relaxed) => None,
        (AssetRef::Id(id), Some(cache)) => cache.get_id(*id),
        (AssetRef::Native(path), Some(cache)) => cache.get_native(path),
        _ => None,
    };
    // The first copy, and any edit made while it was still coming over:
    // each snapshot is the whole truth about its instance, so slices and
    // edits can land in any order and the copy still ends in step.
    let mut dom = WeakDom::new();
    loop {
        match inbox.recv() {
            Ok(Job::Copy(snapshots, done)) => {
                dom.mirror(snapshots);
                if done {
                    break;
                }
            }
            Ok(Job::Changes(snapshots, _)) => dom.mirror(snapshots),
            Ok(Job::Metric(next)) => metric = next,
            Err(_) => return,
        }
    }
    let started = Instant::now();
    let mut metrics = Metrics::new(&dom, &mut fetch);
    eprintln!(
        "rbxstudio: performance view measured {} parts in {:.1} ms",
        metrics.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    let mut owed = true;
    loop {
        if owed {
            owed = false;
            if outbox.send(metrics.report(metric)).is_err() {
                return;
            }
        }
        let first = match inbox.recv_timeout(RETRY) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) if !stop.load(Ordering::Relaxed) => {
                owed = metrics.retry(&mut fetch);
                continue;
            }
            Err(_) => return,
        };
        // Everything already queued in one go: a drag sends an edit per
        // mouse move, and one report after the last of them is all that
        // shows.
        for job in std::iter::once(first).chain(inbox.try_iter()) {
            match job {
                Job::Changes(snapshots, changes) => {
                    dom.mirror(snapshots);
                    metrics.patch(&dom, &changes, &mut fetch);
                }
                Job::Metric(next) => metric = next,
                Job::Copy(..) => {}
            }
            owed = true;
        }
    }
}

impl Shell {
    /// Turns the Performance view on with `metric`, switches it to
    /// another, or turns it off with `None`. Edit only: the live game's DOM
    /// shown during Play is not the place being edited.
    pub(super) fn set_perf_metric(&mut self, metric: Option<Metric>, cx: &mut Context<Self>) {
        match (metric, self.perf.as_mut()) {
            (None, _) => {
                if self.perf.take().is_some() {
                    self.viewport.update(cx, |viewport, cx| {
                        viewport.set_heat(Vec::new());
                        viewport.set_heat_readout(None, cx);
                    });
                }
            }
            (Some(metric), Some(perf)) => {
                perf.metric = metric;
                perf.send(Job::Metric(metric));
            }
            (Some(metric), None) => {
                if self.live_shown() {
                    return;
                }
                self.perf = Some(PerfView::spawn(&self.dom, metric));
                cx.spawn(async move |shell, cx| {
                    let mut wait = Duration::ZERO;
                    loop {
                        cx.background_executor().timer(wait).await;
                        match shell.update(cx, |shell, cx| shell.perf_poll(cx)) {
                            Ok(Some(next)) => wait = next,
                            _ => return,
                        }
                    }
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// Hands over the next slice of the first copy, or the latest report,
    /// if one landed, to the viewport and the dock. How long to wait before
    /// the next look; `None` once the view is off, which ends the polling.
    fn perf_poll(&mut self, cx: &mut Context<Self>) -> Option<Duration> {
        if self.live_shown() {
            self.set_perf_metric(None, cx);
        }
        let perf = self.perf.as_mut()?;
        if perf.copy_slice(&self.dom) {
            return Some(COPY_TICK);
        }
        let Some(report) = perf.reports.try_iter().last() else {
            return Some(POLL);
        };
        let bins = report.bins.clone();
        perf.report = Some(Arc::new(report));
        self.viewport
            .update(cx, |viewport, _| viewport.set_heat(bins));
        cx.notify();
        Some(POLL)
    }

    /// Passes one edit on to the Performance view, if it is on.
    pub(super) fn perf_changes(&self, snapshots: &[Snapshot], changes: &[Change]) {
        if let Some(perf) = self.perf.as_ref().filter(|_| !self.live_shown()) {
            perf.send(Job::Changes(snapshots.to_vec(), changes.to_vec()));
        }
    }

    /// The line the viewport shows beside the cursor over `part`.
    pub(super) fn perf_readout(&self, part: Option<Ref>) -> Option<SharedString> {
        let perf = self.perf.as_ref()?;
        let readout = perf.report.as_ref()?.readout(part?)?;
        Some(SharedString::from(format!(
            "{} · {}, {}",
            readout.name,
            perf.metric.format(readout.value),
            readout.detail
        )))
    }

    /// Selects `target`, a part or a model, and puts the camera on the box
    /// round its parts: a click on one of the dock's costliest lists.
    pub(super) fn perf_focus(&mut self, target: Ref, cx: &mut Context<Self>) {
        self.select(target, cx);
        let (mut low, mut high) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
        let mut pending = vec![target];
        while let Some(at) = pending.pop() {
            let Some(instance) = self.dom.get(at) else {
                continue;
            };
            pending.extend_from_slice(instance.children());
            let properties = instance.properties();
            let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
                continue;
            };
            let centre = glam::Vec3::new(cframe.position.x, cframe.position.y, cframe.position.z);
            let half = match properties.get("size") {
                Some(Variant::Vector3(size)) => glam::Vec3::new(size.x, size.y, size.z) * 0.5,
                _ => glam::Vec3::splat(0.5),
            };
            low = low.min(centre - half);
            high = high.max(centre + half);
        }
        if low.x > high.x {
            return;
        }
        let centre = (low + high) * 0.5;
        let reach = ((high - low).length() * 0.5).max(2.0);
        let eye = centre + glam::Vec3::new(-0.55, 0.45, -1.0).normalize() * reach * 3.0;
        self.viewport
            .read(cx)
            .look_at(eye.to_array(), centre.to_array());
    }
}

#[cfg(test)]
mod tests {
    use super::{Arc, Change, Job, Metric, PerfView, WeakDom};

    /// Turning the view on and off over and over leaves no thread behind:
    /// by the time `drop` returns, the thread has ended and let go of its
    /// half of the stop flag.
    #[test]
    fn turning_the_view_off_joins_its_thread() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        dom.new_instance("Part", "Part", Some(workspace));
        for round in 0..50 {
            let mut view = PerfView::spawn(&dom, Metric::Density);
            // Turned off before the copy came over, mid-measure, or after
            // the first report.
            if round % 3 > 0 {
                while view.copy_slice(&dom) {}
            }
            if round % 3 == 2 {
                view.reports.recv().expect("a first report");
            }
            let stop = view.stop.clone();
            drop(view);
            assert_eq!(Arc::strong_count(&stop), 1, "round {round}");
        }
    }

    /// The first copy, handed over in slices with an edit made halfway
    /// through it, ends in step with the editor's DOM: the first report
    /// counts the part the edit added.
    #[test]
    fn a_sliced_copy_with_an_edit_halfway_ends_in_step() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        for _ in 0..600 {
            dom.new_instance("Part", "Part", Some(workspace));
        }
        dom.take_changes();
        let mut view = PerfView::spawn(&dom, Metric::Density);
        // The half with the `Workspace` in it first, then the edit, then
        // the rest.
        let all = std::mem::take(&mut view.uncopied);
        let (rest, first) = all.split_at(all.len() / 2);
        let wanted: Vec<Change> = first.iter().copied().map(Change::Class).collect();
        view.send(Job::Copy(dom.snapshot(&wanted), false));
        let added = dom.new_instance("Part", "Late", Some(workspace));
        let changes = dom.take_changes();
        view.send(Job::Changes(dom.snapshot(&changes), changes));
        view.uncopied = rest.to_vec();
        while view.copy_slice(&dom) {}

        let report = view.reports.recv().expect("a first report");
        assert!(report.readout(added).is_some());
    }
}
