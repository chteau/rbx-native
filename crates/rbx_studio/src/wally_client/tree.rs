//! Turns one package's unzipped file list into a [`PackageNode`] tree, by
//! the same Rojo convention Argon's own snapshot decoding already assumes:
//! a directory holding `init.lua`/`init.luau` becomes the `ModuleScript`
//! its siblings are children of; a bare `.lua`/`.luau` file is a leaf
//! `ModuleScript`; anything else — a `README`, a `.toml`, a vendored test
//! suite, whatever else a package's repo happens to ship (confirmed by
//! hand this session: real packages can ship a lot more than their
//! installable Lua) — is skipped, not curated around. That's exactly what
//! a Rojo-style syncer turns into instances and nothing else.

use std::collections::BTreeMap;

/// One instance-to-be: `source` is `Some` for a `ModuleScript` (a `.lua`/
/// `.luau` file, or a directory whose own `init.lua`/`init.luau` supplies
/// it), `None` for a plain `Folder` — a directory with no init file of its
/// own, just a container for what's inside it.
pub(crate) struct PackageNode {
    pub(crate) name: String,
    pub(crate) source: Option<String>,
    pub(crate) children: Vec<PackageNode>,
}

enum Entry {
    File(String),
    Dir(BTreeMap<String, Entry>),
}

/// Builds the tree from a flat `(forward-slash path, raw bytes)` list —
/// what unzipping hands back — and the package's own name (its `wally.
/// toml`'s bare name, e.g. `"net"` for `sleitnick/net`), which becomes the
/// root node's name since the zip root itself has none of its own.
pub(crate) fn build(files: &[(String, Vec<u8>)], root_name: &str) -> PackageNode {
    let mut root: BTreeMap<String, Entry> = BTreeMap::new();
    for (path, bytes) in files {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        insert(&mut root, &segments, bytes);
    }
    node_for_dir(root_name.to_owned(), root)
}

/// A plain slice, not a generic iterator: a version of this that recursed
/// over `impl Iterator` (re-wrapping it `.peekable()` at each level) hits
/// `overflow evaluating the requirement Peekable<Peekable<...>>` — every
/// recursive call would monomorphize a new, one-level-deeper concrete
/// type, which is unbounded at compile time even though the actual
/// recursion depth is small at runtime. A slice sidesteps that: `&[&str]`
/// is the same type at every depth.
fn insert(dir: &mut BTreeMap<String, Entry>, segments: &[&str], bytes: &[u8]) {
    let [segment, rest @ ..] = segments else {
        return;
    };
    if rest.is_empty() {
        dir.insert(
            (*segment).to_owned(),
            Entry::File(String::from_utf8_lossy(bytes).into_owned()),
        );
    } else {
        let child = dir
            .entry((*segment).to_owned())
            .or_insert_with(|| Entry::Dir(BTreeMap::new()));
        let Entry::Dir(child_dir) = child else {
            // A path collided a file with a directory (a malformed zip) —
            // there is nothing sane to insert into a file, so this entry
            // is dropped rather than panicking on a hostile/corrupt zip.
            return;
        };
        insert(child_dir, rest, bytes);
    }
}

fn strip_lua_extension(name: &str) -> Option<String> {
    name.strip_suffix(".luau")
        .or_else(|| name.strip_suffix(".lua"))
        .map(str::to_owned)
}

fn node_for_dir(name: String, dir: BTreeMap<String, Entry>) -> PackageNode {
    let init_key = ["init.luau", "init.lua"]
        .into_iter()
        .find(|candidate| dir.contains_key(*candidate));
    let source = init_key.and_then(|key| match dir.get(key) {
        Some(Entry::File(content)) => Some(content.clone()),
        _ => None,
    });

    let children = dir
        .into_iter()
        .filter(|(child_name, _)| Some(child_name.as_str()) != init_key)
        .filter_map(|(child_name, entry)| match entry {
            Entry::File(content) => strip_lua_extension(&child_name).map(|name| PackageNode {
                name,
                source: Some(content),
                children: Vec::new(),
            }),
            Entry::Dir(sub) => Some(node_for_dir(child_name, sub)),
        })
        .collect();

    PackageNode {
        name,
        source,
        children,
    }
}

#[cfg(test)]
mod tests;
