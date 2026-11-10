//! Running Rokit, Rojo and Argon (`lib/run.js`, `lib/tool.js`'s `defaultRun`).
//! Commands take a [`Runner`] so tests can fake every program.

use std::path::PathBuf;
use std::process::{Command, Stdio as ProcessStdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stdio {
    /// Capture stdout and stderr.
    #[default]
    Pipe,
    /// Share the terminal.
    Inherit,
    /// No stdin, the terminal's stdout and stderr (an install without a TTY).
    InheritOutput,
}

#[derive(Debug, Clone, Default)]
pub struct RunOpts {
    pub cwd: Option<PathBuf>,
    pub stdio: Stdio,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnError {
    /// The program is not installed or not on `PATH` (`ENOENT`).
    NotFound,
    Other(String),
}

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<SpawnError>,
    pub signal: Option<String>,
}

impl RunResult {
    pub fn ok(&self) -> bool {
        self.error.is_none() && self.status == Some(0)
    }
}

pub trait Runner {
    fn run(&self, cmd: &str, args: &[String], opts: &RunOpts) -> RunResult;
}

/// Spawns real processes. On Windows a program that only exists as a `.cmd`
/// shim (Rokit's) is retried through `cmd /C`, as upstream does.
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, cmd: &str, args: &[String], opts: &RunOpts) -> RunResult {
        let first = spawn(Command::new(cmd).args(args), opts);
        if cfg!(windows) && first.error == Some(SpawnError::NotFound) {
            let mut shell = Command::new("cmd");
            shell.arg("/C").arg(cmd).args(args);
            let second = spawn(&mut shell, opts);
            let not_recognized = second.status == Some(9009)
                || second
                    .stderr
                    .to_lowercase()
                    .contains("is not recognized as an internal or external command");
            if second.error.is_none() && second.status != Some(0) && not_recognized {
                return RunResult {
                    error: Some(SpawnError::NotFound),
                    ..second
                };
            }
            return second;
        }
        first
    }
}

fn spawn(command: &mut Command, opts: &RunOpts) -> RunResult {
    if let Some(cwd) = &opts.cwd {
        command.current_dir(cwd);
    }
    match opts.stdio {
        Stdio::Pipe => {
            command
                .stdin(ProcessStdio::null())
                .stdout(ProcessStdio::piped())
                .stderr(ProcessStdio::piped());
            // Captured output: no console window of its own when the editor
            // (a GUI program on Windows) runs it. The inherited modes below
            // share the terminal `rbxstudio loren` was started from.
            #[cfg(windows)]
            std::os::windows::process::CommandExt::creation_flags(command, 0x0800_0000);
        }
        // `output()` pipes whatever is not set explicitly; a shared terminal has to be asked for.
        Stdio::Inherit => {
            command
                .stdin(ProcessStdio::inherit())
                .stdout(ProcessStdio::inherit())
                .stderr(ProcessStdio::inherit());
        }
        Stdio::InheritOutput => {
            command
                .stdin(ProcessStdio::null())
                .stdout(ProcessStdio::inherit())
                .stderr(ProcessStdio::inherit());
        }
    }
    match command.output() {
        Ok(output) => RunResult {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            error: None,
            signal: signal_of(&output.status),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => RunResult {
            error: Some(SpawnError::NotFound),
            ..RunResult::default()
        },
        Err(err) => RunResult {
            error: Some(SpawnError::Other(err.to_string())),
            ..RunResult::default()
        },
    }
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt as _;
    status.signal().map(|s| format!("signal {s}"))
}

#[cfg(not(unix))]
fn signal_of(_: &std::process::ExitStatus) -> Option<String> {
    None
}

/// True when `cmd --version` runs and exits 0.
pub fn has_command(runner: &dyn Runner, cmd: &str, cwd: &std::path::Path) -> bool {
    runner
        .run(
            cmd,
            &["--version".to_string()],
            &RunOpts {
                cwd: Some(cwd.to_path_buf()),
                stdio: Stdio::Pipe,
            },
        )
        .ok()
}

/// "not installed" vs "failed with status N", for messages.
pub fn describe_failure(cmd: &str, result: &RunResult) -> String {
    match &result.error {
        Some(SpawnError::NotFound) => format!("{cmd} is not installed or not on PATH"),
        Some(SpawnError::Other(message)) => format!("{cmd} could not start ({message})"),
        None => match (&result.signal, result.status) {
            (Some(signal), _) => format!("{cmd} was stopped by {signal}"),
            (None, Some(status)) => format!("{cmd} exited with status {status}"),
            (None, None) => format!("{cmd} did not run"),
        },
    }
}
