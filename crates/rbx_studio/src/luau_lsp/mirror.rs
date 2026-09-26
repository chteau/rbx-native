//! The place as `luau-lsp` needs to see it: a folder holding one file per
//! script and a Rojo-format `sourcemap.json` naming the whole instance tree,
//! so `require(script.Parent.Module)` resolves, and `workspace.Baseplate`
//! has a type, exactly as they would against a Rojo project on disk.
//!
//! Files are named by referent rather than by instance path: two siblings may
//! share a name, and a name may hold anything, but a referent is unique and
//! survives a rename or a reparent, so moving a script only rewrites the
//! sourcemap and never renames its file under an open document.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lsp_types::FileChangeType;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Map, Value};

use crate::script_editor::source;

pub(crate) const SOURCEMAP: &str = "sourcemap.json";

pub(crate) struct Mirror {
    root: PathBuf,
    /// What each script's file held after the last sync; a sync writes only
    /// what moved since.
    written: HashMap<Ref, String>,
    sourcemap: String,
}

impl Mirror {
    pub(crate) fn new(root: PathBuf) -> Self {
        Mirror {
            root,
            written: HashMap::new(),
            sourcemap: String::new(),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn path_of(&self, reference: Ref) -> PathBuf {
        self.root.join(file_name(reference))
    }

    /// By file name alone: the server may hand a path back spelled
    /// differently from how it was given (a drive letter's case on Windows),
    /// and nothing but this folder holds files named like a live referent.
    pub(crate) fn script_at(&self, path: &Path) -> Option<Ref> {
        let id = path.file_name()?.to_str()?.strip_suffix(".luau")?;
        let reference = Ref::new(id.parse().ok()?);
        self.written.contains_key(&reference).then_some(reference)
    }

    /// Brings the folder in line with `dom`, returning every file it created,
    /// changed or deleted — what the server has to be told about, since it
    /// only rereads a file it has not opened when told the file moved.
    pub(crate) fn sync(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
    ) -> io::Result<Vec<(PathBuf, FileChangeType)>> {
        fs::create_dir_all(&self.root)?;
        let mut scripts = HashMap::new();
        let sourcemap = sourcemap(dom, db, &mut scripts).to_string();

        let mut changes = Vec::new();
        for (&reference, text) in &scripts {
            let kind = match self.written.get(&reference) {
                Some(old) if old == text => continue,
                Some(_) => FileChangeType::CHANGED,
                None => FileChangeType::CREATED,
            };
            let path = self.path_of(reference);
            fs::write(&path, text)?;
            changes.push((path, kind));
        }
        for reference in self.written.keys() {
            if !scripts.contains_key(reference) {
                let path = self.path_of(*reference);
                // Already gone is as good as deleted.
                let _ = fs::remove_file(&path);
                changes.push((path, FileChangeType::DELETED));
            }
        }
        self.written = scripts;

        if sourcemap != self.sourcemap {
            let path = self.root.join(SOURCEMAP);
            fs::write(&path, &sourcemap)?;
            changes.push((path, FileChangeType::CHANGED));
            self.sourcemap = sourcemap;
        }
        Ok(changes)
    }
}

impl Drop for Mirror {
    fn drop(&mut self) {
        // A scratch copy of the place; the DOM is the only one that counts.
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn file_name(reference: Ref) -> String {
    format!("{}.luau", reference.value())
}

/// The whole tree under a `DataModel` root, collecting every script's source
/// into `scripts` on the way. Non-scripts are kept too — they are what gives
/// `workspace.Part` a type (Rojo's `--include-non-scripts`).
fn sourcemap(dom: &WeakDom, db: &ReflectionDatabase, scripts: &mut HashMap<Ref, String>) -> Value {
    let children: Vec<Value> = dom
        .root_refs()
        .iter()
        .filter_map(|&child| node(dom, db, child, scripts))
        .collect();
    json!({"name": "game", "className": "DataModel", "children": children})
}

fn node(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    scripts: &mut HashMap<Ref, String>,
) -> Option<Value> {
    let instance = dom.get(reference)?;
    let mut out = Map::new();
    out.insert("name".into(), instance.name().into());
    out.insert("className".into(), instance.class().into());
    if source::is_script_class(db, instance.class()) {
        scripts.insert(reference, source::read(dom, reference).unwrap_or_default());
        out.insert("filePaths".into(), json!([file_name(reference)]));
    }
    let children: Vec<Value> = instance
        .children()
        .iter()
        .filter_map(|&child| node(dom, db, child, scripts))
        .collect();
    if !children.is_empty() {
        out.insert("children".into(), children.into());
    }
    Some(out.into())
}

#[cfg(test)]
mod tests;
