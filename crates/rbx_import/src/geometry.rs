//! Per-mesh geometry of a source file, for the avatar item checks: positions,
//! triangles, UVs, how many bones each vertex follows, and the pixel size of
//! every texture the file uses. Positions stay in the file's own units.

use std::path::Path;

mod fbx;
mod gltf;

/// One mesh of the file, under its node (or mesh) name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceMesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// One per vertex, or empty when the mesh has none.
    pub uvs: Vec<[f32; 2]>,
    /// Bones with a non-zero weight on each vertex; empty when unskinned.
    pub influences: Vec<u8>,
    /// Names of the bones the mesh is skinned to; empty when unskinned.
    pub bones: Vec<String>,
}

/// A texture the file uses, embedded or beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTexture {
    /// The image's file name, or `image <n>` for an embedded one.
    pub name: String,
    pub width: u32,
    pub height: u32,
}

/// What [`read_geometry`] found. `notes` names what could not be read (an
/// ASCII FBX, a texture file that is missing), in a sentence each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceGeometry {
    pub meshes: Vec<SourceMesh>,
    pub textures: Vec<SourceTexture>,
    pub notes: Vec<String>,
}

/// Reads every mesh and texture of a `.fbx` (binary), `.obj`, `.gltf` or
/// `.glb`. `Err` is a sentence for the user.
pub fn read_geometry(path: &Path) -> Result<SourceGeometry, String> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "fbx" => fbx::read(&read_file(path)?, path),
        "gltf" | "glb" => gltf::read(&read_file(path)?, path),
        "obj" => read_obj(path),
        _ => Err("Only .fbx, .obj, .gltf and .glb files can be read here.".to_string()),
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|err| format!("Couldn\u{2019}t read {}: {err}", path.display()))
}

fn read_obj(path: &Path) -> Result<SourceGeometry, String> {
    let options = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    };
    let (models, materials) = tobj::load_obj(path, &options)
        .map_err(|err| format!("Couldn\u{2019}t read {}: {err}", path.display()))?;
    let mut out = SourceGeometry::default();
    for model in models {
        let mesh = model.mesh;
        if mesh.indices.is_empty() {
            continue;
        }
        let positions: Vec<[f32; 3]> = mesh.positions.as_chunks::<3>().0.to_vec();
        let uvs = if mesh.texcoords.len() / 2 == positions.len() {
            mesh.texcoords.as_chunks::<2>().0.to_vec()
        } else {
            Vec::new()
        };
        out.meshes.push(SourceMesh {
            name: model.name,
            triangles: mesh.indices.as_chunks::<3>().0.to_vec(),
            positions,
            uvs,
            influences: Vec::new(),
            bones: Vec::new(),
        });
    }
    let dir = path.parent().unwrap_or(Path::new(""));
    match materials {
        Ok(materials) => {
            for material in materials {
                for texture in [material.diffuse_texture, material.normal_texture]
                    .into_iter()
                    .flatten()
                {
                    add_side_texture(dir, &texture, &mut out);
                }
            }
        }
        Err(err) => out.notes.push(format!(
            "The material file could not be read ({err}), so the textures were not checked."
        )),
    }
    Ok(out)
}

/// Adds the size of a texture file named by `reference`, looked up beside the
/// model (as written, then by file name alone, since exporters often store
/// absolute paths from another machine).
fn add_side_texture(dir: &Path, reference: &str, out: &mut SourceGeometry) {
    let relative = reference.replace('\\', "/");
    let name = relative.rsplit('/').next().unwrap_or(&relative);
    if name.is_empty() || out.textures.iter().any(|t| t.name == name) {
        return;
    }
    let size = imagesize::size(dir.join(&relative))
        .or_else(|_| imagesize::size(dir.join(name)))
        .ok();
    match size {
        Some(size) => out.textures.push(SourceTexture {
            name: name.to_string(),
            width: size.width as u32,
            height: size.height as u32,
        }),
        None => out.notes.push(format!(
            "The texture {name} is missing or is not an image beside the file, so its size was not checked."
        )),
    }
}

#[cfg(test)]
mod tests;
