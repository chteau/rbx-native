//! `rbxmerge`: a git merge driver for Roblox place and model files.
//!
//! ```text
//! rbxmerge <base> <ours> <theirs> [<path>]   # git: merge.rbx.driver "rbxmerge %O %A %B %P"
//! rbxmerge install                            # sets that up in the current repository
//! ```
//!
//! Exit codes: `0` merged cleanly (or `install` succeeded), `1` merged with
//! conflicts (the file holds ours wherever they conflicted, and with a
//! `<path>` a `<path>.merge.json` sidecar lists them), `2` an unreadable
//! input, a failed write, bad usage or `install` outside a repository; in
//! the last case `<ours>` is left untouched.

mod install;
mod io;
mod sidecar;

use std::path::Path;

const USAGE: &str = "usage: rbxmerge <base> <ours> <theirs> [<path>]\n       rbxmerge install";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.as_slice() {
        [command] if command == "install" => report(install::run().map(|()| 0)),
        [base, ours, theirs] => report(merge(base, ours, theirs, None)),
        [base, ours, theirs, path] => report(merge(base, ours, theirs, Some(path))),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

fn report(result: Result<i32, String>) -> i32 {
    result.unwrap_or_else(|message| {
        eprintln!("rbxmerge: {message}");
        2
    })
}

fn merge(base: &str, ours: &str, theirs: &str, file: Option<&str>) -> Result<i32, String> {
    let base_bytes = io::read_bytes(Path::new(base))?;
    let ours_bytes = io::read_bytes(Path::new(ours))?;
    let theirs_bytes = io::read_bytes(Path::new(theirs))?;
    let (base_dom, _) = io::parse(&base_bytes, Path::new(base))?;
    let (ours_dom, format) = io::parse(&ours_bytes, Path::new(ours))?;
    let (theirs_dom, _) = io::parse(&theirs_bytes, Path::new(theirs))?;

    let (merged, conflicts) = rbx_dom::merge::merge(&base_dom, &ours_dom, &theirs_dom);
    let encoded = io::encode(&merged, format)?;
    io::write(Path::new(ours), &encoded)?;

    if conflicts.is_empty() {
        if let Some(file) = file {
            sidecar::remove(file)?;
        }
        return Ok(0);
    }
    if let Some(file) = file {
        sidecar::write(file, &base_bytes, &theirs_bytes, &conflicts)?;
    }
    let label = file.unwrap_or(ours);
    for conflict in &conflicts {
        eprintln!(
            "rbxmerge: {label}: {}: {}",
            conflict.path,
            sidecar::target_text(&conflict.target)
        );
    }
    Ok(1)
}
