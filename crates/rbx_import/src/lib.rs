//! A local `.fbx`/`.obj`/`.gltf`/`.glb` readied for Roblox's Assets API: the
//! per-mesh triangle count (the importer refuses a mesh over
//! [`TRIANGLE_LIMIT`], `creator-docs` `art/modeling/specifications.md`), and
//! the one self-contained file to upload. The API takes `.fbx`, `.gltf` and
//! `.glb` (`cloud/guides/usage-assets.md`); `.obj` is rewritten as glTF and a
//! `.gltf` has its side files inlined.

mod fbx;
mod gltf_file;
mod obj;

use std::path::Path;

/// Roblox's budget for one mesh.
pub const TRIANGLE_LIMIT: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Fbx,
    Gltf,
    Glb,
    Obj,
}

impl Format {
    pub fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "fbx" => Some(Self::Fbx),
            "gltf" => Some(Self::Gltf),
            "glb" => Some(Self::Glb),
            "obj" => Some(Self::Obj),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshInfo {
    pub name: String,
    pub triangles: usize,
}

/// What a file holds, as far as it could be read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub meshes: Vec<MeshInfo>,
    /// Set when the triangle count could not be taken (an unreadable `.fbx`);
    /// the upload then goes ahead on Roblox's own check.
    pub unchecked: Option<String>,
}

impl Report {
    pub fn over_budget(&self) -> impl Iterator<Item = &MeshInfo> {
        self.meshes.iter().filter(|m| m.triangles > TRIANGLE_LIMIT)
    }

    /// Why the upload should not go ahead, or `None`.
    pub fn refusal(&self) -> Option<String> {
        let over: Vec<String> = self
            .over_budget()
            .map(|m| format!("{} ({} triangles)", m.name, m.triangles))
            .collect();
        (!over.is_empty()).then(|| {
            format!(
                "Roblox meshes can\u{2019}t exceed {TRIANGLE_LIMIT} triangles: {}. Reduce them (decimate, or split into several meshes) and import again.",
                over.join(", ")
            )
        })
    }
}

/// The file to upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub file_name: String,
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Reads `path`, counts its triangles and readies the upload. `Err` is a
/// sentence for the user.
pub fn prepare(path: &Path) -> Result<(Report, Upload), String> {
    let format = Format::of(path).ok_or("Only .fbx, .obj, .gltf and .glb files can be imported")?;
    let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let stem = path
        .file_stem()
        .map_or_else(|| "model".into(), |s| s.to_string_lossy().into_owned());
    prepare_bytes(format, &stem, bytes, path.parent())
        .map_err(|err| format!("{}: {err}", path.display()))
}

/// [`prepare`] on bytes already read; `dir` is where a `.gltf`'s side files
/// are looked for.
pub fn prepare_bytes(
    format: Format,
    stem: &str,
    bytes: Vec<u8>,
    dir: Option<&Path>,
) -> Result<(Report, Upload), String> {
    match format {
        Format::Fbx => {
            let report = fbx::report(&bytes);
            let upload = Upload {
                file_name: format!("{stem}.fbx"),
                content_type: "model/fbx",
                bytes,
            };
            Ok((report, upload))
        }
        Format::Glb => {
            let meshes = gltf_file::meshes(&bytes)?;
            let upload = Upload {
                file_name: format!("{stem}.glb"),
                content_type: "model/gltf-binary",
                bytes,
            };
            Ok((
                Report {
                    meshes,
                    unchecked: None,
                },
                upload,
            ))
        }
        Format::Gltf => {
            let bytes = gltf_file::inline_side_files(&bytes, dir)?;
            let meshes = gltf_file::meshes(&bytes)?;
            let upload = Upload {
                file_name: format!("{stem}.gltf"),
                content_type: "model/gltf+json",
                bytes,
            };
            Ok((
                Report {
                    meshes,
                    unchecked: None,
                },
                upload,
            ))
        }
        Format::Obj => {
            let (meshes, document) = obj::convert(&bytes)?;
            let upload = Upload {
                file_name: format!("{stem}.gltf"),
                content_type: "model/gltf+json",
                bytes: document,
            };
            Ok((
                Report {
                    meshes,
                    unchecked: None,
                },
                upload,
            ))
        }
    }
}

#[cfg(test)]
mod tests;
