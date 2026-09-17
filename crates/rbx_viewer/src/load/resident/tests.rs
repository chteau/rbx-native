use std::cell::Cell;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use super::*;
use crate::assets::Failure;
use crate::load::fetcher::tests::Counted;
use crate::load::fetcher::Source;

fn failure(warning: &str, transient: bool) -> Failure {
    Failure {
        warning: warning.to_string(),
        transient,
    }
}

/// A loader that decodes `Id(n)` to `n` and fails every odd id — as the
/// machine's fault, the kind a later load retries — counting how many
/// references it was actually asked for.
fn loader(asked: &Cell<usize>) -> impl Fn(&[AssetRef]) -> Keyed<u64> + '_ {
    move |references| {
        asked.set(asked.get() + references.len());
        references
            .iter()
            .map(|reference| {
                let AssetRef::Id(id) = reference else {
                    unreachable!("the tests only ask for ids");
                };
                let result = if id % 2 == 0 {
                    Ok(*id)
                } else {
                    Err(failure(&format!("asset {id}: odd"), true))
                };
                (reference.clone(), result)
            })
            .collect()
    }
}

#[test]
fn a_first_fetch_loads_everything_once() {
    let asked = Cell::new(0);
    let mut table = Table::default();

    let (found, warnings) = table.fetch(
        &[AssetRef::Id(2), AssetRef::Id(3), AssetRef::Id(2)],
        loader(&asked),
    );

    assert_eq!(asked.get(), 2, "a repeated reference is asked for once");
    assert_eq!(found, HashMap::from([(AssetRef::Id(2), 2)]));
    assert_eq!(warnings, vec!["asset 3: odd".to_string()]);
}

// The whole point: a second pass of the same load asks for the same assets
// again and must not decode any of them a second time — nor retry the one
// that failed — while still answering exactly what the first pass answered,
// warning included.
#[test]
fn a_second_fetch_answers_from_memory_warnings_included() {
    let asked = Cell::new(0);
    let mut table = Table::default();
    let references = [AssetRef::Id(2), AssetRef::Id(3)];
    let first = table.fetch(&references, loader(&asked));

    let again = table.fetch(&references, loader(&asked));

    assert_eq!(asked.get(), 2);
    assert_eq!(again, first);
}

#[test]
fn only_a_never_seen_reference_is_loaded_later() {
    let asked = Cell::new(0);
    let mut table = Table::default();
    table.fetch(&[AssetRef::Id(2)], loader(&asked));

    let (found, _) = table.fetch(&[AssetRef::Id(2), AssetRef::Id(4)], loader(&asked));

    assert_eq!(asked.get(), 2);
    assert_eq!(found.len(), 2);
}

// A failure of the machine is worth one more try once it may have changed —
// the next load, not the next ask: the blip is then gone, and the asset
// resolves as if it had never failed.
#[test]
fn a_transient_failure_is_retried_by_the_next_load() {
    let asked = Cell::new(0);
    let mut table = Table::default();
    let (_, warnings) = table.fetch(&[AssetRef::Id(3)], loader(&asked));
    assert_eq!(warnings, vec!["asset 3: odd".to_string()]);
    table.fetch(&[AssetRef::Id(3)], loader(&asked));
    assert_eq!(asked.get(), 1, "the same load never retries");

    table.forget_failures();
    let (found, warnings) = table.fetch(&[AssetRef::Id(3)], |references| {
        asked.set(asked.get() + references.len());
        references
            .iter()
            .map(|reference| (reference.clone(), Ok(30)))
            .collect()
    });

    assert_eq!(asked.get(), 2, "the next load retries exactly once");
    assert_eq!(found, HashMap::from([(AssetRef::Id(3), 30)]));
    assert!(warnings.is_empty());
}

// A failure of the asset — a 404, a file the package does not hold — is
// not: the answer cannot change, and on a real place the ask is the
// expensive part, so the next load answers it from memory like a success,
// warning included.
#[test]
fn a_permanent_failure_is_not_retried_by_the_next_load() {
    let asked = Cell::new(0);
    let mut table: Table<u64> = Table::default();
    let not_found = |references: &[AssetRef]| {
        asked.set(asked.get() + references.len());
        references
            .iter()
            .map(|reference| (reference.clone(), Err(failure("asset 5: not found", false))))
            .collect()
    };
    table.fetch(&[AssetRef::Id(5)], not_found);

    table.forget_failures();
    let (found, warnings) = table.fetch(&[AssetRef::Id(5)], not_found);

    assert_eq!(asked.get(), 1, "never asked again");
    assert!(found.is_empty());
    assert_eq!(warnings, vec!["asset 5: not found".to_string()]);
}

// Forgetting the failures must not cost the successes: those are the decodes
// a reload exists to skip.
#[test]
fn forgetting_failures_keeps_every_success() {
    let asked = Cell::new(0);
    let mut table = Table::default();
    let references = [AssetRef::Id(2), AssetRef::Id(3)];
    table.fetch(&references, loader(&asked));

    table.forget_failures();
    let (found, warnings) = table.fetch(&references, loader(&asked));

    assert_eq!(asked.get(), 3, "only the failed reference is asked again");
    assert_eq!(found, HashMap::from([(AssetRef::Id(2), 2)]));
    assert_eq!(warnings, vec!["asset 3: odd".to_string()]);
}

// What `assets::load_with` answers when no resolver could be built at all:
// the one message against every reference. Every reference is remembered as
// failed (so the next load retries once the machine is fixed), but the dock
// reads the message once.
#[test]
fn one_warning_shared_by_every_reference_is_reported_once() {
    let mut table: Table<u64> = Table::default();
    let no_resolver = |references: &[AssetRef]| {
        references
            .iter()
            .map(|reference| {
                (
                    reference.clone(),
                    Err(failure("rbxview: no things (boom)", true)),
                )
            })
            .collect()
    };

    let (found, warnings) = table.fetch(
        &[AssetRef::Id(1), AssetRef::Id(2), AssetRef::Id(3)],
        no_resolver,
    );

    assert!(found.is_empty());
    assert_eq!(warnings, vec!["rbxview: no things (boom)".to_string()]);
}

// A loader that answers nothing for a reference leaves it unknown rather
// than remembered as failed: it is asked again on the very next fetch.
#[test]
fn a_reference_the_loader_did_not_answer_is_asked_again() {
    let asked = Cell::new(0);
    let mut table: Table<u64> = Table::default();
    let silent = |references: &[AssetRef]| {
        asked.set(asked.get() + references.len());
        HashMap::new()
    };

    table.fetch(&[AssetRef::Id(2)], silent);
    table.fetch(&[AssetRef::Id(2)], silent);

    assert_eq!(asked.get(), 2);
}

/// A streaming `Resident` over the fetcher's own counting test source: no
/// cache directory, no network, no fixture.
fn streaming() -> (Resident, Arc<Counted>) {
    let source = Arc::new(Counted::default());
    (Resident::fed_by(Arc::new(Arc::clone(&source))), source)
}

const PATIENCE: Duration = Duration::from_secs(5);

#[test]
fn a_streaming_ask_answers_nothing_and_queues_everything() {
    let (mut resident, _source) = streaming();

    let (found, warnings) = resident.images(&[AssetRef::Id(2), AssetRef::Id(4)]);

    assert!(found.is_empty(), "nothing is resident on the first ask");
    assert!(warnings.is_empty(), "a streaming ask warns through poll");
    assert_eq!(resident.in_flight(), 2);
}

#[test]
fn a_landed_asset_answers_the_next_ask_from_memory() {
    let (mut resident, source) = streaming();
    resident.images(&[AssetRef::Id(2)]);

    let settled = resident.settle(PATIENCE);

    assert_eq!(settled.references, vec![AssetRef::Id(2)]);
    assert_eq!(resident.in_flight(), 0);
    let (found, _) = resident.images(&[AssetRef::Id(2)]);
    assert_eq!(found.len(), 1);
    assert_eq!(source.resolved.load(Ordering::Relaxed), 1);
}

// Coalescing: an edit that names the same new asset twice, or twice in a
// row before it lands, must queue exactly one fetch for it.
#[test]
fn a_reference_asked_for_twice_is_fetched_once() {
    let (mut resident, source) = streaming();

    resident.images(&[AssetRef::Id(2), AssetRef::Id(2)]);
    resident.images(&[AssetRef::Id(2)]);
    assert_eq!(resident.in_flight(), 1);
    resident.settle(PATIENCE);

    assert_eq!(source.resolved.load(Ordering::Relaxed), 1);
}

// The same reference wanted as two different kinds is two decodes, and the
// two must not cancel each other out.
#[test]
fn a_reference_wanted_as_two_kinds_is_fetched_once_per_kind() {
    let (mut resident, source) = streaming();

    resident.images(&[AssetRef::Id(2)]);
    resident.bytes(&[AssetRef::Id(2)]);
    assert_eq!(resident.in_flight(), 2);
    resident.settle(PATIENCE);

    assert_eq!(source.resolved.load(Ordering::Relaxed), 2);
    assert_eq!(resident.images(&[AssetRef::Id(2)]).0.len(), 1);
    assert_eq!(resident.bytes(&[AssetRef::Id(2)]).0.len(), 1);
}

#[test]
fn a_failed_fetch_warns_once_and_is_never_retried() {
    let (mut resident, source) = streaming();
    resident.images(&[AssetRef::Id(3)]);

    let settled = resident.settle(PATIENCE);
    assert_eq!(settled.warnings, vec!["asset 3: odd".to_string()]);

    // Every later ask: still absent, still silent, still not fetched again.
    for _ in 0..3 {
        let (found, warnings) = resident.images(&[AssetRef::Id(3)]);
        assert!(found.is_empty());
        assert!(warnings.is_empty());
    }
    assert_eq!(resident.poll().warnings.len(), 0);
    assert_eq!(source.resolved.load(Ordering::Relaxed), 1);
}

#[test]
fn answered_tells_a_failure_apart_from_a_fetch_still_running() {
    let (mut resident, _source) = streaming();
    resident.images(&[AssetRef::Id(2), AssetRef::Id(3)]);

    let waiting = resident.answered(&[AssetRef::Id(2), AssetRef::Id(3)]);
    assert!(waiting.is_empty(), "neither has an answer yet");

    resident.settle(PATIENCE);
    let answered = resident.answered(&[AssetRef::Id(2), AssetRef::Id(3)]);
    assert!(answered[&AssetRef::Id(2)].is_some());
    assert!(answered[&AssetRef::Id(3)].is_none(), "tried and failed");
}

#[test]
fn forgetting_a_reference_makes_the_next_ask_fetch_it_again() {
    let (mut resident, source) = streaming();
    resident.images(&[AssetRef::Id(2)]);
    resident.settle(PATIENCE);

    resident.forget(&AssetRef::Id(2));
    // The first ask after a forget is the one the reload that stages an edit
    // makes: answered absent, and deliberately not refetched.
    assert!(resident.images(&[AssetRef::Id(2)]).0.is_empty());
    assert_eq!(resident.in_flight(), 0);
    assert_eq!(source.resolved.load(Ordering::Relaxed), 1);

    // The one after it is the edit itself, which does fetch.
    assert!(resident.images(&[AssetRef::Id(2)]).0.is_empty());
    assert_eq!(resident.in_flight(), 1);
    resident.settle(PATIENCE);

    assert_eq!(source.resolved.load(Ordering::Relaxed), 2);
    assert_eq!(resident.images(&[AssetRef::Id(2)]).0.len(), 1);
}

/// A source that fails a reference the way a rate limit does — transiently —
/// for its first `flaky` asks and then answers it, counting every ask.
struct Flaky {
    flaky: usize,
    asked: Mutex<HashMap<AssetRef, usize>>,
}

impl Flaky {
    fn new(flaky: usize) -> Arc<Self> {
        Arc::new(Flaky {
            flaky,
            asked: Mutex::new(HashMap::new()),
        })
    }

    fn answer<T>(&self, reference: &AssetRef, value: T) -> Result<T, Failure> {
        let mut asked = self.asked.lock().unwrap();
        let count = asked.entry(reference.clone()).or_insert(0);
        *count += 1;
        if *count > self.flaky {
            Ok(value)
        } else {
            Err(failure("asset 2: rate limited", true))
        }
    }

    fn asks(&self, reference: &AssetRef) -> usize {
        self.asked
            .lock()
            .unwrap()
            .get(reference)
            .copied()
            .unwrap_or(0)
    }
}

impl Source for Arc<Flaky> {
    fn image(&self, reference: &AssetRef) -> Result<Image, Failure> {
        self.answer(
            reference,
            Image {
                width: 1,
                height: 1,
                pixels: vec![255; 4],
            },
        )
    }

    fn mesh(&self, _reference: &AssetRef) -> Result<rbx_mesh::Mesh, Failure> {
        unreachable!("these tests only ask for images")
    }

    fn bytes(&self, _reference: &AssetRef) -> Result<Vec<u8>, Failure> {
        unreachable!("these tests only ask for images")
    }
}

fn flaky(flaky: usize) -> (Resident, Arc<Flaky>) {
    let source = Flaky::new(flaky);
    (
        Resident::fed_by(Arc::new(Arc::clone(&source))),
        Arc::clone(&source),
    )
}

/// Settles, bringing each pending retry due rather than sleeping it out,
/// until every try this load allows is spent.
///
/// Not [`Resident::settle`]: that waits for `in_flight` to drain, which a
/// reference parked on its [`RETRY_DELAY`] never does on its own.
fn settle_retrying(resident: &mut Resident) -> Vec<String> {
    let deadline = Instant::now() + PATIENCE;
    let mut warnings = Vec::new();
    while resident.in_flight() > 0 && Instant::now() < deadline {
        resident.hurry_retries();
        warnings.extend(resident.poll().warnings);
    }
    warnings
}

// The bug this is here for: a burst of keyed asset requests trips Open
// Cloud's per-minute limit, some images come back rate-limited, and a
// viewport that filed those as failures left them blank until the next
// reload.
#[test]
fn a_rate_limited_image_is_asked_about_again_and_lands() {
    let (mut resident, source) = flaky(1);
    resident.images(&[AssetRef::Id(2)]);

    let warnings = settle_retrying(&mut resident);

    assert!(
        warnings.is_empty(),
        "a retry that works says nothing: {warnings:?}"
    );
    assert_eq!(source.asks(&AssetRef::Id(2)), 2);
    assert_eq!(resident.images(&[AssetRef::Id(2)]).0.len(), 1);
    assert_eq!(resident.in_flight(), 0);
}

// While a retry is pending the reference is still coming, not failed: a
// scene re-resolved in between must neither queue a second fetch for it nor
// be told it will never arrive.
#[test]
fn a_reference_waiting_on_a_retry_is_neither_refetched_nor_reported_failed() {
    let (mut resident, source) = flaky(1);
    resident.images(&[AssetRef::Id(2)]);
    // Poll, without hurrying, until the first ask has failed into the retry
    // queue: `in_flight` counts both, so it is the source that says which.
    while source.asks(&AssetRef::Id(2)) == 0 {
        resident.poll();
    }
    resident.poll();

    assert_eq!(resident.in_flight(), 1, "still outstanding, as a retry");
    assert!(!resident.image_failed(&AssetRef::Id(2)));
    assert!(resident.answered(&[AssetRef::Id(2)]).is_empty());
    assert!(resident.images(&[AssetRef::Id(2)]).0.is_empty());
    assert_eq!(source.asks(&AssetRef::Id(2)), 1, "no second fetch queued");
}

// The cap: an endpoint that stays down must not have the viewport asking
// about it every few seconds for the life of the session.
#[test]
fn a_reference_that_keeps_failing_gives_up_after_the_retry_cap() {
    let (mut resident, source) = flaky(usize::MAX);
    resident.images(&[AssetRef::Id(2)]);

    let warnings = settle_retrying(&mut resident);

    assert_eq!(warnings, vec!["asset 2: rate limited".to_string()]);
    assert_eq!(source.asks(&AssetRef::Id(2)) as u32, RETRIES + 1);
    assert!(resident.image_failed(&AssetRef::Id(2)));
    assert_eq!(resident.in_flight(), 0);
}

// The budget is per load, like the blocking path's one retry: the next load
// gets the full set of tries again.
#[test]
fn the_next_load_restores_the_retry_budget() {
    let (mut resident, source) = flaky(usize::MAX);
    resident.images(&[AssetRef::Id(2)]);
    settle_retrying(&mut resident);
    let spent = source.asks(&AssetRef::Id(2));

    resident.forget_failures();
    resident.images(&[AssetRef::Id(2)]);
    settle_retrying(&mut resident);

    assert_eq!(source.asks(&AssetRef::Id(2)), spent * 2);
}

// A permanent failure keeps its old shape: filed at once, warned about
// once, never asked again.
#[test]
fn a_permanent_failure_is_still_never_retried() {
    let (mut resident, source) = streaming();
    resident.images(&[AssetRef::Id(3)]);

    let warnings = settle_retrying(&mut resident);

    assert_eq!(warnings, vec!["asset 3: odd".to_string()]);
    assert_eq!(source.resolved.load(Ordering::Relaxed), 1);
}
