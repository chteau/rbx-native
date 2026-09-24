//! Fetching and decoding a place's assets off the thread that draws it.
//!
//! Resolving one reference is a download, a disk read and a decode in that
//! order: hundreds of milliseconds cold, and still several for an image the
//! cache already holds — the Tier 1 profile put decoding alone at about two
//! thirds of a `marked.rbxl` reload. None of that may happen between two
//! frames, so it happens here instead. A fixed set of worker threads takes
//! requests off one queue, resolves them through [`Source`], and hands the
//! decoded result back over a channel the render loop drains once a tick (see
//! `load::Resident::poll`).
//!
//! Requests carry no ordering and no cancellation: a result for a reference
//! nobody wants any more is filed and ignored by the caller, which costs a
//! decode already paid for and nothing else. Coalescing is the caller's job
//! too — see `load::Resident`, which is what knows whether a reference is
//! already resident or already in flight.

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Condvar, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(test)]
use std::time::Duration;

use rbx_assets::AssetRef;

use crate::assets::{Failure, Image};

#[cfg(target_arch = "wasm32")]
pub(crate) use web::Fetcher;

/// Which decoder a reference is bound for.
///
/// Part of the request rather than read off the reference: nothing stops a
/// place naming one asset as a mesh in one place and as a texture in another,
/// and the two decode to different things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Want {
    Image,
    Mesh,
    Bytes,
}

/// One finished request: the decoded value, or why it could not be — the
/// same [`Failure`] the blocking path already puts against a reference, so
/// the Output dock reads the same either way and a transient one is retried
/// by the next load just the same.
pub(crate) enum Landed {
    Image(AssetRef, Result<Arc<Image>, Failure>),
    Mesh(AssetRef, Result<Arc<rbx_mesh::Mesh>, Failure>),
    Bytes(AssetRef, Result<Vec<u8>, Failure>),
}

impl Landed {
    #[cfg(test)]
    pub(crate) fn reference(&self) -> &AssetRef {
        match self {
            Landed::Image(reference, _)
            | Landed::Mesh(reference, _)
            | Landed::Bytes(reference, _) => reference,
        }
    }
}

/// Where a worker gets its bytes.
///
/// A trait rather than the resolver itself so the state machine above it can
/// be tested against a table in memory: `crate::assets` fills this with the
/// real disk-cache-and-network resolver, and nothing else in the crate needs
/// to know which of the two it has.
pub(crate) trait Source: Send + Sync + 'static {
    fn image(&self, reference: &AssetRef) -> Result<Image, Failure>;
    fn mesh(&self, reference: &AssetRef) -> Result<rbx_mesh::Mesh, Failure>;
    fn bytes(&self, reference: &AssetRef) -> Result<Vec<u8>, Failure>;
}

#[cfg(not(target_arch = "wasm32"))]
/// The work every worker shares. `None` once the [`Fetcher`] is gone, which
/// is the only stop signal a worker waiting on the condvar can be given.
struct Queue {
    waiting: Mutex<Option<VecDeque<(Want, AssetRef)>>>,
    ready: Condvar,
}

/// A pool of asset workers and the channel their results come back on.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Fetcher {
    queue: Arc<Queue>,
    landed: Receiver<Landed>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Fetcher {
    /// Starts `workers` threads against `source`.
    ///
    /// A thread that will not start is reported and skipped rather than
    /// failing the load: one worker resolves the same assets as six, slower.
    /// With none at all the queue simply never drains, and every asset stays
    /// on its fallback — which is exactly what a machine with no asset
    /// resolver already draws.
    pub(crate) fn new(source: Arc<dyn Source>, workers: usize) -> Self {
        let queue = Arc::new(Queue {
            waiting: Mutex::new(Some(VecDeque::new())),
            ready: Condvar::new(),
        });
        let (done, landed) = mpsc::channel();

        for index in 0..workers.max(1) {
            let queue = Arc::clone(&queue);
            let source = Arc::clone(&source);
            let done = done.clone();
            let started = thread::Builder::new()
                .name(format!("rbxview-assets-{index}"))
                .spawn(move || work(&queue, source.as_ref(), &done));
            if started.is_err() {
                eprintln!("rbxview: an asset worker could not be started");
            }
        }

        Fetcher { queue, landed }
    }

    /// Queues one reference. Costs a lock and a `notify_one`, so a caller that
    /// has already checked the reference is neither resident nor in flight can
    /// do this from the render thread without thinking about it.
    pub(crate) fn request(&self, want: Want, reference: AssetRef) {
        {
            let mut waiting = self.queue.waiting.lock().unwrap_or_else(|e| e.into_inner());
            let Some(pending) = waiting.as_mut() else {
                return;
            };
            pending.push_back((want, reference));
        }
        self.queue.ready.notify_one();
    }

    /// Everything finished since the last call, without ever blocking.
    pub(crate) fn drain(&self) -> Vec<Landed> {
        self.landed.try_iter().collect()
    }

    /// The next result, waiting up to `timeout` for it — for a caller that has
    /// nothing to draw meanwhile (the tests, and nothing on the render path).
    #[cfg(test)]
    pub(crate) fn wait(&self, timeout: Duration) -> Option<Landed> {
        self.landed.recv_timeout(timeout).ok()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Fetcher {
    /// Closes the queue and leaves the workers to notice.
    ///
    /// Deliberately not joined, unlike the render thread `rbxstudio` owns: a
    /// worker part-way through a download would hold the join for as long as
    /// the network takes, and what is being torn down is a viewport somebody
    /// has just closed. Nothing a worker still holds touches a GPU — it has a
    /// `Source` and a `Sender` whose receiver is about to go — so its last
    /// `send` fails and it exits on its own.
    fn drop(&mut self) {
        *self.queue.waiting.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.queue.ready.notify_all();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn work(queue: &Queue, source: &dyn Source, done: &Sender<Landed>) {
    while let Some((want, reference)) = next(queue) {
        let landed = match want {
            Want::Image => Landed::Image(reference.clone(), source.image(&reference).map(Arc::new)),
            Want::Mesh => Landed::Mesh(reference.clone(), source.mesh(&reference).map(Arc::new)),
            Want::Bytes => Landed::Bytes(reference.clone(), source.bytes(&reference)),
        };
        // The receiver is gone: the place this was being fetched for is, too.
        if done.send(landed).is_err() {
            return;
        }
    }
}

/// The next request, waiting for one if the queue is empty; `None` once the
/// queue is closed, which is this worker's cue to stop.
#[cfg(not(target_arch = "wasm32"))]
fn next(queue: &Queue) -> Option<(Want, AssetRef)> {
    let mut waiting = queue.waiting.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let pending = waiting.as_mut()?;
        if let Some(request) = pending.pop_front() {
            return Some(request);
        }
        waiting = queue.ready.wait(waiting).unwrap_or_else(|e| e.into_inner());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A source that decodes `Id(n)` into an `n`-pixel-wide image and fails
    /// every odd id — the asset's own fault, so no load retries it — counting
    /// what it was actually asked to resolve. No cache directory, no network,
    /// no fixture on disk.
    #[derive(Default)]
    pub(crate) struct Counted {
        pub(crate) resolved: AtomicUsize,
    }

    impl Counted {
        fn answer<T>(
            &self,
            reference: &AssetRef,
            value: impl FnOnce(u64) -> T,
        ) -> Result<T, Failure> {
            self.resolved.fetch_add(1, Ordering::Relaxed);
            let AssetRef::Id(id) = reference else {
                return Err(permanent(&format!("{reference:?}: not an id")));
            };
            if id % 2 == 0 {
                Ok(value(*id))
            } else {
                Err(permanent(&format!("asset {id}: odd")))
            }
        }
    }

    fn permanent(warning: &str) -> Failure {
        Failure {
            warning: warning.to_string(),
            transient: false,
        }
    }

    impl Source for Arc<Counted> {
        fn image(&self, reference: &AssetRef) -> Result<Image, Failure> {
            self.answer(reference, |id| Image {
                width: id as u32,
                height: 1,
                pixels: vec![255; (id as usize) * 4],
            })
        }

        fn mesh(&self, _reference: &AssetRef) -> Result<rbx_mesh::Mesh, Failure> {
            Err(permanent("no meshes here"))
        }

        fn bytes(&self, reference: &AssetRef) -> Result<Vec<u8>, Failure> {
            self.answer(reference, |id| vec![id as u8])
        }
    }

    const PATIENCE: Duration = Duration::from_secs(5);

    #[test]
    fn a_request_comes_back_decoded() {
        let source = Arc::new(Counted::default());
        let fetcher = Fetcher::new(Arc::new(Arc::clone(&source)), 2);

        fetcher.request(Want::Image, AssetRef::Id(4));

        let landed = fetcher.wait(PATIENCE).expect("the worker should answer");
        let Landed::Image(reference, Ok(image)) = landed else {
            panic!("expected a decoded image");
        };
        assert_eq!(reference, AssetRef::Id(4));
        assert_eq!(image.width, 4);
    }

    #[test]
    fn a_failure_comes_back_as_its_warning() {
        let source = Arc::new(Counted::default());
        let fetcher = Fetcher::new(Arc::new(Arc::clone(&source)), 1);

        fetcher.request(Want::Bytes, AssetRef::Id(3));

        let Some(Landed::Bytes(_, Err(failure))) = fetcher.wait(PATIENCE) else {
            panic!("expected a warning");
        };
        assert_eq!(failure.warning, "asset 3: odd");
    }

    // Every worker has to be reachable: a pool whose queue only ever wakes one
    // thread would still answer, just one asset at a time.
    #[test]
    fn every_queued_reference_is_answered_once() {
        let source = Arc::new(Counted::default());
        let fetcher = Fetcher::new(Arc::new(Arc::clone(&source)), 4);

        for id in [2, 4, 6, 8, 10, 12] {
            fetcher.request(Want::Image, AssetRef::Id(id));
        }

        let mut seen = Vec::new();
        while seen.len() < 6 {
            let landed = fetcher.wait(PATIENCE).expect("all six should answer");
            seen.push(landed.reference().clone());
        }
        seen.sort_by_key(|reference| match reference {
            AssetRef::Id(id) => *id,
            _ => 0,
        });
        assert_eq!(
            seen,
            [2, 4, 6, 8, 10, 12].map(AssetRef::Id).to_vec(),
            "every reference answered exactly once"
        );
        assert_eq!(source.resolved.load(Ordering::Relaxed), 6);
    }

    // Dropping the fetcher must not leave a worker parked on the condvar for
    // the life of the process; there is no join to prove it, so the test
    // proves the queue is closed instead.
    #[test]
    fn dropping_the_fetcher_closes_the_queue() {
        let source = Arc::new(Counted::default());
        let fetcher = Fetcher::new(Arc::new(Arc::clone(&source)), 2);
        let queue = Arc::clone(&fetcher.queue);

        drop(fetcher);

        assert!(queue
            .waiting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }
}
