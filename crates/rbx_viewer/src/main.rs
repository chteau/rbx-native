//! Binary entry point for `rbxview`: parses command line arguments and delegates to the lib.

use std::process::ExitCode;

use rbx_viewer::Options;

fn main() -> ExitCode {
    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "rbxview".to_string());

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
