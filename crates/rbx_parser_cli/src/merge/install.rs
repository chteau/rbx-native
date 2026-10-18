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
    Ok(format!(
        "{} %O %A %B %P",
        shell_path(&exe.to_string_lossy())
    ))
}

/// `exe` as a word for the POSIX shell git runs the driver through (Git for
/// Windows' bundled `sh` too). That shell eats backslashes and splits on
/// spaces, and it cannot run Windows' verbatim `\\?\` paths that
/// `canonicalize` returns, so the prefix goes, separators become `/` and the
/// whole path is single-quoted.
fn shell_path(exe: &str) -> String {
    let exe = if let Some(unc) = exe.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        exe.strip_prefix(r"\\?\").unwrap_or(exe).to_string()
    };
    let exe = if cfg!(windows) {
        exe.replace('\\', "/")
    } else {
        exe
    };
    format!("'{}'", exe.replace('\'', r"'\''"))
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

#[cfg(test)]
mod tests {
    use super::shell_path;

    #[test]
    fn quotes_and_escapes_for_sh() {
        assert_eq!(shell_path("/usr/bin/rbxmerge"), "'/usr/bin/rbxmerge'");
        assert_eq!(shell_path("/a b/it's"), r"'/a b/it'\''s'");
    }

    #[cfg(windows)]
    #[test]
    fn drops_the_verbatim_prefix_and_backslashes() {
        assert_eq!(
            shell_path(r"\\?\D:\a\rbx native\target\debug\rbxmerge.exe"),
            "'D:/a/rbx native/target/debug/rbxmerge.exe'"
        );
        assert_eq!(
            shell_path(r"\\?\UNC\srv\share\rbxmerge.exe"),
            "'//srv/share/rbxmerge.exe'"
        );
    }
}
