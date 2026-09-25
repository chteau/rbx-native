//! The editor's command line: which place to open, at what quality, and what
//! to do to it once the window is up.
//!
//! `--select` and `--run` are the supported spelling of the
//! `RBX_STUDIO_SELECT`/`RBX_STUDIO_RUN` debugging variables that predate them
//! — the same two capabilities, reachable from a CI job or a wrapper script
//! that would rather pass arguments than export an environment it cannot see
//! the effect of. Both spellings stay: a flag simply wins over its variable
//! when the two disagree.

use std::path::PathBuf;

use rbx_viewer::QualityLevel;

pub(crate) const USAGE: &str = "\
usage: rbxstudio [options] [file.rbxl|.rbxm|.rbxlx|.rbxmx]

Without a file, opens the API key setup wizard on first launch and the
Home screen after that.

options:
  --select <target>[,<target>...]
        Select these instances once the place is open — the first on its
        own, each further one added the way a Shift-click would. A target is
        either an Explorer path anchored at a root (Workspace.Model.Part) or
        a bare name, which matches the first instance called that anywhere
        in the tree.
  --run <script.luau>
        Run this file against the place once the window is up, exactly as
        pasting it into the Command Bar and pressing Enter would.
  --quality <auto|1..21>
        Graphics quality, spelled the way rbxview spells it. Defaults to
        whatever the editor's own dropdown was last left at.
  --verbose
        Narrate startup on stdout: the file read, what --select resolved to,
        what --run reported back.
  --help
        Print this and exit.";

/// A parsed command line.
pub(crate) struct Arguments {
    /// `None` starts at the setup wizard or Home — see `home::route`.
    pub(crate) path: Option<PathBuf>,
    pub(crate) quality: QualityLevel,
    /// `--select`'s comma list, still raw: the names in it can only be looked
    /// up once the place has been read.
    pub(crate) select: Option<String>,
    /// `--run`'s script, still unopened — reading it is `main`'s job, so that
    /// a path typo fails at an exit code rather than as a line in the Output
    /// dock on the first frame.
    pub(crate) run: Option<PathBuf>,
    pub(crate) verbose: bool,
}

/// What [`parse`] concluded: open a place, or print [`USAGE`] and stop.
pub(crate) enum Parsed {
    Open(Arguments),
    Usage,
}

/// Reads the command line. `default_quality` is what a bare invocation opens
/// at when `--quality` is absent — the persisted [`crate::settings::Settings`],
/// so the last pick from the dropdown survives a relaunch; an explicit flag
/// still overrides it.
pub(crate) fn parse(arguments: &[String], default_quality: QualityLevel) -> Result<Parsed, String> {
    let mut path = None;
    let mut quality = default_quality;
    let mut select = None;
    let mut run = None;
    let mut verbose = false;

    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--help" | "-h" => return Ok(Parsed::Usage),
            "--verbose" => verbose = true,
            "--quality" => quality = value(&mut arguments, "--quality")?.parse()?,
            "--select" => select = Some(value(&mut arguments, "--select")?.clone()),
            "--run" => run = Some(PathBuf::from(value(&mut arguments, "--run")?)),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            file if path.is_none() => path = Some(PathBuf::from(file)),
            _ => return Err("only one place file can be opened".to_string()),
        }
    }

    Ok(Parsed::Open(Arguments {
        path,
        quality,
        select,
        run,
        verbose,
    }))
}

/// The value following a flag that takes one, or an error naming the flag
/// that was left dangling at the end of the line.
fn value<'a>(
    arguments: &mut impl Iterator<Item = &'a String>,
    flag: &str,
) -> Result<&'a String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// What a scripted launch asks the editor to do once its window is up, with
/// every part of it that could still fail already resolved: `--run`'s file has
/// been read off disk, and the flag-versus-variable precedence settled (see
/// `main`, which is where both happen).
#[derive(Default)]
pub(crate) struct Launch {
    /// `--select`'s comma list, or `RBX_STUDIO_SELECT` when the flag is
    /// absent.
    pub(crate) select: Option<String>,
    /// `--run`'s file contents, or `RBX_STUDIO_RUN`'s inline source when the
    /// flag is absent.
    pub(crate) run: Option<String>,
    pub(crate) verbose: bool,
}

impl Launch {
    /// One line of `--verbose` narration on stdout, a no-op without the flag.
    pub(crate) fn say(&self, message: impl std::fmt::Display) {
        if self.verbose {
            println!("rbxstudio: {message}");
        }
    }
}

#[cfg(test)]
#[path = "cli/tests.rs"]
mod tests;
