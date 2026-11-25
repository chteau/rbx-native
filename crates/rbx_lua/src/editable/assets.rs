//! Editables made from content: another editable's copy, an opaque
//! snapshot's, or an asset's download. Opening an asset as an editable
//! takes permission, as on Roblox: the asset's creator has to be one the
//! place may use (the EditableAssets golden's `roblox_mesh`), while
//! `CreateMeshPartAsync` may use any mesh.

use mlua::{Lua, Result, Value};
use rbx_assets::AssetRef;
use rbx_dom::editable::lock;

use super::image::{self, LuaEditableImage};
use super::mesh::{self, MeshData};
use super::{target, Env, Target};

pub(super) fn error(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

/// Whether the place may open asset `id` as an editable.
fn permitted(env: &Env, id: u64) -> bool {
    if env.owners.is_empty() {
        return true;
    }
    // A lookup that fails (offline, an unknown asset) leaves it to the
    // download to say what's wrong.
    let Ok(info) = crate::server::product_info(id as i64, "Asset", env.cache_dir.clone()) else {
        return true;
    };
    let creator = &info["Creator"];
    match creator["CreatorTargetId"]
        .as_u64()
        .or_else(|| creator["Id"].as_u64())
    {
        Some(creator) => env.owners.contains(&creator),
        None => true,
    }
}

/// Why a download failed, as Roblox words it: `None` for a missing asset.
pub(super) enum Failure {
    Missing,
    Denied,
    Other,
}

/// `uri`'s bytes; `method` names the call for a permission error, `None`
/// skipping the check.
pub(super) fn download(env: &Env, uri: &str, check: bool) -> std::result::Result<Vec<u8>, Failure> {
    let source = env.assets().ok_or(Failure::Other)?;
    match AssetRef::parse(uri) {
        Ok(AssetRef::Id(id)) => {
            if check && !permitted(env, id) {
                return Err(Failure::Denied);
            }
            source.fetch(id).map_err(|e| {
                match e.contains("404") || e.to_lowercase().contains("not found") {
                    true => Failure::Missing,
                    false => Failure::Other,
                }
            })
        }
        Ok(AssetRef::Thumb(spec)) => source.fetch_thumb(&spec).map_err(|_| Failure::Other),
        _ => Err(Failure::Other),
    }
}

fn failure(method: &str, what: &str, uri: &str, failure: Failure) -> mlua::Error {
    match failure {
        Failure::Missing => error("AssetNotFound"),
        Failure::Denied => error(format!(
            "AssetService::{method} failed because no permission to load asset"
        )),
        Failure::Other => error(format!("Failed to load {what} {uri}")),
    }
}

/// The mesh `CreateEditableMeshAsync` copies or loads.
pub(super) fn mesh_data(env: &Env, content: &Value) -> Result<MeshData> {
    match target(content)? {
        Target::Object(object) => match super::as_mesh(&object) {
            Some(mesh) => mesh.read(|m| Ok(m.clone())),
            None => Err(error(
                "AssetService:CreateEditableMeshAsync: Object is expected to be an EditableMesh.",
            )),
        },
        Target::Opaque(referent) => snapshot_mesh(referent).ok_or_else(|| {
            error("AssetService:CreateEditableMeshAsync: Object is expected to be an EditableMesh.")
        }),
        Target::None => Err(error("Failed to load mesh ")),
        Target::Uri(uri) => {
            let bytes = download(env, &uri, true)
                .map_err(|f| failure("CreateEditableMeshAsync", "mesh", &uri, f))?;
            mesh::load::from_file(&bytes).ok_or_else(|| error(format!("Failed to load mesh {uri}")))
        }
    }
}

/// An opaque snapshot's mesh.
pub(super) fn snapshot_mesh(referent: rbx_dom::Ref) -> Option<MeshData> {
    match rbx_dom::editable::get(referent)? {
        rbx_dom::editable::Editable::Mesh(source) => {
            let shared = source.into_any().downcast::<mesh::SharedMesh>().ok()?;
            let data = lock(&shared.0).clone();
            Some(data)
        }
        _ => None,
    }
}

fn decode(bytes: Vec<u8>) -> Option<(u32, u32, Vec<u8>)> {
    let kind = rbx_assets::sniff(&bytes);
    let decoded = rbx_assets::decode_image(&rbx_assets::Asset { bytes, kind }).ok()?;
    Some((decoded.width(), decoded.height(), decoded.into_raw()))
}

pub(super) fn image_async(
    lua: &Lua,
    env: &Env,
    content: &Value,
) -> Result<Option<LuaEditableImage>> {
    const WRONG: &str =
        "AssetService:CreateEditableImageAsync: Object is expected to be an EditableImage";
    let pixels = match target(content)? {
        Target::Object(object) => match object.borrow::<LuaEditableImage>() {
            Ok(image) => {
                let pixels = lock(&image.pixels);
                (pixels.width, pixels.height, pixels.rgba.clone())
            }
            Err(_) => return Err(error(WRONG)),
        },
        Target::Opaque(referent) => match rbx_dom::editable::get(referent) {
            Some(rbx_dom::editable::Editable::Image(pixels)) => {
                let pixels = lock(&pixels);
                (pixels.width, pixels.height, pixels.rgba.clone())
            }
            _ => return Err(error(WRONG)),
        },
        Target::None => return Err(error("Failed to load texture ")),
        Target::Uri(uri) => {
            let bytes = download(env, &uri, true)
                .map_err(|f| failure("CreateEditableImageAsync", "texture", &uri, f))?;
            decode(bytes).ok_or_else(|| error(format!("Failed to load texture {uri}")))?
        }
    };
    image::from_rgba(lua, env, pixels)
}
