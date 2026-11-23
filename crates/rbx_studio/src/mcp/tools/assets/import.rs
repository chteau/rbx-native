//! `import_model`, ours: a local `.glb`/`.gltf`/`.obj`/`.fbx`, or a free
//! kit's model by provider and id, through the Toolbox's Free kits import
//! (`crate::kits`): uploaded as Import 3D uploads, then one `Model` scaled
//! to studs, its pivot on its base centre at `position`, and credited. The
//! shell grafts it like an `insert_asset`, and the answer names its path.

use std::path::PathBuf;

use glam::Vec3;
use serde_json::Value;

use super::super::{text_argument, Outcome};
use super::insert::{parent, tagged};
use crate::kits::{self, Provider};
use crate::mcp::{PlayState, Session};

pub(super) const NAME: &str = "import_model";

enum Source {
    File(PathBuf),
    Kit(Provider, String),
}

pub(super) fn run(arguments: &Value, session: &mut Session) -> Outcome {
    if session.studio.play == PlayState::ShowingLive {
        return Err(
            "The editor is showing a running playtest; switch back to the edited place first."
                .to_owned(),
        );
    }
    let source = match (
        text_argument(arguments, "path"),
        text_argument(arguments, "provider"),
        text_argument(arguments, "id"),
    ) {
        (Some(path), _, _) => {
            let path = PathBuf::from(path.trim());
            if rbx_import::Format::of(&path).is_none() {
                return Err(format!(
                    "{} is not a .glb, .gltf, .obj or .fbx file",
                    path.display()
                ));
            }
            Source::File(path)
        }
        (None, Some(provider), Some(id)) => {
            let provider = Provider::parse(provider).ok_or_else(|| {
                format!("unknown provider {provider}: use \"polyhaven\" or \"local\"")
            })?;
            Source::Kit(provider, id.trim().to_owned())
        }
        _ => return Err("give `path`, or `provider` and `id`".to_owned()),
    };
    let scale = match arguments.get("scale") {
        None => None,
        Some(value) => Some(
            value
                .as_f64()
                .map(|scale| scale as f32)
                .filter(|scale| scale.is_finite() && *scale > 0.0)
                .ok_or("`scale` must be a positive number of studs per file unit")?,
        ),
    };
    let position = match arguments.get("position") {
        None => Vec3::ZERO,
        Some(value) => position(value).ok_or("`position` must be [x, y, z] in studs")?,
    };
    let (parent, parent_warning) = parent(arguments, session.dom)?;
    session.inserting = Some(Box::new(move || {
        let resolved = match source {
            Source::File(path) => kits::from_file(&path),
            Source::Kit(provider, id) => kits::resolve(provider, &id).map_err(|err| {
                match kits::search(provider, &id, 10)[..] {
                    [] => err,
                    ref close => format!("{err}. Close matches: {}", close.join(", ")),
                }
            })?,
        };
        let mut prepared = kits::import(&resolved, scale)?;
        kits::translate(&mut prepared.dom, &[prepared.root], position);
        let mut insertion = tagged(
            prepared.asset_id,
            parent,
            prepared.dom,
            "Model".to_owned(),
            resolved.name,
            false,
        );
        insertion.parent_warning = match (parent_warning, prepared.note) {
            (Some(a), Some(b)) => Some(format!("{a} {b}")),
            (a, b) => a.or(b),
        };
        Ok(insertion)
    }));
    Ok(Value::Null)
}

fn position(value: &Value) -> Option<Vec3> {
    match value.as_array()?.as_slice() {
        [x, y, z] => Some(Vec3::new(
            x.as_f64()? as f32,
            y.as_f64()? as f32,
            z.as_f64()? as f32,
        )),
        _ => None,
    }
}
