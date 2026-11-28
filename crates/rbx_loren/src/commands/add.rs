//! `loren add <repo> [alias]` (`commands/add.js`), and the GitHub download
//! that replaces degit.

use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::commands::refresh::{refresh_project, resolve_project_tool};
use crate::constants::{is_spec_file, PROJECT_FILE};
use crate::ctx::{CliError, CmdResult, Ctx, Fetch};
use crate::fsutil::{self, is_dir, is_file, is_inside, rel, remove_path, walk_files, DEFAULT_SKIP};
use crate::names::{
    derive_package_name, is_reserved_package, parse_repo_spec, validate_identifier, RepoSpec,
};
use crate::project::{backup_path, find_entry_loose, project_paths, require_project};
use crate::scriptsync::is_script_sync;

/// Downloads a repository's zipball from GitHub and unpacks it without its top folder.
pub struct GithubFetch;

impl Fetch for GithubFetch {
    fn clone_repo(&self, spec: &str, dest: &Path) -> Result<(), String> {
        let (repo, git_ref) = match spec.split_once('#') {
            Some((repo, r)) => (repo, r),
            None => (spec, "HEAD"),
        };
        let url = format!("https://codeload.github.com/{repo}/zip/{git_ref}");
        let bytes = rbx_net::agent()
            .get(&url)
            .header("User-Agent", rbx_net::USER_AGENT)
            .call()
            .map_err(|err| err.to_string())?
            .into_body()
            .read_to_vec()
            .map_err(|err| err.to_string())?;
        let mut archive =
            zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| err.to_string())?;
        fs::create_dir_all(dest).map_err(|err| err.to_string())?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|err| err.to_string())?;
            let name = file
                .enclosed_name()
                .ok_or_else(|| format!("unsafe path in archive: {}", file.name()))?;
            // GitHub wraps everything in a `<repo>-<ref>` folder.
            let inner: PathBuf = name
                .components()
                .skip(1)
                .filter(|c| matches!(c, Component::Normal(_)))
                .collect();
            if inner.as_os_str().is_empty() {
                continue;
            }
            let out = dest.join(&inner);
            if !is_inside(dest, &out) {
                return Err(format!(
                    "refusing to write outside the destination: {}",
                    inner.display()
                ));
            }
            if file.is_dir() {
                fs::create_dir_all(&out).map_err(|err| err.to_string())?;
            } else {
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
                }
                let mut target = fs::File::create(&out).map_err(|err| err.to_string())?;
                std::io::copy(&mut file, &mut target).map_err(|err| err.to_string())?;
            }
        }
        Ok(())
    }
}

/// The folder name for a package: the alias, or one derived from the repository name.
fn package_name(spec: &RepoSpec, alias: Option<&str>) -> Result<String, CliError> {
    let res = match alias {
        Some(a) => validate_identifier(a, "Package name"),
        None => derive_package_name(&spec.repo),
    };
    res.map_err(CliError::new)
}

/// Where `require` can find the package: a root default.project.json or init.lua(u).
fn has_entry_point(dir: &Path) -> bool {
    [PROJECT_FILE, "init.lua", "init.luau"]
        .iter()
        .any(|f| is_file(&dir.join(f)))
}

fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fsutil::copy_path(from, to).map_err(|err| err.to_string())?;
    remove_path(from);
    Ok(())
}

pub fn add_command(
    ctx: &Ctx,
    repo: &str,
    alias: Option<&str>,
    force: bool,
    tool: Option<&str>,
) -> CmdResult {
    let spec = parse_repo_spec(repo).map_err(CliError::new)?;
    let name = package_name(&spec, alias)?;

    let root = require_project(&ctx.cwd)?;
    let pkg_dir = project_paths(&root).packages;
    let target = pkg_dir.join(&name);
    if !is_inside(&pkg_dir, &target)
        || fsutil::resolve(&pkg_dir, &name) == fsutil::resolve(&pkg_dir, "")
    {
        return Err(CliError::new(format!(
            "\"{name}\" is not a safe package folder name."
        )));
    }

    if is_reserved_package(&name) && !force {
        return Err(CliError::hint(
            format!(
                "{}/Promise is bundled with Loren and the runtime requires it.",
                rel(&root, &pkg_dir)
            ),
            if alias.is_some_and(|a| !a.is_empty()) {
                "Pick another alias."
            } else {
                "Pass an alias (loren add <repo> <Name>), or --force to replace the bundled Promise."
            },
        ));
    }
    let existing = find_entry_loose(&pkg_dir, &name);
    if let Some(existing) = &existing {
        if !force {
            return Err(CliError::hint(
                format!("{} already exists.", rel(&root, &pkg_dir.join(existing))),
                "Pass another alias (loren add <repo> <Name>), or --force to replace it.",
            ));
        }
    }

    let tool = resolve_project_tool(ctx, &root, tool)?;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // The counter keeps two adds in one process apart: macOS's clock only
    // ticks in microseconds, so parallel tests got the same folder and one
    // deleted the other's clone.
    static ADDS: AtomicU64 = AtomicU64::new(0);
    let n = ADDS.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("loren-add-{}-{nanos}-{n}", std::process::id()));
    let staged = stage_and_install(
        ctx,
        &root,
        &spec,
        &tmp.join(&name),
        &pkg_dir,
        &target,
        existing.as_deref(),
    );
    remove_path(&tmp);
    staged?;

    ctx.log
        .ok(&format!("Added {} as {}.", spec.spec, rel(&root, &target)));
    if is_script_sync(&root) && !is_file(&target.join("init.luau")) {
        ctx.log.warn(&format!(
            "{} has no init.luau at its root. Script Sync maps folders and .luau files only (no project files), so arrange it by hand if requiring it fails.",
            rel(&root, &target)
        ));
    } else if !has_entry_point(&target) {
        ctx.log.warn(&format!(
            "{} has no {PROJECT_FILE} or init.lua(u) at its root, so requiring it may not work. Point at its source folder by hand.",
            rel(&root, &target)
        ));
    } else {
        ctx.log.info(&format!(
            "Require it with: require(game:GetService(\"ReplicatedStorage\").LorenPackages.{name})"
        ));
    }

    let refreshed = refresh_project(ctx, &root, tool, true, true);
    Ok(if refreshed.ok { 0 } else { 1 })
}

fn stage_and_install(
    ctx: &Ctx,
    root: &Path,
    spec: &RepoSpec,
    staging: &Path,
    pkg_dir: &Path,
    target: &Path,
    existing: Option<&str>,
) -> Result<(), CliError> {
    ctx.log.info(&format!("Downloading {}...", spec.spec));
    ctx.fetch
        .clone_repo(&spec.spec, staging)
        .map_err(|err| CliError::new(format!("Could not download {}: {err}", spec.spec)))?;
    let empty = fs::read_dir(staging)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true);
    if !is_dir(staging) || empty {
        return Err(CliError::new(format!(
            "The download of {} was empty; nothing was added.",
            spec.spec
        )));
    }
    // Script Sync has no ignore list: test files would reach the place, so they are dropped.
    if is_script_sync(root) {
        let specs: Vec<PathBuf> = walk_files(staging, &DEFAULT_SKIP)
            .into_iter()
            .filter(|f| {
                f.file_name()
                    .is_some_and(|n| is_spec_file(&n.to_string_lossy()))
            })
            .collect();
        for f in &specs {
            remove_path(f);
        }
        if !specs.is_empty() {
            ctx.log.info(&format!(
                "Left out {} test file(s) (*.spec.lua/luau): Script Sync would sync them into your place.",
                specs.len()
            ));
        }
    }
    if let Some(existing) = existing {
        let existing_path = pkg_dir.join(existing);
        let stamp = fsutil::timestamp(SystemTime::now());
        let saved = backup_path(root, &existing_path, &stamp).map_err(CliError::new)?;
        remove_path(&existing_path);
        let saved_rel = saved.map(|s| rel(root, &s)).unwrap_or_default();
        ctx.log.warn(&format!(
            "Replaced {} (backup: {saved_rel}).",
            rel(root, &existing_path)
        ));
    }
    move_path(staging, target).map_err(CliError::new)
}
