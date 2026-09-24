//! Binary entry point for `rbxview`: parses command line arguments and delegates to the lib.

use std::path::PathBuf;
use std::process::ExitCode;

use rbx_viewer::Options;

fn main() -> ExitCode {
    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "rbxview".to_string());
    let args: Vec<String> = args.collect();

    if args.first().map(String::as_str) == Some("--serve") {
        return match serve(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                eprintln!("usage: {program} --serve <web-dir> [place-file] [--port <n>]");
                ExitCode::FAILURE
            }
        };
    }

    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("{}", Options::usage(&program));
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = rbx_viewer::run(&options) {
        eprintln!("error: {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// `--serve <web-dir> [place-file] [--port <n>]` — see `rbx_viewer::serve`.
fn serve(args: &[String]) -> Result<(), String> {
    let mut paths = Vec::new();
    let mut port = rbx_viewer::DEFAULT_PORT;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .and_then(|port| port.parse().ok())
                    .ok_or("'--port' needs a port number")?;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option '{flag}'")),
            path => paths.push(PathBuf::from(path)),
        }
    }
    match &paths[..] {
        [web] => rbx_viewer::serve(web, None, port),
        [web, place] => rbx_viewer::serve(web, Some(place), port),
        _ => Err("expected the built web directory, and at most one place file".to_string()),
    }
}
