//! `insert_asset`, with Studio's arguments and answer: downloads an asset
//! by id through asset delivery (anonymous first, then Open Cloud's
//! `asset-delivery-api` with the key's `legacy-asset:manage` scope) and
//! plans the instances it becomes. The shell grafts them into the place;
//! nothing here writes to the DOM.
//!
//! What each kind becomes was read off Studio's own tool, inserting the same
//! ids: a model, package or mesh part asset lands in a `Model` named after
//! the asset (as `InsertService:LoadAsset` wraps it), an animation as its
//! `KeyframeSequence`, an image or decal as a `Decal`, audio as a `Sound`
//! and a video as a `VideoFrame`. A bare mesh, which Studio's tool refuses
//! (its metadata lookup 404s), becomes an anchored `MeshPart` of the mesh's
//! own size. Like Studio, every inserted root is tagged `Assistant:<guid>`
//! and the answer names that tag. Studio also moves what it inserts in front
//! of the camera; here it keeps the position it was published with, as
//! Import 3D does.

mod build;

pub(super) use build::build;
use build::{holds_package_link, missing, read_model};

use rbx_cloud::{AssetContent, Client, CloudError, KeyedRefusal};
use rbx_dom::attributes::encode_tags;
use rbx_dom::{Ref, Variant, WeakDom};
use serde_json::{json, Value};

use super::super::{path_of, resolve, text_argument, Outcome, ROOT};
use super::{client, describe, uuid};
use crate::mcp::{PlayState, Session};

pub(super) const NAME: &str = "insert_asset";

const DELIVERY_SCOPE: &str = "legacy-asset:manage";

/// `assetType`'s values and the kind each inserts as.
const TYPES: [(&str, Kind); 9] = [
    ("Model", Kind::Model),
    ("Package", Kind::Model),
    ("MeshPart", Kind::Model),
    ("Mesh", Kind::Mesh),
    ("Image", Kind::Image),
    ("Decal", Kind::Image),
    ("Audio", Kind::Audio),
    ("Video", Kind::Video),
    ("Animation", Kind::Animation),
];

/// What an `insert_asset` call adds, downloaded and ready to graft.
pub(crate) struct Insertion {
    pub(crate) asset_id: u64,
    /// Resolved when the call was read; the shell checks it still exists.
    pub(crate) parent: Ref,
    /// Its roots are what is inserted.
    pub(crate) content: WeakDom,
    pub(crate) asset_type: String,
    pub(crate) asset_name: String,
    pub(crate) is_package: bool,
    /// Why `parentPath` was not used, as Studio reports it.
    pub(crate) parent_warning: Option<String>,
    /// Names the `Assistant:<guid>` tag on every inserted root.
    pub(crate) guid: String,
}

/// Studio's failure, as the text of a tool error.
fn failure(message: impl Into<String>) -> String {
    json!({ "status": "error", "error": message.into() }).to_string()
}

pub(super) fn run(arguments: &Value, session: &mut Session) -> Outcome {
    if session.studio.play == PlayState::ShowingLive {
        return Err(failure(
            "The editor is showing a running playtest; switch back to the edited place first.",
        ));
    }
    let raw = match arguments.get("assetId") {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        _ => return Err(failure("Missing assetId.")),
    };
    let Some(asset_id) = raw.parse::<u64>().ok().filter(|&id| id > 0) else {
        return Err(failure(format!("Invalid asset ID: {raw}")));
    };
    // Studio goes by the type delivery reports and ignores a hint it does
    // not know.
    let hint = text_argument(arguments, "assetType")
        .and_then(|name| {
            TYPES
                .iter()
                .find(|(known, _)| known.eq_ignore_ascii_case(name.trim()))
        })
        .copied();
    let dom = session.dom;
    let Some(workspace) = dom
        .root_refs()
        .iter()
        .copied()
        .find(|&r| dom.get(r).is_some_and(|i| i.class() == "Workspace"))
    else {
        return Err(failure("The place has no Workspace to insert into."));
    };
    let (parent, parent_warning) = match text_argument(arguments, "parentPath") {
        Some(path) => match resolve(dom, path) {
            Some(parent) => (parent, None),
            None => (
                workspace,
                Some(format!(
                    "Could not resolve path \"{path}\": \"{}\" not found. Defaulting to workspace.",
                    missing(dom, path)
                )),
            ),
        },
        None => (workspace, None),
    };
    let name = text_argument(arguments, "assetName")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    session.inserting = Some(Box::new(move || {
        let mut insertion = fetch(asset_id, hint, name, parent).map_err(failure)?;
        insertion.parent_warning = parent_warning;
        Ok(insertion)
    }));
    Ok(Value::Null)
}

/// `insert_asset`'s answer, once the shell has grafted `roots`.
pub(crate) fn inserted(dom: &WeakDom, roots: &[Ref], insertion: &Insertion) -> Value {
    let instances: Vec<Value> = roots
        .iter()
        .filter_map(|&r| {
            let instance = dom.get(r)?;
            Some(json!({
                "fullPath": format!("{ROOT}.{}", path_of(dom, r)),
                "name": instance.name(),
                "className": instance.class(),
            }))
        })
        .collect();
    let mut answer = json!({
        "status": "success",
        "insertGuid": insertion.guid,
        "tag": tag(&insertion.guid),
        "assetId": insertion.asset_id.to_string(),
        "assetType": insertion.asset_type,
        "assetName": insertion.asset_name,
        "isPackage": insertion.is_package,
        "sandboxed": false,
        "insertedInstances": instances,
    });
    if let Some(warning) = &insertion.parent_warning {
        answer["parentWarning"] = json!(warning);
    }
    Value::String(answer.to_string())
}

fn fetch(
    asset_id: u64,
    hint: Option<(&'static str, Kind)>,
    name: Option<String>,
    parent: Ref,
) -> Result<Insertion, String> {
    let client = client()?;
    let content = client
        .asset(asset_id)
        .map_err(|err| refused(asset_id, &err))?;
    let (asset_type, kind) =
        Kind::of(&content, hint).ok_or_else(|| unsupported(asset_id, &content))?;
    let asset_name = name.unwrap_or_else(|| display_name(&client, asset_id, kind));
    let mut content = build(asset_id, kind, &content.bytes, &asset_name)?;
    let is_package = asset_type == "Package" || holds_package_link(&content);
    let guid = uuid();
    let added = tag(&guid);
    for root in content.root_refs().to_vec() {
        let mut tags: Vec<String> = content
            .get(root)
            .map(|instance| instance.tags().into_iter().map(str::to_owned).collect())
            .unwrap_or_default();
        tags.push(added.clone());
        let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
        let _ = content.set_property(root, "Tags", Variant::String(encode_tags(&tags)));
    }
    Ok(Insertion {
        asset_id,
        parent,
        content,
        asset_type: asset_type.to_owned(),
        asset_name,
        is_package,
        parent_warning: None,
        guid,
    })
}

/// What the Toolbox inserts for `asset_id`, with the asset's name. A model
/// asset's own roots, unwrapped: Studio's Toolbox drops them in as they are
/// (a MeshPart asset lands as the MeshPart, a model keeps its own name),
/// unlike `insert_asset`, which wraps them in a `Model` named after the
/// asset the way `InsertService:LoadAsset` does. Any other kind becomes the
/// instance `insert_asset` makes for it.
pub(crate) fn toolbox_fetch(asset_id: u64) -> Result<(WeakDom, String), String> {
    // Search works keyless, but the download route answers 401 without a key.
    let client = client().map_err(|_| {
        "Inserting needs an Open Cloud API key: add one from Home \u{203a} Manage key.".to_owned()
    })?;
    let content = client
        .asset(asset_id)
        .map_err(|err| refused(asset_id, &err))?;
    let (_, kind) = Kind::of(&content, None).ok_or_else(|| unsupported(asset_id, &content))?;
    let name = display_name(&client, asset_id, kind);
    let dom = match kind {
        Kind::Model => {
            read_model(&content.bytes).map_err(|err| format!("asset {asset_id}: {err}"))?
        }
        _ => build(asset_id, kind, &content.bytes, &name)?,
    };
    Ok((dom, name))
}

fn tag(guid: &str) -> String {
    format!("Assistant:{guid}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Any `.rbxm`/`.rbxmx` asset: a model, a package, a mesh part, an
    /// accessory.
    Model,
    Image,
    Mesh,
    Audio,
    Video,
    Animation,
}

impl Kind {
    /// `Enum.AssetType` as delivery reports it, with the name the answer
    /// gives it; else the call's hint, else the bytes.
    fn of(
        content: &AssetContent,
        hint: Option<(&'static str, Kind)>,
    ) -> Option<(&'static str, Kind)> {
        Some(match content.asset_type_id {
            Some(1) => ("Image", Kind::Image),
            Some(13) => ("Decal", Kind::Image),
            Some(3) => ("Audio", Kind::Audio),
            Some(4) => ("Mesh", Kind::Mesh),
            Some(10) => ("Model", Kind::Model),
            Some(40) => ("MeshPart", Kind::Model),
            Some(24 | 48..=56 | 61 | 78) => ("Animation", Kind::Animation),
            Some(62) => ("Video", Kind::Video),
            // A place, a plugin, a badge, a game pass, a font: nothing a
            // place holds as an instance.
            Some(5 | 9 | 21 | 34 | 38 | 73) => return None,
            _ if hint.is_some() => return hint,
            _ => {
                let kind = Kind::sniff(&content.bytes)?;
                let name = TYPES.iter().find(|(_, k)| *k == kind).map(|(n, _)| *n)?;
                (name, kind)
            }
        })
    }

    pub(super) fn sniff(bytes: &[u8]) -> Option<Kind> {
        use rbx_assets::AssetKind;
        match rbx_assets::sniff(bytes) {
            AssetKind::RobloxBinaryModel | AssetKind::RobloxXmlModel => Some(Kind::Model),
            AssetKind::Png
            | AssetKind::Jpeg
            | AssetKind::Webp
            | AssetKind::Bmp
            | AssetKind::Tga
            | AssetKind::Dds => Some(Kind::Image),
            AssetKind::RobloxMesh { .. } => Some(Kind::Mesh),
            AssetKind::Ogg | AssetKind::Mp3 => Some(Kind::Audio),
            AssetKind::Webm => Some(Kind::Video),
            AssetKind::Unknown => None,
        }
    }

    /// The class it becomes and the property naming the asset; `None` for
    /// what is inserted from its own file.
    fn instance(self) -> Option<(&'static str, &'static str)> {
        match self {
            Kind::Model | Kind::Animation => None,
            Kind::Image => Some(("Decal", "Texture")),
            Kind::Mesh => Some(("MeshPart", "MeshId")),
            Kind::Audio => Some(("Sound", "SoundId")),
            Kind::Video => Some(("VideoFrame", "Video")),
        }
    }
}

/// The asset's own name: the Creator Store's when it is listed there, the
/// Assets API's when the key owner made it, the class otherwise.
fn display_name(client: &Client, asset_id: u64, kind: Kind) -> String {
    client
        .creator_store_asset(asset_id)
        .map(|asset| asset.name)
        .or_else(|_| {
            client
                .asset_metadata(asset_id)
                .map(|meta| meta.display_name)
        })
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            match kind {
                Kind::Animation => "KeyframeSequence",
                _ => kind.instance().map_or("Model", |(class, _)| class),
            }
            .to_owned()
        })
}

fn refused(asset_id: u64, err: &CloudError) -> String {
    match err {
        CloudError::KeyedAssetRefused {
            why: KeyedRefusal::NoAccess,
            ..
        } => format!(
            "Asset {asset_id} is not one your account may use: asset delivery serves your own \
             assets, your groups', and what is free or owned from the Creator Store. ({err})"
        ),
        CloudError::Http { status: 404, .. } => {
            format!("Failed to fetch asset {asset_id}: HTTP 404 (Not Found)")
        }
        _ => describe(err, DELIVERY_SCOPE),
    }
}

fn unsupported(asset_id: u64, content: &AssetContent) -> String {
    match content.asset_type_id {
        Some(9) => format!("Asset {asset_id} is a place; open it from Home instead."),
        Some(38) => format!("Asset {asset_id} is a plugin; it is installed, not inserted."),
        Some(type_id) => format!(
            "Asset {asset_id} is of asset type {type_id}, which a place does not hold \
             (models, packages, meshes, images, audio, videos and animations are)."
        ),
        None => format!("Asset {asset_id} is in a format this editor does not recognise."),
    }
}
