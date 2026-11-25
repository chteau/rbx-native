//! `rbxlua loadtest`: a Local Run server with bot players on a ramp, for
//! CI. See `rbx_sim::loadtest` for what is measured.

use std::path::PathBuf;
use std::process::ExitCode;

use rbx_sim::bots::Mix;
use rbx_sim::loadtest::{self, Config, LoadTest, Ramp};

use crate::dom_io;

pub const USAGE: &str = "usage: rbxlua loadtest <place.rbxl|.rbxlx> [--bots N] [--ramp 5/10s] [--duration 60s] [--seed N] [--mix idle=1,wander=6,follow=0,click=2,remotes=0] [--recording run.rbxnrec] [--min-hz 57] [--json] [--out <report base path>]
  Exits 1 when the server's Heartbeat (its own step time, the bots' clients left out) stays under --min-hz (default 57) for 3 s in a row: the bot count it happened at is the breaking point. At most 50 bots; more than 20 warns. --out writes <base>.md and <base>.json.";

#[derive(Debug, PartialEq)]
pub struct LoadTestArgs {
    pub place: PathBuf,
    pub config: Config,
    pub recording: Option<PathBuf>,
    pub json: bool,
    pub out: Option<PathBuf>,
}

fn parse_mix(text: &str) -> Result<Mix, String> {
    let mut mix = Mix {
        idle: 0,
        wander: 0,
        follow: 0,
        click: 0,
        remotes: 0,
    };
    for part in text.split(',') {
        let (name, weight) = part
            .split_once('=')
            .ok_or(format!("mix entry '{part}' should look like wander=6"))?;
        let weight: u32 = weight
            .trim()
            .parse()
            .map_err(|_| format!("mix weight '{weight}' isn't a number"))?;
        let slot = match name.trim() {
            "idle" => &mut mix.idle,
            "wander" => &mut mix.wander,
            "follow" => &mut mix.follow,
            "click" => &mut mix.click,
            "remotes" => &mut mix.remotes,
            other => return Err(format!("unknown behaviour '{other}'")),
        };
        *slot = weight;
    }
    Ok(mix)
}

pub fn parse<I: Iterator<Item = String>>(argv: I) -> Result<LoadTestArgs, String> {
    // Without --ramp everyone joins at once.
    let mut config = Config {
        duration: Some(60.0),
        ramp: None,
        ..Config::default()
    };
    let (mut place, mut recording, mut json, mut out) = (None, None, false, None);
    let mut argv = argv.peekable();
    while let Some(arg) = argv.next() {
        let mut value = |flag: &str| argv.next().ok_or(format!("{flag} requires a value"));
        match arg.as_str() {
            "--bots" => {
                config.bots = value("--bots")?
                    .parse()
                    .map_err(|_| "--bots takes a number".to_owned())?
            }
            "--ramp" => config.ramp = Some(Ramp::parse(&value("--ramp")?)?),
            "--duration" => config.duration = Some(loadtest::parse_seconds(&value("--duration")?)?),
            "--seed" => {
                config.seed = value("--seed")?
                    .parse()
                    .map_err(|_| "--seed takes a number".to_owned())?
            }
            "--mix" => config.mix = parse_mix(&value("--mix")?)?,
            "--min-hz" => {
                config.min_hz = value("--min-hz")?
                    .parse()
                    .map_err(|_| "--min-hz takes a number".to_owned())?
            }
            "--recording" => recording = Some(PathBuf::from(value("--recording")?)),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--json" => json = true,
            _ if arg.starts_with("--") => return Err(format!("unknown flag '{arg}'")),
            _ if place.is_none() => place = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument '{arg}'")),
        }
    }
    Ok(LoadTestArgs {
        place: place.ok_or("missing <place> argument")?,
        config,
        recording,
        json,
        out,
    })
}

/// 0 when the server kept 60 Hz, 1 when it broke or the run failed.
pub fn run(args: LoadTestArgs) -> ExitCode {
    let result = (|| {
        let dom = dom_io::read_dom(&args.place)?;
        let recording = args
            .recording
            .as_deref()
            .map(rbx_sim::replay::Recording::load)
            .transpose()?;
        let name = args
            .place
            .file_name()
            .map_or("place".into(), |n| n.to_string_lossy().into_owned());
        let mut test = LoadTest::new(dom, &name, args.config.clone(), recording.as_ref())?;
        if let Some(warning) = test.warning() {
            eprintln!("warning: {warning}");
        }
        test.run(|test, sampled| {
            if let Some(s) = sampled.then(|| test.samples().last()).flatten() {
                eprintln!(
                    "{:>4.0} s  {:>2} bots  server {:>6.2} ms (worst {:>6.2})  Heartbeat {:>4.1} Hz",
                    s.t, s.bots, s.server_ms, s.server_ms_max, s.heartbeat_hz
                );
            }
            true
        })?;
        Ok::<_, String>(test.finish())
    })();
    let report = match result {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };
    let markdown = report.markdown();
    let json = serde_json::to_string_pretty(&report.json()).unwrap_or_default();
    if let Some(base) = &args.out {
        for (ext, text) in [("md", &markdown), ("json", &json)] {
            let path = base.with_extension(ext);
            if let Err(err) = std::fs::write(&path, text) {
                eprintln!("error: failed to write '{}': {err}", path.display());
                return ExitCode::from(1);
            }
        }
    }
    if args.json {
        println!("{json}");
    } else {
        print!("{markdown}");
    }
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<LoadTestArgs, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn flags_parse() {
        let parsed = args(&[
            "place.rbxl",
            "--bots",
            "30",
            "--ramp",
            "5/10s",
            "--duration",
            "120s",
            "--json",
            "--mix",
            "wander=3,remotes=1",
        ])
        .unwrap();
        assert_eq!(parsed.place, PathBuf::from("place.rbxl"));
        assert_eq!(parsed.config.bots, 30);
        assert_eq!(
            parsed.config.ramp,
            Some(Ramp {
                step: 5,
                every: 10.0
            })
        );
        assert_eq!(parsed.config.duration, Some(120.0));
        assert_eq!(parsed.config.mix.wander, 3);
        assert_eq!(parsed.config.mix.remotes, 1);
        assert_eq!(parsed.config.mix.idle, 0);
        assert!(parsed.json);
        assert!(args(&["--bots", "3"]).is_err());
        assert!(args(&["p.rbxl", "--mix", "dance=1"]).is_err());
        assert!(args(&["p.rbxl", "--wat"]).is_err());
    }
}
