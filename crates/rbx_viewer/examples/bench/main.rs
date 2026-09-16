//! Reproducible timings for scene reload and rendering, on a real GPU.
//!
//! Run it with `scripts/bench.sh`, or directly:
//! `cargo run --release --example bench -p rbx_viewer -- --help`.
//!
//! # Why an example and not a criterion benchmark
//!
//! Criterion measures a closure it may call thousands of times and reports one
//! distribution per closure. Neither half suits this:
//!
//! - The expensive operations here open a GPU device (`Headless::load` builds
//!   one per call) and a place worth measuring takes the better part of a
//!   second to reload, so criterion's sampling floor turns a single benchmark
//!   into minutes of wall clock for a number a dozen iterations already pin
//!   down.
//! - The number that matters is *two* numbers from one iteration: when the call
//!   returned, and when the first frame it queued was actually readable. A
//!   criterion closure reports one duration, so expressing the pair means two
//!   benchmarks that each redo the work — and then they cannot be subtracted,
//!   because they did not measure the same iteration.
//! - GPU work makes criterion's statistical machinery unreliable rather than
//!   more precise: the outlier classification assumes independent samples, and
//!   consecutive frames on one device share caches, clocks and a thermal budget.
//!
//! So: an explicit warmup, a fixed iteration count, and nearest-rank median/p95
//! (see `stats`). Every operation is timed through `Headless`'s public API only,
//! so this harness runs unchanged against a branch that rewrites the reload path
//! underneath it and the two can be compared by checking out either one.
//!
//! # Completion discipline
//!
//! `Headless::reload`/`patch_instance` return once the GPU work is *queued*, and
//! `render_frame` queues a frame and hands back the one before it. So every
//! phase reports the call's own wall clock and, from the same instant, the time
//! until `take_frame` hands back the pixels of the frame queued after the edit —
//! which is where the driver actually waits for the upload and the draw. The two
//! are labelled separately and never added together.
//!
//! One thing that discipline cannot reach: `Renderer::draw` keeps a per-frame
//! budget for texture uploads and spreads a freshly built scene's remainder over
//! the frames after the first, and the call that finishes that early
//! (`Offscreen::finish_loading`) is crate-private, with no route to it through
//! `Headless`. So "first frame readable" is exactly that — the first frame — and
//! not a fully textured one. The phases that are not about the first frame drain
//! the backlog first (see `measure::drain_upload_budget`), which is also what
//! measures it: on the 16k-instance fixture it is worth roughly a dozen frames
//! at several times their settled cost.

mod args;
mod measure;
mod report;
mod stats;

use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use args::Args;
use report::{Fixture, Outcome, Run};

fn main() -> ExitCode {
    let mut raw = std::env::args();
    raw.next();

    let args = match Args::parse(raw) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{}", Args::usage());
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("{}", Args::usage());
            return ExitCode::FAILURE;
        }
    };

    // A debug number is not a slow version of the release number, it is a
    // different number entirely — and one that would be committed to
    // BENCHMARKS.md as though it meant something.
    if cfg!(debug_assertions) {
        eprintln!("error: this is a debug build; a timing from it is worthless");
        eprintln!("run: cargo run --release --example bench -p rbx_viewer -- ...");
        return ExitCode::FAILURE;
    }

    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let adapter = rbx_viewer::describe_adapter()?;
    let (commit, dirty) = commit();

    let mut fixtures = Vec::with_capacity(args.fixtures.len());
    for path in &args.fixtures {
        if !path.is_file() {
            eprintln!("-- {} (skipped: fixture missing)", path.display());
            fixtures.push(Fixture {
                path: path.clone(),
                outcome: Outcome::Skipped("fixture missing".to_string()),
            });
            continue;
        }
        eprintln!("-- {}", path.display());
        // A fixture that fails takes itself out of the run and nothing else
        // with it. Propagating here would throw away every fixture already
        // measured — minutes of GPU work — before the table or the JSON file
        // was ever written, over a place file that happened to be corrupt.
        let outcome = match measure::fixture(path, args) {
            Ok(measured) => Outcome::Measured(Box::new(measured)),
            Err(err) => {
                eprintln!("   failed: {err}");
                Outcome::Failed(err)
            }
        };
        fixtures.push(Fixture {
            path: path.clone(),
            outcome,
        });
    }

    let run = Run {
        adapter,
        commit,
        dirty,
        recorded_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or_default(),
        fixtures,
    };

    report::table(&run, args);
    report::json(&run, args, &args.json)?;
    println!("wrote {}", args.json.display());

    // Reported after both outputs exist, and still a failure: a run missing a
    // fixture is not a run anyone should quietly compare against a full one.
    let failed: Vec<String> = run
        .fixtures
        .iter()
        .filter_map(|fixture| match &fixture.outcome {
            Outcome::Failed(reason) => Some(format!("{}: {reason}", fixture.path.display())),
            _ => None,
        })
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("measurement failed for {}", failed.join("; ")))
    }
}

/// The commit these numbers belong to, and whether the tree they were taken
/// from actually matches it.
///
/// `unknown` rather than a failure when git has nothing to say: a tarball with
/// no `.git` is a perfectly good place to measure, it just cannot label the
/// result.
fn commit() -> (String, bool) {
    let Some(hash) = git(&["rev-parse", "HEAD"]) else {
        return ("unknown".to_string(), false);
    };
    let dirty = git(&["status", "--porcelain"]).is_none_or(|status| !status.is_empty());
    (hash, dirty)
}

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(Path::new("."))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
