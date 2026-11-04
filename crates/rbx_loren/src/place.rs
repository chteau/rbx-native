//! A Loren project read into a place the way Rojo builds one: folders,
//! `init.luau` folder modules, `.server`/`.client` scripts and nested
//! project files. The editor's Home uses it so a new Loren project opens
//! ready to play; the spec runner builds its test place with it.

use std::path::{Path, PathBuf};

use rbx_dom::{Ref, Variant, WeakDom};

use crate::constants::{Key, PROJECT_FILE};
use crate::fsutil;
use crate::json::Json;
use crate::project::node_path;
use crate::scriptsync::is_script_sync;

const SCRIPT_SUFFIXES: [(&str, &str); 8] = [
    (".server.luau", "Script"),
    (".server.lua", "Script"),
    (".client.luau", "LocalScript"),
    (".client.lua", "LocalScript"),
    (".local.luau", "LocalScript"),
    (".local.lua", "LocalScript"),
    (".luau", "ModuleScript"),
    (".lua", "ModuleScript"),
];

/// The script class and bare name for a file name, if it is a script file.
fn script_kind(file: &str) -> Option<(&'static str, &str)> {
    SCRIPT_SUFFIXES
        .iter()
        .find_map(|(suffix, class)| file.strip_suffix(suffix).map(|stem| (*class, stem)))
}

fn entry_names(dir: &Path) -> Result<Vec<String>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|err| format!("Could not read {}: {err}", dir.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names)
}

/// Which file each script's `Source` was read from.
pub type SourceFiles = Vec<(Ref, PathBuf)>;

fn set_source(
    dom: &mut WeakDom,
    files: &mut SourceFiles,
    instance: Ref,
    path: &Path,
) -> Result<(), String> {
    let text = fsutil::read_text(path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    dom.set_property(instance, "Source", Variant::String(text))
        .map_err(|err| format!("{err}"))?;
    files.push((instance, path.to_path_buf()));
    Ok(())
}

/// Adds the file or folder at `path` under `parent` as `name`, with Rojo's
/// rules (`X.luau` ModuleScript, `X.server.luau` Script, `X.client.luau`
/// LocalScript, a folder with `init*.luau` becomes that script, a folder
/// holding `default.project.json` follows that project's tree).
pub fn insert_path(dom: &mut WeakDom, parent: Ref, name: &str, path: &Path) -> Result<Ref, String> {
    insert_path_into(dom, &mut Vec::new(), parent, name, path)
}

fn insert_path_into(
    dom: &mut WeakDom,
    files: &mut SourceFiles,
    parent: Ref,
    name: &str,
    path: &Path,
) -> Result<Ref, String> {
    if fsutil::is_file(path) {
        let file = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (class, _) =
            script_kind(&file).ok_or_else(|| format!("Unsupported file {}", path.display()))?;
        let made = dom.new_instance(class, name, Some(parent));
        set_source(dom, files, made, path)?;
        return Ok(made);
    }
    if !fsutil::is_dir(path) {
        return Err(format!("{} does not exist", path.display()));
    }

    let project = path.join(PROJECT_FILE);
    if fsutil::is_file(&project) {
        let text = fsutil::read_text(&project)
            .map_err(|err| format!("Could not read {}: {err}", project.display()))?;
        let data = Json::parse(fsutil::strip_bom(&text))
            .map_err(|err| format!("{} is not valid JSON: {err}", project.display()))?;
        let tree = data
            .get("tree")
            .ok_or_else(|| format!("{} has no tree", project.display()))?;
        return build_node(dom, files, parent, name, tree, path)?
            .ok_or_else(|| format!("{} points at nothing", project.display()));
    }

    let names = entry_names(path)?;
    let init = names.iter().find_map(|file| match script_kind(file) {
        Some((class, "init")) => Some((class, file.clone())),
        _ => None,
    });
    let (class, init_file) = match init {
        Some((class, file)) => (class, Some(file)),
        None => ("Folder", None),
    };
    let made = dom.new_instance(class, name, Some(parent));
    if let Some(file) = &init_file {
        set_source(dom, files, made, &path.join(file))?;
    }
    insert_children(dom, files, made, path, &names, init_file.as_deref())?;
    Ok(made)
}

fn insert_children(
    dom: &mut WeakDom,
    files: &mut SourceFiles,
    parent: Ref,
    dir: &Path,
    names: &[String],
    skip: Option<&str>,
) -> Result<(), String> {
    for file in names {
        if Some(file.as_str()) == skip || file == PROJECT_FILE {
            continue;
        }
        let full = dir.join(file);
        if fsutil::is_dir(&full) {
            insert_path_into(dom, files, parent, file, &full)?;
        } else if let Some((_, stem)) = script_kind(file) {
            insert_path_into(dom, files, parent, stem, &full)?;
        }
    }
    Ok(())
}

/// Builds one project-tree node; `None` when it is an optional path that is absent.
fn build_node(
    dom: &mut WeakDom,
    files: &mut SourceFiles,
    parent: Ref,
    name: &str,
    node: &Json,
    base: &Path,
) -> Result<Option<Ref>, String> {
    let made = if let Some(rel) = node_path(node) {
        let target = fsutil::resolve(base, rel);
        let optional = node.get("$path").is_some_and(Json::is_object);
        if optional && !fsutil::exists(&target) {
            return Ok(None);
        }
        insert_path_into(dom, files, parent, name, &target)?
    } else if let Some(class) = node.get("$className").and_then(Json::as_str) {
        dom.new_instance(class, name, Some(parent))
    } else {
        // Like Rojo: a bare node under StarterPlayer is its scripts container,
        // and one the place already has is filled in rather than shadowed by a
        // Folder of the same name, which Roblox would never run.
        let player_scripts = dom
            .get(parent)
            .is_some_and(|p| p.class() == "StarterPlayer")
            && matches!(name, "StarterPlayerScripts" | "StarterCharacterScripts");
        match find_child(dom, parent, name) {
            Some(existing) if player_scripts => existing,
            _ if player_scripts => dom.new_instance(name, name, Some(parent)),
            _ => dom.new_instance("Folder", name, Some(parent)),
        }
    };
    build_children(dom, files, made, node, base)?;
    Ok(Some(made))
}

fn build_children(
    dom: &mut WeakDom,
    files: &mut SourceFiles,
    parent: Ref,
    node: &Json,
    base: &Path,
) -> Result<(), String> {
    let Some(entries) = node.as_object() else {
        return Ok(());
    };
    for (key, child) in entries.iter() {
        if key.starts_with('$') || !child.is_object() {
            continue;
        }
        build_node(dom, files, parent, key, child, base)?;
    }
    Ok(())
}

fn find_service(dom: &mut WeakDom, name: &str) -> Ref {
    let found = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == name));
    found.unwrap_or_else(|| dom.new_instance(name, name, None))
}

fn find_child(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    let children = dom.get(parent)?.children();
    children
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
}

fn apply_script_sync(
    dom: &mut WeakDom,
    files: &mut SourceFiles,
    root: &Path,
) -> Result<(), String> {
    let layout: [(Key, &[&str]); 4] = [
        (Key::Shared, &["ReplicatedStorage", "Shared"]),
        (Key::Packages, &["ReplicatedStorage", "LorenPackages"]),
        (Key::Server, &["ServerScriptService", "Server"]),
        (
            Key::Client,
            &["StarterPlayer", "StarterPlayerScripts", "Client"],
        ),
    ];
    for (key, parts) in layout {
        let dir = fsutil::resolve(root, key.script_sync_path());
        if !fsutil::is_dir(&dir) {
            continue;
        }
        let Some((last, middle)) = parts.split_last() else {
            continue;
        };
        let mut at = find_service(dom, middle[0]);
        for part in &middle[1..] {
            at = match find_child(dom, at, part) {
                Some(existing) => existing,
                None => dom.new_instance("Folder", part, Some(at)),
            };
        }
        insert_path_into(dom, files, at, last, &dir)?;
    }
    Ok(())
}

/// Applies the project's `default.project.json` tree (or a Script Sync
/// project's four folders) onto `dom`, reusing services already there.
pub fn apply_project(dom: &mut WeakDom, root: &Path) -> Result<(), String> {
    apply_project_files(dom, root).map(drop)
}

/// `apply_project`, also returning the file behind each script, for lints
/// that report a place's scripts by file.
pub fn apply_project_files(dom: &mut WeakDom, root: &Path) -> Result<SourceFiles, String> {
    let mut files = SourceFiles::new();
    if is_script_sync(root) {
        apply_script_sync(dom, &mut files, root)?;
        return Ok(files);
    }
    let file = root.join(PROJECT_FILE);
    let text =
        fsutil::read_text(&file).map_err(|err| format!("Could not read {PROJECT_FILE}: {err}"))?;
    let data = Json::parse(fsutil::strip_bom(&text))
        .map_err(|err| format!("{PROJECT_FILE} is not valid JSON: {err}"))?;
    let Some(entries) = data.get("tree").and_then(Json::as_object) else {
        return Ok(files);
    };
    for (name, node) in entries.iter() {
        if name.starts_with('$') || !node.is_object() {
            continue;
        }
        let service = find_service(dom, name);
        build_children(dom, &mut files, service, node, root)?;
    }
    Ok(files)
}

#[cfg(test)]
mod tests;
