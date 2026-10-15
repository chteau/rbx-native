//! Reading plugin files and copying their trees.

use std::path::Path;

use rbx_dom::{Ref, WeakDom};

use super::api::{Manifest, PluginSource};

/// Reads one plugin: a `.lua`/`.luau` file (`SourceBody::Luau`), a
/// `.rbxm` (rbx_binary) or `.rbxmx` (rbx_xml) model (`SourceBody::Model`),
/// or a directory holding a `plugin.toml` whose `entry` is a `.luau` file
/// (`Origin::Native`). The name is the file stem, or the manifest's.
/// A `.wasm`/`.wat` entry is an error naming `rbx_wasm` (not this host's).
pub fn read_source(path: &Path) -> Result<PluginSource, String> {
    todo!()
}

/// Parses `plugin.toml`: `name` (required), `version`, `description`,
/// `entry` (default `init.luau`), `permissions` (strings, see
/// `native::parse_permission`; an unknown one is an error).
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    todo!()
}

/// Deep-copies `root`'s subtree from `src` into `dst` under `parent` (`None`
/// = a root), with fresh referents from `dst`'s allocator. `Ref`-valued
/// properties pointing inside the subtree are remapped; ones pointing
/// outside are cleared. Returns the new root's referent.
pub fn copy_tree(src: &WeakDom, root: Ref, dst: &mut WeakDom, parent: Option<Ref>) -> Ref {
    todo!()
}

/// The scripts a plugin runs, depth-first in child order under `roots`:
/// class `Script` exactly, `Disabled` not true and `Enabled` not false.
pub(crate) fn scripts_to_run(dom: &WeakDom, roots: &[Ref]) -> Vec<Ref> {
    todo!()
}
