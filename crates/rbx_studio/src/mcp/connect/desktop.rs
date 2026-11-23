//! Claude Desktop's `claude_desktop_config.json`: where each OS keeps it,
//! and adding our server to it without touching anything else in it.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{Os, SERVER_NAME};

/// `RBX_STUDIO_CLAUDE_DESKTOP_CONFIG=<file>`: the config Add to Claude
/// Desktop writes instead of the real one, for tests and captures.
pub(crate) const OVERRIDE_VARIABLE: &str = "RBX_STUDIO_CLAUDE_DESKTOP_CONFIG";

const FILE: &str = "claude_desktop_config.json";

/// The folders the config path is built from.
#[derive(Debug, Clone, Default)]
pub(crate) struct Dirs {
    pub(crate) home: PathBuf,
    /// Windows' `%APPDATA%`.
    pub(crate) appdata: Option<PathBuf>,
    /// The Microsoft Store (MSIX) install's own package folder under
    /// `%LOCALAPPDATA%\Packages`, when there is one: that install reads its
    /// config from a redirected copy of `%APPDATA%` inside it.
    pub(crate) store_package: Option<PathBuf>,
    /// Linux's `$XDG_CONFIG_HOME`.
    pub(crate) config_home: Option<PathBuf>,
    pub(crate) overridden: Option<PathBuf>,
}

impl Dirs {
    pub(crate) fn current(home: &Path) -> Dirs {
        let var = |key: &str| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let store_package = var("LOCALAPPDATA").and_then(|local| {
            std::fs::read_dir(local.join("Packages"))
                .ok()?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("Claude_"))
                })
        });
        Dirs {
            home: home.to_owned(),
            appdata: var("APPDATA"),
            store_package,
            config_home: var("XDG_CONFIG_HOME"),
            overridden: var(OVERRIDE_VARIABLE),
        }
    }
}

/// Where Claude Desktop reads its config on `os`. Linux has no official
/// build; the community ones read the XDG config folder.
pub(crate) fn config_path(os: Os, dirs: &Dirs) -> PathBuf {
    if let Some(path) = &dirs.overridden {
        return path.clone();
    }
    let folder = match os {
        Os::Mac => dirs.home.join("Library/Application Support"),
        Os::Windows => match &dirs.store_package {
            Some(package) => package.join(r"LocalCache\Roaming"),
            None => dirs
                .appdata
                .clone()
                .unwrap_or_else(|| dirs.home.join(r"AppData\Roaming")),
        },
        Os::Linux => dirs
            .config_home
            .clone()
            .unwrap_or_else(|| dirs.home.join(".config")),
    };
    folder.join("Claude").join(FILE)
}

/// What adding the server did to the file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Change {
    Added,
    /// An entry under our name was there, and said this.
    Replaced(Value),
    Unchanged,
}

/// `existing` (the file's text, `None` when there is none) with our server
/// set to `entry`, every other key and server kept in place, and how many
/// other servers there are. A file that isn't a JSON object, or whose
/// `mcpServers` isn't one, is an error: it is never overwritten.
pub(crate) fn merged(
    existing: Option<&str>,
    entry: &Value,
) -> Result<(String, Change, usize), String> {
    let mut root = match existing.map(str::trim).filter(|text| !text.is_empty()) {
        None => Value::Object(Map::new()),
        Some(text) => serde_json::from_str(text)
            .map_err(|error| format!("it isn't valid JSON ({error}), so it was left as it is"))?,
    };
    let Value::Object(object) = &mut root else {
        return Err("it doesn't hold a JSON object, so it was left as it is".to_owned());
    };
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = servers else {
        return Err("its mcpServers isn't an object, so it was left as it is".to_owned());
    };
    let change = match servers.get(SERVER_NAME) {
        None => Change::Added,
        Some(old) if old == entry => Change::Unchanged,
        Some(old) => Change::Replaced(old.clone()),
    };
    servers.insert(SERVER_NAME.to_owned(), entry.clone());
    let others = servers.len() - 1;
    let mut text = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    text.push('\n');
    Ok((text, change, others))
}

/// What [`install`] did.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Installed {
    pub(crate) path: PathBuf,
    pub(crate) change: Change,
    /// The copy of the file as it was, when there was one to change.
    pub(crate) backup: Option<PathBuf>,
    pub(crate) others: usize,
}

/// Adds the server to the config at `path`, first copying the file as it
/// was next to it. `stamp` names that copy (seconds since the epoch).
pub(crate) fn install(path: &Path, entry: &Value, stamp: u64) -> Result<Installed, String> {
    let folder = path.parent().unwrap_or_else(|| Path::new("."));
    if !folder.is_dir() {
        return Err(format!(
            "{} doesn't exist. Install Claude Desktop and open it once, or copy the snippet by hand.",
            folder.display()
        ));
    }
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("couldn't read {}: {error}", path.display())),
    };
    let (text, change, others) = merged(existing.as_deref(), entry)
        .map_err(|why| format!("{} wasn't changed: {why}", path.display()))?;
    let mut installed = Installed {
        path: path.to_owned(),
        change,
        backup: None,
        others,
    };
    if installed.change == Change::Unchanged {
        return Ok(installed);
    }
    if existing.is_some() {
        let name = path.file_name().map_or_else(
            || FILE.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let backup = path.with_file_name(format!("{name}.{stamp}.bak"));
        std::fs::copy(path, &backup)
            .map_err(|error| format!("couldn't back up {}: {error}", path.display()))?;
        installed.backup = Some(backup);
    }
    crate::settings::write_atomic(path, text.as_bytes())
        .map_err(|error| format!("couldn't write {}: {error}", path.display()))?;
    Ok(installed)
}
