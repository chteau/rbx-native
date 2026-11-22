//! Smoke-tests any place headless in Local Run: one player, a bot that
//! walks, jumps, equips the first tool and clicks the middle of the screen,
//! then checks nothing errored. DataStores stay in memory and nothing is
//! asked of the web, so a private game can be pointed at safely.
//!
//! ```text
//! cargo run -p rbx_sim --example smoke -- place.rbxl [--seconds 10]
//!     [--expect "printed text"]... [--stat Name=Value]... [--quiet]
//! ```
//!
//! Exits 1 on a script error, a kick, or an expectation not met.

#[path = "support/smoke.rs"]
mod smoke;

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut place, mut seconds, mut quiet) = (None, 10.0, false);
    let (mut expect, mut stats) = (Vec::new(), Vec::new());
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match arg.as_str() {
            "--seconds" => seconds = value().parse().unwrap_or_else(|_| usage()),
            "--expect" => expect.push(value()),
            "--stat" => {
                let pair = value();
                let (name, want) = pair.split_once('=').unwrap_or_else(|| usage());
                stats.push((name.to_owned(), want.to_owned()));
            }
            "--quiet" => quiet = true,
            _ if place.is_none() => place = Some(std::path::PathBuf::from(arg)),
            _ => usage(),
        }
    }
    let place = place.unwrap_or_else(|| usage());
    let report = rbx_sim::load::read_place(&place)
        .and_then(|dom| smoke::run(dom, seconds))
        .unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(1);
        });
    if !quiet {
        print!("{}", report.transcript());
    }
    let expect: Vec<&str> = expect.iter().map(String::as_str).collect();
    let stats: Vec<(&str, &str)> = stats
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let failures = report.failures(&expect, &stats);
    for failure in &failures {
        println!("FAIL {}: {failure}", place.display());
    }
    if !failures.is_empty() {
        std::process::exit(1);
    }
    println!("ok {} ({seconds}s)", place.display());
}

fn usage() -> ! {
    eprintln!(
        "usage: smoke <place.rbxl|.rbxlx> [--seconds N] [--expect TEXT]... [--stat Name=Value]... [--quiet]"
    );
    std::process::exit(2);
}
