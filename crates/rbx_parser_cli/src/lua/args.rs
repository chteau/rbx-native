//! Command-line argument parsing for `rbxlua`.

use std::path::PathBuf;

/// Parsed invocation of the `rbxlua` binary.
pub struct Args {
    pub place: PathBuf,
    pub script: PathBuf,
    pub out: Option<PathBuf>,
    pub print_changes: bool,
}

const USAGE: &str =
    "usage: rbxlua <place.rbxl|.rbxm|.rbxlx|.rbxmx> <script.luau> [--out <file>] [--print-changes]";

pub fn usage() -> &'static str {
    USAGE
}

/// Parses `argv` (excluding the program name). Positional arguments are
/// consumed in order; `--out` takes the next argument as its value.
pub fn parse<I: Iterator<Item = String>>(argv: I) -> Result<Args, String> {
    let mut place = None;
    let mut script = None;
    let mut out = None;
    let mut print_changes = false;

    let mut argv = argv.peekable();
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--out" => {
                let value = argv.next().ok_or("--out requires a file path")?;
                out = Some(PathBuf::from(value));
            }
            "--print-changes" => print_changes = true,
            _ if arg.starts_with("--") => return Err(format!("unknown flag '{arg}'")),
            _ if place.is_none() => place = Some(PathBuf::from(arg)),
            _ if script.is_none() => script = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument '{arg}'")),
        }
    }

    let place = place.ok_or("missing <place> argument")?;
    let script = script.ok_or("missing <script> argument")?;

    Ok(Args {
        place,
        script,
        out,
        print_changes,
    })
}
