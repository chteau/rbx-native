//! `rbxlua`: runs a Luau script against a place/model file and, optionally,
//! writes the mutated DOM back out.

mod args;
mod dom_io;

use std::process::ExitCode;

use rbx_lua::{LineKind, Runtime};

fn main() -> ExitCode {
    let mut argv = std::env::args();
    let _program = argv.next();

    let parsed = match args::parse(argv) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("{}", args::usage());
            return ExitCode::from(2);
        }
    };

    match run(&parsed) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run(args: &args::Args) -> Result<(), String> {
    let dom = dom_io::read_dom(&args.place)?;
    let script = std::fs::read_to_string(&args.script)
        .map_err(|err| format!("failed to read '{}': {err}", args.script.display()))?;

    let database = rbx_parser_cli::default_reflection_database();
    let mut runtime = Runtime::new(dom, database).map_err(|err| format!("error: {err}"))?;

    // `run` drives the scheduler until no thread is left waiting (or
    // abandons them, warning, after its limit), so the save below sees
    // everything the script's threads did.
    let (output, failure) = match runtime.run(&script) {
        Ok(output) => (output, None),
        Err(err) => (*err.output, Some(err.error)),
    };
    // Lines printed before an error still reach the terminal.
    for (line, kind) in output.lines().iter().zip(output.kinds()) {
        match kind {
            LineKind::Print => println!("{line}"),
            LineKind::Warn | LineKind::Error => eprintln!("{line}"),
        }
    }
    if let Some(error) = failure {
        return Err(format!("error: {error}"));
    }

    let mut dom = runtime.into_dom();

    // Changes are read before the write so `--print-changes` reflects the
    // script's mutations even if the caller didn't ask for `--out`.
    if args.print_changes {
        for change in dom.take_changes() {
            println!("{change:?}");
        }
    }

    if let Some(out) = &args.out {
        dom_io::write_dom(out, &dom)?;
    }

    Ok(())
}
