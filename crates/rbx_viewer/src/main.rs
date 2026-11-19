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

    if args.first().map(String::as_str) == Some("--gpu-probe") {
        return match args
            .get(1)
            .map(|path| gpu_probe(path.as_ref(), args.get(2)))
        {
            Some(Ok(())) => ExitCode::SUCCESS,
            Some(Err(err)) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
            None => {
                eprintln!("usage: {program} --gpu-probe <place-file> [quality-level]");
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

/// `--gpu-probe <place-file> [quality-level]`: lists every adapter, then
/// opens the place the way the editor does and draws a few frames, for
/// telling on a tester's machine whether and where the 3D view fails.
fn gpu_probe(path: &std::path::Path, level: Option<&String>) -> Result<(), String> {
    eprint!("{}", rbx_viewer::adapters_report());
    if let Some(level) = level {
        rbx_viewer::prefer_level(level.parse()?);
    }
    let mut viewer = rbx_viewer::Headless::load(path, true)?;
    for frame in 1..=3 {
        eprintln!("drawing frame {frame}");
        viewer.render_frame(640, 360)?;
    }
    eprintln!("reading the last frame back");
    let frame = viewer.take_frame()?.ok_or("no frame came back")?;
    let lit = frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[..3] != [0, 0, 0])
        .count();
    eprintln!(
        "rendered 640x360 on {}: {lit} of {} pixels not black",
        rbx_viewer::chosen_adapter().unwrap_or_default(),
        640 * 360
    );
    Ok(())
}

/// `--serve <web-dir> [place-file] [--port <n>]`: see `rbx_viewer::serve`.
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
