//! `rbxlua test`: runs a place's specs (see `rbx_lua::test_runner`, and
//! `rbx_loren::specs` for Loren's) and exits non-zero when any fails. The
//! place can be a Rojo/Argon project folder, read the way Rojo builds it.
//! Integration specs (`*.integration.spec`) run on a Local Run session
//! (`rbx_sim::integration`), after the others.

use std::path::PathBuf;
use std::process::ExitCode;

use rbx_dom::WeakDom;
use rbx_loren::{place, specs};
use rbx_lua::test_runner::{self, Options};
use rbx_sim::integration;

use crate::dom_io;

pub const USAGE: &str = "usage: rbxlua test <place.rbxl|.rbxlx|project folder> [--filter <text>] [--root <Path.To.Folder>]... [--format human|json] [--junit <out.xml>] [--seed <n>] [--network <profile>]";

#[derive(Debug, Default, PartialEq)]
pub struct TestArgs {
    pub place: PathBuf,
    pub filter: Option<String>,
    pub roots: Vec<String>,
    pub json: bool,
    pub junit: Option<PathBuf>,
    /// Integration specs: the seed and the link they start with.
    pub seed: u64,
    pub network: Option<String>,
}

pub fn parse<I: Iterator<Item = String>>(argv: I) -> Result<TestArgs, String> {
    let mut args = TestArgs::default();
    let mut place = None;
    let mut argv = argv.peekable();
    while let Some(arg) = argv.next() {
        let mut value = |flag: &str| argv.next().ok_or(format!("{flag} requires a value"));
        match arg.as_str() {
            "--filter" => args.filter = Some(value("--filter")?),
            "--root" => args.roots.push(value("--root")?),
            "--junit" => args.junit = Some(PathBuf::from(value("--junit")?)),
            "--seed" => {
                let seed = value("--seed")?;
                args.seed = seed
                    .parse()
                    .map_err(|_| format!("--seed expects a whole number, got '{seed}'"))?;
            }
            "--network" => {
                let name = value("--network")?;
                if integration::profile(&name).is_none() {
                    let names: Vec<_> = integration::PROFILES.iter().map(|(n, _)| *n).collect();
                    return Err(format!(
                        "unknown network profile '{name}' (one of {})",
                        names.join(", ")
                    ));
                }
                args.network = Some(name);
            }
            "--format" => match value("--format")?.as_str() {
                "human" => args.json = false,
                "json" => args.json = true,
                other => return Err(format!("unknown format '{other}'")),
            },
            _ if arg.starts_with("--") => return Err(format!("unknown flag '{arg}'")),
            _ if place.is_none() => place = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument '{arg}'")),
        }
    }
    args.place = place.ok_or("missing <place> argument")?;
    Ok(args)
}

/// Exit code 0 when every test passed (or there were none), 1 when one
/// failed or the run could not start.
pub fn run(args: &TestArgs) -> ExitCode {
    let report = (|| {
        let dom = match args.place.is_dir() {
            true => {
                let mut dom = WeakDom::new();
                place::apply_project(&mut dom, &args.place)?;
                dom
            }
            false => dom_io::read_dom(&args.place)?,
        };
        let files = test_runner::discover(&dom, &args.roots)?;
        let database = rbx_parser_cli::default_reflection_database();
        let options = Options {
            filter: args.filter.clone(),
            only: None,
        };
        let mut report = specs::run_mixed(&dom, &files, &database, &options);
        let files = test_runner::discover_integration(&dom, &args.roots)?;
        if !files.is_empty() {
            let config = integration::Config {
                seed: args.seed,
                network: args
                    .network
                    .as_deref()
                    .and_then(integration::profile)
                    .unwrap_or_default(),
            };
            let run = integration::run(&dom, &files, &database, &options, &config);
            report.files.extend(run.files);
        }
        if let Some(path) = &args.junit {
            std::fs::write(path, test_runner::junit(&report))
                .map_err(|err| format!("failed to write '{}': {err}", path.display()))?;
        }
        Ok::<_, String>(report)
    })();
    match report {
        Ok(report) => {
            if args.json {
                println!("{}", test_runner::json(&report));
            } else {
                print!("{}", test_runner::human(&report));
            }
            if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}
