//! Reading plugin files and copying their trees.

use std::collections::HashMap;
use std::path::Path;

use rbx_dom::{Ref, Variant, WeakDom};

use super::api::{Manifest, Origin, PluginSource, SourceBody};
use super::native::parse_permission;

/// Reads one plugin: a `.lua`/`.luau` file (`SourceBody::Luau`), a
/// `.rbxm` (rbx_binary) or `.rbxmx` (rbx_xml) model (`SourceBody::Model`),
/// or a directory holding a `plugin.toml` whose `entry` is a `.luau` file
/// (`Origin::Native`). The name is the file stem, or the manifest's.
/// A `.wasm`/`.wat` entry is an error naming `rbx_wasm` (not this host's).
pub fn read_source(path: &Path) -> Result<PluginSource, String> {
    let shown = path.display();
    if path.is_dir() {
        let manifest_path = path.join("plugin.toml");
        let text = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
        let manifest =
            parse_manifest(&text).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
        let entry_ext = Path::new(&manifest.entry)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        if matches!(entry_ext.as_deref(), Some("wasm" | "wat")) {
            return Err(format!(
                "{shown}: entry `{}` is a WASM module, which is run by rbx_wasm, not the Luau plugin host",
                manifest.entry
            ));
        }
        let entry_path = path.join(&manifest.entry);
        let source = std::fs::read_to_string(&entry_path)
            .map_err(|e| format!("cannot read {}: {e}", entry_path.display()))?;
        return Ok(PluginSource {
            name: manifest.name.clone(),
            body: SourceBody::Luau(source),
            origin: Origin::Native {
                path: path.to_path_buf(),
                manifest,
            },
        });
    }

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin")
        .to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let body = match ext.as_str() {
        "lua" | "luau" => SourceBody::Luau(
            std::fs::read_to_string(path).map_err(|e| format!("cannot read {shown}: {e}"))?,
        ),
        "rbxm" => {
            let bytes = std::fs::read(path).map_err(|e| format!("cannot read {shown}: {e}"))?;
            SourceBody::Model(rbx_binary::deserialize(&bytes).map_err(|e| format!("{shown}: {e}"))?)
        }
        "rbxmx" => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("cannot read {shown}: {e}"))?;
            SourceBody::Model(rbx_xml::deserialize(&text).map_err(|e| format!("{shown}: {e}"))?)
        }
        _ => {
            return Err(format!(
                "{shown}: not a plugin file (expected .lua, .luau, .rbxm or .rbxmx)"
            ))
        }
    };
    Ok(PluginSource {
        name,
        body,
        origin: Origin::Roblox {
            path: path.to_path_buf(),
        },
    })
}

/// Parses `plugin.toml`: `name` (required), `version`, `description`,
/// `entry` (default `init.luau`), `permissions` (strings, see
/// `native::parse_permission`; an unknown one is an error).
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let table = text
        .parse::<toml::Table>()
        .map_err(|e| format!("invalid plugin.toml: {e}"))?;
    let string = |key: &str| -> Result<Option<String>, String> {
        match table.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_str()
                .map(|s| Some(s.to_string()))
                .ok_or_else(|| format!("`{key}` must be a string")),
        }
    };
    let name = string("name")?.ok_or_else(|| "missing required `name`".to_string())?;
    let mut permissions = Vec::new();
    if let Some(value) = table.get("permissions") {
        let list = value
            .as_array()
            .ok_or_else(|| "`permissions` must be an array of strings".to_string())?;
        for item in list {
            let text = item
                .as_str()
                .ok_or_else(|| "`permissions` must be an array of strings".to_string())?;
            permissions.push(
                parse_permission(text).ok_or_else(|| format!("unknown permission `{text}`"))?,
            );
        }
    }
    Ok(Manifest {
        name,
        version: string("version")?.unwrap_or_default(),
        description: string("description")?.unwrap_or_default(),
        entry: string("entry")?.unwrap_or_else(|| "init.luau".to_string()),
        permissions,
    })
}

/// Deep-copies `root`'s subtree from `src` into `dst` under `parent` (`None`
/// = a root), with fresh referents from `dst`'s allocator. `Ref`-valued
/// properties pointing inside the subtree are remapped; ones pointing
/// outside are cleared. Returns the new root's referent.
pub fn copy_tree(src: &WeakDom, root: Ref, dst: &mut WeakDom, parent: Option<Ref>) -> Ref {
    let mut order: Vec<(Ref, Option<Ref>)> = Vec::new();
    let mut stack = vec![(root, None::<Ref>)];
    while let Some((old, old_parent)) = stack.pop() {
        order.push((old, old_parent));
        if let Some(inst) = src.get(old) {
            for child in inst.children().iter().rev() {
                stack.push((*child, Some(old)));
            }
        }
    }

    let mut map: HashMap<Ref, Ref> = HashMap::new();
    for (old, old_parent) in &order {
        let Some(inst) = src.get(*old) else { continue };
        let new_parent = match old_parent {
            Some(p) => map.get(p).copied(),
            None => parent,
        };
        let new = dst.new_instance(inst.class(), inst.name(), new_parent);
        map.insert(*old, new);
    }

    for (old, _) in &order {
        let (Some(inst), Some(&new)) = (src.get(*old), map.get(old)) else {
            continue;
        };
        for (name, value) in inst.properties() {
            let value = match value {
                Variant::Ref(target) => match map.get(target) {
                    Some(&mapped) => Variant::Ref(mapped),
                    None => continue,
                },
                other => other.clone(),
            };
            let _ = dst.set_property(new, name, value);
        }
    }
    map[&root]
}

/// The scripts a plugin runs, depth-first in child order under `roots`:
/// class `Script` exactly, `Disabled` not true and `Enabled` not false.
pub(crate) fn scripts_to_run(dom: &WeakDom, roots: &[Ref]) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = roots.iter().rev().copied().collect();
    while let Some(r) = stack.pop() {
        let Some(inst) = dom.get(r) else { continue };
        if inst.class() == "Script" {
            let disabled = matches!(inst.properties().get("Disabled"), Some(Variant::Bool(true)));
            let off = matches!(inst.properties().get("Enabled"), Some(Variant::Bool(false)));
            if !disabled && !off {
                out.push(r);
            }
        }
        stack.extend(inst.children().iter().rev().copied());
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::plugin::api::Permission;

    fn temp_dir() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rbx_loader_test_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_lua_file() {
        let dir = temp_dir();
        let file = dir.join("hello.lua");
        std::fs::write(&file, "print('hi')").unwrap();
        let source = read_source(&file).unwrap();
        assert_eq!(source.name, "hello");
        assert!(matches!(source.body, SourceBody::Luau(ref s) if s == "print('hi')"));
        assert!(matches!(source.origin, Origin::Roblox { .. }));
    }

    #[test]
    fn reads_native_directory() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("plugin.toml"),
            "name = \"Demo\"\nversion = \"1.0\"\npermissions = [\"dom.read\", \"http:example.com\"]\n",
        )
        .unwrap();
        std::fs::write(dir.join("init.luau"), "return 1").unwrap();
        let source = read_source(&dir).unwrap();
        assert_eq!(source.name, "Demo");
        let Origin::Native { manifest, .. } = source.origin else {
            panic!("expected a native origin");
        };
        assert_eq!(
            manifest.permissions,
            vec![Permission::DomRead, Permission::Http("example.com".into())]
        );
    }

    #[test]
    fn rejects_bad_permission_and_wasm() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("plugin.toml"),
            "name = \"X\"\npermissions = [\"root\"]\n",
        )
        .unwrap();
        assert!(read_source(&dir)
            .err()
            .unwrap()
            .contains("unknown permission"));
        let dir = temp_dir();
        std::fs::write(
            dir.join("plugin.toml"),
            "name = \"X\"\nentry = \"a.wasm\"\n",
        )
        .unwrap();
        assert!(read_source(&dir).err().unwrap().contains("rbx_wasm"));
    }

    #[test]
    fn manifest_defaults() {
        let m = parse_manifest("name = \"P\"").unwrap();
        assert_eq!(m.entry, "init.luau");
        assert_eq!(m.version, "");
        assert!(m.permissions.is_empty());
        assert!(parse_manifest("version = \"1\"").is_err());
    }

    #[test]
    fn copy_tree_remaps_refs() {
        let mut src = WeakDom::new();
        let outside = src.new_instance("Part", "Outside", None);
        let root = src.new_instance("Folder", "Root", None);
        let a = src.new_instance("Part", "A", Some(root));
        let b = src.new_instance("Part", "B", Some(root));
        src.set_property(a, "Link", Variant::Ref(b)).unwrap();
        src.set_property(b, "Link", Variant::Ref(outside)).unwrap();

        let mut dst = WeakDom::new();
        dst.new_instance("Folder", "Pad", None);
        let new_root = copy_tree(&src, root, &mut dst, None);
        let kids = dst.get(new_root).unwrap().children().to_vec();
        assert_eq!(kids.len(), 2);
        assert_eq!(dst.get(kids[0]).unwrap().name(), "A");
        assert_eq!(
            dst.get(kids[0]).unwrap().properties().get("Link"),
            Some(&Variant::Ref(kids[1]))
        );
        assert!(dst.get(kids[1]).unwrap().properties().get("Link").is_none());
    }

    #[test]
    fn scripts_skip_disabled_and_other_classes() {
        let mut dom = WeakDom::new();
        let root = dom.new_instance("Folder", "Root", None);
        let good = dom.new_instance("Script", "Good", Some(root));
        let off = dom.new_instance("Script", "Off", Some(root));
        dom.set_property(off, "Disabled", Variant::Bool(true))
            .unwrap();
        let not_enabled = dom.new_instance("Script", "NotEnabled", Some(root));
        dom.set_property(not_enabled, "Enabled", Variant::Bool(false))
            .unwrap();
        dom.new_instance("LocalScript", "Local", Some(root));
        dom.new_instance("ModuleScript", "Module", Some(root));
        let nested = dom.new_instance("Script", "Nested", Some(good));
        assert_eq!(scripts_to_run(&dom, &[root]), vec![good, nested]);
    }
}
