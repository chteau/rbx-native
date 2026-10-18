//! `rbxmerge install`: registers this binary as git's `rbx` merge driver and
//! routes Roblox file extensions to it. Safe to re-run.

use std::path::PathBuf;
use std::process::Command;

const PATTERNS: [&str; 4] = [
    "*.rbxl merge=rbx",
    "*.rbxm merge=rbx",
    "*.rbxlx merge=rbx",
    "*.rbxmx merge=rbx",
];

fn git(args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|err| format!("failed to run git: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {} failed: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn driver_command() -> Result<String, String> {
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|err| format!("failed to locate rbxmerge: {err}"))?;
    let exe = exe.to_string_lossy();
    // Git runs the driver through a shell, which splits on spaces.
    let exe = if exe.contains(' ') {
        format!("\"{exe}\"")
    } else {
        exe.into_owned()
    };
    Ok(format!("{exe} %O %A %B %P"))
}

pub fn run() -> Result<(), String> {
    let top = git(&["rev-parse", "--show-toplevel"])
        .map_err(|err| format!("not inside a git work tree ({err})"))?;
    let driver = driver_command()?;
    git(&["config", "merge.rbx.name", "Roblox place merge (rbxmerge)"])?;
    git(&["config", "merge.rbx.driver", &driver])?;
    println!("rbxmerge: set merge.rbx.driver to {driver}");

    let attributes: PathBuf = PathBuf::from(top).join(".gitattributes");
    let mut content = match std::fs::read_to_string(&attributes) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(format!("failed to read '{}': {err}", attributes.display())),
    };
    let missing: Vec<&str> = PATTERNS
        .into_iter()
        .filter(|pattern| !content.lines().any(|line| line.trim() == *pattern))
        .collect();
    if missing.is_empty() {
        println!("rbxmerge: {} already up to date", attributes.display());
        return Ok(());
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    for pattern in &missing {
        content.push_str(pattern);
        content.push('\n');
    }
    std::fs::write(&attributes, content)
        .map_err(|err| format!("failed to write '{}': {err}", attributes.display()))?;
    println!(
        "rbxmerge: added {} to {}",
        missing.join(", "),
        attributes.display()
    );
    Ok(())
}
