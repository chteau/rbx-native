//! A local `.fbx`/`.obj`/`.gltf`/`.glb` readied for Roblox's Assets API: the
//! per-mesh triangle count (Roblox refuses a mesh over [`TRIANGLE_LIMIT`],
//! `creator-docs` `art/modeling/specifications.md`; Studio's importer
//! decimates it to exactly that many instead, and so does this), and the one
//! self-contained file to upload. The API takes `.fbx`, `.gltf` and
//! `.glb` (`cloud/guides/usage-assets.md`); `.obj` is rewritten as glTF and a
//! `.gltf` has its side files inlined.

mod decimate;
mod fbx;
mod fbx_decimate;
mod geometry;
mod gltf_file;
mod obj;

pub use decimate::Decimated;
pub use geometry::{read_geometry, SourceGeometry, SourceMesh, SourceTexture};

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
    /// Meshes brought under the limit in the upload; `meshes` has their new
    /// counts.
    pub decimated: Vec<Decimated>,
}

impl Report {
    pub fn over_budget(&self) -> impl Iterator<Item = &MeshInfo> {
        self.meshes.iter().filter(|m| m.triangles > TRIANGLE_LIMIT)
    }

    /// Why the upload should not go ahead, or `None`. Every other format is
    /// decimated under the limit, so only an ASCII FBX can be refused.
    pub fn refusal(&self) -> Option<String> {
        let over: Vec<String> = self
            .over_budget()
            .map(|m| format!("{} ({} triangles)", m.name, m.triangles))
            .collect();
        (!over.is_empty()).then(|| {
            format!(
                "Roblox meshes can\u{2019}t exceed {TRIANGLE_LIMIT} triangles: {}. An ASCII FBX can\u{2019}t be decimated on import: export it as binary FBX or glTF and import again.",
                over.join(", ")
            )
        })
    }

    /// What decimation did, as a sentence, or `None`.
    pub fn decimation_note(&self) -> Option<String> {
        let done: Vec<String> = self
            .decimated
            .iter()
            .map(|d| format!("{} from {} to {}", d.name, d.from, d.to))
            .collect();
        (!done.is_empty()).then(|| {
            format!(
                "Over Roblox\u{2019}s {TRIANGLE_LIMIT}-triangle limit, so decimated like Studio\u{2019}s importer does: {}.",
                done.join(", ")
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
            let mut report = fbx::report(&bytes);
            let mut bytes = bytes;
            if report.over_budget().next().is_some() {
                if let Some((smaller, decimated)) = fbx_decimate::decimate(&bytes, TRIANGLE_LIMIT)?
                {
                    bytes = smaller;
                    report = Report {
                        decimated,
                        ..fbx::report(&bytes)
                    };
                }
            }
            let upload = Upload {
                file_name: format!("{stem}.fbx"),
                content_type: "model/fbx",
                bytes,
            };
            Ok((report, upload))
        }
        Format::Glb => {
            let (bytes, meshes, decimated) = gltf_within_limit(bytes)?;
            let upload = Upload {
                file_name: format!("{stem}.glb"),
                content_type: "model/gltf-binary",
                bytes,
            };
            Ok((
                Report {
                    meshes,
                    unchecked: None,
                    decimated,
                },
                upload,
            ))
        }
        Format::Gltf => {
            let bytes = gltf_file::inline_side_files(&bytes, dir)?;
            let (bytes, meshes, decimated) = gltf_within_limit(bytes)?;
            let upload = Upload {
                file_name: format!("{stem}.gltf"),
                content_type: "model/gltf+json",
                bytes,
            };
            Ok((
                Report {
                    meshes,
                    unchecked: None,
                    decimated,
                },
                upload,
            ))
        }
        Format::Obj => {
            let (meshes, decimated, document) = obj::convert(&bytes)?;
            let upload = Upload {
                file_name: format!("{stem}.gltf"),
                content_type: "model/gltf+json",
                bytes: document,
            };
            Ok((
                Report {
                    meshes,
                    unchecked: None,
                    decimated,
                },
                upload,
            ))
        }
    }
}

/// The document, its meshes' counts after, and those decimated.
type Limited = (Vec<u8>, Vec<MeshInfo>, Vec<Decimated>);

/// A glTF document (or GLB) with every mesh over the limit decimated, and
/// its counts after.
fn gltf_within_limit(bytes: Vec<u8>) -> Result<Limited, String> {
    let meshes = gltf_file::meshes(&bytes)?;
    if meshes.iter().all(|m| m.triangles <= TRIANGLE_LIMIT) {
        return Ok((bytes, meshes, Vec::new()));
    }
    let (bytes, decimated) = gltf_file::decimate(&bytes, TRIANGLE_LIMIT)?;
    let meshes = gltf_file::meshes(&bytes)?;
    Ok((bytes, meshes, decimated))
}

#[cfg(test)]
mod tests;
