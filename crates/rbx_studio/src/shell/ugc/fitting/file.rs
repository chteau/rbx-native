//! An item picked as a source file: its biggest mesh written into the local
//! asset cache and placed in `Workspace` as a `MeshPart`, one undo step, so
//! it can be fitted before anything is uploaded. File units are taken as
//! studs, as Roblox's importer takes glTF units (see `rbx_autorig`'s glTF
//! writer).

use glam::{Vec2, Vec3};
use gpui_kit::*;
use rbx_autorig::Mesh;
use rbx_dom::{CFrameData, Content, Variant, Vector3Data};

use super::cages::{bounds, normals};
use super::{workspace, FittingWindow};
use crate::shell::rig;

const CACHE_FOLDER: &str = "rbx-native-local/fitting";

/// A source file's mesh, cached: the part's name, mesh URI and size.
#[derive(Debug, Clone, PartialEq)]
struct Loaded {
    name: String,
    uri: String,
    size: [f32; 3],
}

/// The file's biggest mesh other than a cage, centred on its bounds.
fn mesh_of(geometry: rbx_import::SourceGeometry) -> Result<(String, Mesh), String> {
    let source = geometry
        .meshes
        .into_iter()
        .filter(|m| !m.name.ends_with("_InnerCage") && !m.name.ends_with("_OuterCage"))
        .max_by_key(|m| m.triangles.len())
        .ok_or("The file holds no mesh")?;
    let mut positions: Vec<Vec3> = source.positions.iter().copied().map(Vec3::from).collect();
    let mut mesh = Mesh {
        positions: positions.clone(),
        ..Mesh::default()
    };
    let (centre, _) = bounds(&mesh);
    for p in &mut positions {
        *p -= Vec3::from(centre);
    }
    mesh.normals = normals(&positions, &source.triangles);
    mesh.positions = positions;
    mesh.uvs = source.uvs.iter().copied().map(Vec2::from).collect();
    mesh.triangles = source.triangles;
    Ok((source.name, mesh))
}

/// Blocking: reads `path` and caches its mesh.
fn load(path: &std::path::Path) -> Result<Loaded, String> {
    let (name, mesh) = mesh_of(rbx_import::read_geometry(path)?)?;
    let name = if name.is_empty() {
        path.file_stem()
            .map_or_else(|| "Item".into(), |s| s.to_string_lossy().into_owned())
    } else {
        name
    };
    let bytes = rbx_autorig::mesh_v2(&mesh);
    let relative = format!(
        "{CACHE_FOLDER}/{:016x}/{name}.mesh",
        crate::recovery::fnv1a(&bytes)
    );
    rbx_assets::AssetCache::new(None)
        .and_then(|c| c.put_native(&relative, &bytes))
        .map_err(|e| format!("caching the mesh: {e}"))?;
    Ok(Loaded {
        name,
        uri: format!("rbxasset://{relative}"),
        size: bounds(&mesh).1,
    })
}

fn v3(v: [f32; 3]) -> Variant {
    Variant::Vector3(Vector3Data {
        x: v[0],
        y: v[1],
        z: v[2],
    })
}

impl FittingWindow {
    pub(super) fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Fit".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let loaded = cx.background_spawn(async move { load(&path) }).await;
            let _ = this.update_in(cx, |view, window, cx| match loaded {
                Ok(loaded) => view.place_file(loaded, window, cx),
                Err(e) => {
                    view.source_error = Some(e);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Puts the loaded mesh in `Workspace` beside the fitting body, as one
    /// undo step, and fits it.
    fn place_file(&mut self, loaded: Loaded, window: &mut Window, cx: &mut Context<Self>) {
        let part = self.shell.update(cx, |shell, cx| {
            let ws = workspace(&shell.dom)?;
            let feet = rig::spawn_top(&shell.dom, ws);
            shell.push_history();
            let part = shell.dom.new_instance("MeshPart", &loaded.name, Some(ws));
            let at = CFrameData {
                position: Vector3Data {
                    x: feet[0] + 6.0,
                    y: feet[1] + loaded.size[1] / 2.0,
                    z: feet[2],
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            };
            for (key, value) in [
                ("MeshId", Variant::Content(Content::Uri(loaded.uri.clone()))),
                ("size", v3(loaded.size)),
                ("InitialSize", v3(loaded.size)),
                ("Anchored", Variant::Bool(true)),
                ("CFrame", Variant::CFrame(at)),
            ] {
                let _ = shell.dom.set_property(part, key, value);
            }
            let changes = shell.dom.take_changes();
            shell.rebuild_explorer(cx);
            shell.reflect_changes(&changes, cx);
            shell.record_history_change(changes);
            Some(part)
        });
        if let Some(part) = part {
            self.pick(part, window, cx);
            self.sync(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bounds, mesh_of};
    use rbx_import::{SourceGeometry, SourceMesh};

    fn mesh(name: &str, triangles: usize) -> SourceMesh {
        SourceMesh {
            name: name.into(),
            positions: vec![[2.0, 2.0, 2.0], [4.0, 2.0, 2.0], [2.0, 6.0, 2.0]],
            triangles: vec![[0, 1, 2]; triangles],
            uvs: Vec::new(),
            influences: Vec::new(),
            bones: Vec::new(),
        }
    }

    #[test]
    fn the_biggest_mesh_that_is_not_a_cage_is_centred() {
        let geometry = SourceGeometry {
            meshes: vec![mesh("Hat_InnerCage", 9), mesh("Brim", 1), mesh("Hat", 2)],
            textures: Vec::new(),
            notes: Vec::new(),
        };
        let (name, m) = mesh_of(geometry).unwrap();
        assert_eq!(name, "Hat");
        let (centre, size) = bounds(&m);
        assert_eq!(centre, [0.0, 0.0, 0.0]);
        assert_eq!(size, [2.0, 4.0, 0.0]);
        assert_eq!(m.normals.len(), 3);
    }
}
