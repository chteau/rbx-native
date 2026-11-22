//! Replays a Local Run recording headless, for CI: exits 1 when the replay
//! diverges from the recording or a script errors, 2 on a bad recording.
//! `--record` makes one first, from a place played by the smoke bot.
//!
//! ```text
//! cargo run -p rbx_sim --example replay -- run.rbxnrec [--quiet]
//! cargo run -p rbx_sim --example replay -- --record place.rbxl run.rbxnrec
//!     [--seconds 8] [--seed 1]
//! ```

#[allow(dead_code)]
#[path = "support/smoke.rs"]
mod smoke;

use rbx_sim::replay::{Recording, Replay, ENGINE};

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut files, mut record, mut quiet) = (Vec::new(), false, false);
    let (mut seconds, mut seed) = (8.0, 1u64);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match arg.as_str() {
            "--record" => record = true,
            "--seconds" => seconds = value().parse().unwrap_or_else(|_| usage()),
            "--seed" => seed = value().parse().unwrap_or_else(|_| usage()),
            "--quiet" => quiet = true,
            _ => files.push(std::path::PathBuf::from(arg)),
        }
    }
    if record {
        let [place, out] = files.as_slice() else {
            usage()
        };
        let (report, recording) = rbx_sim::load::read_place(place)
            .and_then(|dom| smoke::record(dom, seconds, seed))
            .unwrap_or_else(|err| fail(2, &err));
        if !quiet {
            print!("{}", report.transcript());
        }
        recording.save(out).unwrap_or_else(|err| fail(2, &err));
        println!(
            "recorded {} frames to {}",
            recording.frame_count(),
            out.display()
        );
        return;
    }
    let [file] = files.as_slice() else { usage() };
    let recording = Recording::load(file).unwrap_or_else(|err| fail(2, &err));
    if recording.engine != ENGINE {
        eprintln!(
            "warning: recorded with engine {}, replaying on {ENGINE}",
            recording.engine
        );
    }
    let frames = recording.frame_count();
    let mut replay = Replay::new(recording).unwrap_or_else(|err| fail(2, &err));
    let result = replay.run_to(frames);
    if !quiet {
        for (frame, peer, output) in &replay.output {
            for line in output.lines() {
                println!("[{frame:5} {peer:?}] {line}");
            }
        }
    }
    let errors = replay.errors();
    for (frame, peer, text) in &errors {
        eprintln!("script error at frame {frame} ({peer:?}): {text}");
    }
    match result {
        Err(divergence) => fail(1, &divergence.to_string()),
        Ok(()) if !errors.is_empty() => std::process::exit(1),
        Ok(()) => println!("replayed {frames} frames, no divergence"),
    }
}

fn fail(code: i32, why: &str) -> ! {
    eprintln!("{why}");
    std::process::exit(code)
}

fn usage() -> ! {
    eprintln!(
        "usage: replay RECORDING [--quiet]\n       replay --record PLACE OUT [--seconds S] [--seed N]"
    );
    std::process::exit(2)
}
