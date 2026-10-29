//! The background helper: the OS runs `rbxstudio --update-planning-tick`
//! every minute while a plan has steps left, so they fire with the editor
//! closed as long as the PC is on and the user logged in. A systemd user
//! timer on Linux, a launchd agent on macOS, a Task Scheduler task on
//! Windows. Registered by Start update, removed by the last run that finds
//! nothing left to do (or by Cancel).
//!
//! `RBX_STUDIO_UPDATE_PLAN_HELPER_DIR` (and the mock) write the files into
//! that folder instead and run nothing, so a test never touches the real
//! scheduler.

use std::path::{Path, PathBuf};

pub(crate) const TICK_FLAG: &str = "--update-planning-tick";
const NAME: &str = "rbx-native-update-planning";
const SCRATCH_VARIABLE: &str = "RBX_STUDIO_UPDATE_PLAN_HELPER_DIR";

/// What runs the steps while the editor is closed, for the steps view.
pub(crate) fn describe() -> &'static str {
    if cfg!(target_os = "linux") {
        "A systemd user timer (rbx-native-update-planning.timer) runs them every minute."
    } else if cfg!(target_os = "macos") {
        "A launchd agent (com.rbx-native-update-planning) runs them every minute."
    } else {
        "A Task Scheduler task (rbx-native-update-planning) runs them every minute."
    }
}

/// Variables the helper needs to find the same plans and mode as the
/// editor that registered it.
fn passed_env() -> Vec<(&'static str, String)> {
    ["XDG_CONFIG_HOME", super::api::MOCK_VARIABLE]
        .into_iter()
        .filter_map(|name| Some((name, std::env::var(name).ok().filter(|v| !v.is_empty())?)))
        .collect()
}

#[cfg(any(test, target_os = "linux"))]
fn systemd_units(exe: &Path, env: &[(&str, String)]) -> (String, String) {
    let environment: String = env
        .iter()
        .map(|(name, value)| format!("Environment=\"{name}={value}\"\n"))
        .collect();
    let service = format!(
        "[Unit]\nDescription=rbx-native update planning: apply due icon steps\n\n\
         [Service]\nType=oneshot\n{environment}ExecStart=\"{}\" {TICK_FLAG}\n",
        exe.display()
    );
    let timer = format!(
        "[Unit]\nDescription=rbx-native update planning, every minute\n\n\
         [Timer]\nOnCalendar=*-*-* *:*:00\nAccuracySec=1s\nPersistent=true\nUnit={NAME}.service\n\n\
         [Install]\nWantedBy=timers.target\n"
    );
    (service, timer)
}

#[cfg(any(test, target_os = "macos"))]
fn launchd_plist(exe: &Path, env: &[(&str, String)]) -> String {
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;");
    let environment: String = env
        .iter()
        .map(|(name, value)| format!("<key>{name}</key><string>{}</string>", escape(value)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key><string>com.{NAME}</string>\n\
         <key>ProgramArguments</key><array><string>{}</string><string>{TICK_FLAG}</string></array>\n\
         <key>EnvironmentVariables</key><dict>{environment}</dict>\n\
         <key>StartInterval</key><integer>60</integer>\n\
         <key>RunAtLoad</key><true/>\n\
         </dict></plist>\n",
        escape(&exe.display().to_string())
    )
}

#[cfg(any(test, not(any(target_os = "linux", target_os = "macos"))))]
fn schtasks_create(exe: &Path) -> Vec<String> {
    [
        "/Create", "/F", "/SC", "MINUTE", "/MO", "1", "/TN", NAME, "/TR",
    ]
    .into_iter()
    .map(str::to_string)
    .chain([format!("\"{}\" {TICK_FLAG}", exe.display())])
    .collect()
}

/// The files this OS's registration writes, and where.
fn files(exe: &Path, dir: &Path) -> Vec<(PathBuf, String)> {
    let env = passed_env();
    #[cfg(target_os = "linux")]
    {
        let (service, timer) = systemd_units(exe, &env);
        vec![
            (dir.join(format!("{NAME}.service")), service),
            (dir.join(format!("{NAME}.timer")), timer),
        ]
    }
    #[cfg(target_os = "macos")]
    {
        vec![(
            dir.join(format!("com.{NAME}.plist")),
            launchd_plist(exe, &env),
        )]
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = env;
        vec![(
            dir.join(format!("{NAME}.task.txt")),
            schtasks_create(exe).join(" "),
        )]
    }
}

fn scratch_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(SCRATCH_VARIABLE).filter(|d| !d.is_empty()) {
        return Some(dir.into());
    }
    super::api::mock_enabled().then(|| super::store::root().map(|r| r.join("mock-helper")))?
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("{program} {}: {status}", args.join(" ")))
}

/// The real scheduler's folder for this OS's files; Windows keeps none.
fn system_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "linux") {
        home.map(|h| h.join(".config/systemd/user"))
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/LaunchAgents"))
    } else {
        None
    }
}

/// Registers the helper (again: it is idempotent).
pub(crate) fn register() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let scratch = scratch_dir();
    let dir = scratch.clone().or_else(system_dir);
    if let Some(dir) = &dir {
        for (path, text) in files(&exe, dir) {
            super::store::write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
        }
    }
    if scratch.is_some() {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        run("systemctl", &["--user", "daemon-reload"])?;
        run(
            "systemctl",
            &["--user", "enable", "--now", &format!("{NAME}.timer")],
        )
    }
    #[cfg(target_os = "macos")]
    {
        let plist = dir
            .ok_or("no HOME")?
            .join(format!("com.{NAME}.plist"))
            .display()
            .to_string();
        let _ = run("launchctl", &["unload", &plist]);
        run("launchctl", &["load", "-w", &plist])
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let args = schtasks_create(&exe);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run("schtasks", &args)
    }
}

/// Removes the helper; nothing to remove is not an error.
pub(crate) fn unregister() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let scratch = scratch_dir();
    let dir = scratch.clone().or_else(system_dir);
    if scratch.is_none() {
        #[cfg(target_os = "linux")]
        let _ = run(
            "systemctl",
            &["--user", "disable", "--now", &format!("{NAME}.timer")],
        );
        #[cfg(target_os = "macos")]
        if let Some(dir) = &dir {
            let plist = dir.join(format!("com.{NAME}.plist")).display().to_string();
            let _ = run("launchctl", &["unload", "-w", &plist]);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = run("schtasks", &["/Delete", "/F", "/TN", NAME]);
    }
    if let Some(dir) = &dir {
        for (path, _) in files(&exe, dir) {
            let _ = std::fs::remove_file(path);
        }
    }
    #[cfg(target_os = "linux")]
    if scratch.is_none() {
        let _ = run("systemctl", &["--user", "daemon-reload"]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Vec<(&'static str, String)> {
        vec![("XDG_CONFIG_HOME", "/home/a b/.cfg".to_string())]
    }

    #[test]
    fn systemd_runs_the_tick_every_minute_and_catches_up() {
        let (service, timer) = systemd_units(Path::new("/opt/rbx native/rbxstudio"), &env());
        assert!(service.contains("Type=oneshot\n"));
        assert!(
            service.contains("ExecStart=\"/opt/rbx native/rbxstudio\" --update-planning-tick\n")
        );
        assert!(service.contains("Environment=\"XDG_CONFIG_HOME=/home/a b/.cfg\"\n"));
        assert!(timer.contains("OnCalendar=*-*-* *:*:00\n"));
        assert!(timer.contains("Persistent=true\n"));
        assert!(timer.contains("Unit=rbx-native-update-planning.service\n"));
        assert!(timer.contains("WantedBy=timers.target\n"));
    }

    #[test]
    fn launchd_runs_every_sixty_seconds() {
        let plist = launchd_plist(Path::new("/Applications/R&N.app/rbxstudio"), &env());
        assert!(plist.contains("<string>/Applications/R&amp;N.app/rbxstudio</string><string>--update-planning-tick</string>"));
        assert!(plist.contains("<key>StartInterval</key><integer>60</integer>"));
        assert!(plist.contains("<key>XDG_CONFIG_HOME</key><string>/home/a b/.cfg</string>"));
    }

    #[test]
    fn task_scheduler_runs_every_minute() {
        let args = schtasks_create(Path::new("C:\\Program Files\\rbx\\rbxstudio.exe"));
        assert_eq!(&args[..6], ["/Create", "/F", "/SC", "MINUTE", "/MO", "1"]);
        assert_eq!(
            args.last().unwrap(),
            "\"C:\\Program Files\\rbx\\rbxstudio.exe\" --update-planning-tick"
        );
    }

    #[test]
    fn scratch_registration_writes_files_and_runs_nothing() {
        let dir = std::env::temp_dir().join(format!("rbx-up-helper-{}", std::process::id()));
        let written = files(Path::new("/x/rbxstudio"), &dir);
        assert!(!written.is_empty());
        assert!(written.iter().all(|(path, _)| path.starts_with(&dir)));
    }
}
