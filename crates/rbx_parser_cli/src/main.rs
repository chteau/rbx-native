use std::path::Path;
use std::process::ExitCode;

use rbx_dom::WeakDom;

fn main() -> ExitCode {
    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "rbxdump".to_string());

    let mut roundtrip = false;
    let mut path_arg = args.next();
    if path_arg.as_deref() == Some("--roundtrip") {
        roundtrip = true;
        path_arg = args.next();
    }

    let Some(path) = path_arg else {
        eprintln!("usage: {program} [--roundtrip] <file.rbxm|.rbxl|.rbxmx|.rbxlx>");
        // Usage errors are exit code 2 in --roundtrip mode (see module doc); the
        // default dump mode keeps its original FAILURE-only contract.
        return if roundtrip {
            ExitCode::from(2)
        } else {
            ExitCode::FAILURE
        };
    };

    if roundtrip {
        return run_roundtrip(&path);
    }

    if let Err(err) = run(&path) {
        eprintln!("error: {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

// Exit codes for --roundtrip: 0 both round-trips clean, 1 either failed, 2 the
// file couldn't even be loaded (usage/read/parse error, not a round-trip result).
fn run_roundtrip(path: &str) -> ExitCode {
    let dom = match load_dom(path) {
        Ok(dom) => dom,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    let results = rbx_parser_cli::roundtrip::run(&dom);
    if rbx_parser_cli::roundtrip::report(&results) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run(path: &str) -> Result<(), String> {
    let dom = load_dom(path)?;
    let db = rbx_parser_cli::default_reflection_database();
    print!("{}", rbx_parser_cli::render_tree(&dom, &db));

    Ok(())
}

fn load_dom(path: &str) -> Result<WeakDom, String> {
    let extension = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    if !["rbxm", "rbxl", "rbxmx", "rbxlx"].contains(&extension) {
        return Err(format!(
            "'{path}' is not a .rbxm, .rbxl, .rbxmx or .rbxlx file"
        ));
    }

    let bytes = std::fs::read(path).map_err(|err| format!("failed to read '{path}': {err}"))?;
    if rbx_xml::is_xml(&bytes) {
        let text = std::str::from_utf8(&bytes)
            .map_err(|err| format!("'{path}' is not valid UTF-8 XML: {err}"))?;
        rbx_xml::deserialize(text).map_err(|err| format!("failed to parse '{path}': {err}"))
    } else {
        rbx_binary::deserialize(&bytes).map_err(|err| format!("failed to parse '{path}': {err}"))
    }
}
