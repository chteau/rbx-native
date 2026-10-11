//! Unsaved Animation Editor work kept on disk: the clip and graph being
//! edited, written beside the editor's other local state whenever they
//! change unsaved, read back the next time the place's rig is opened, and
//! deleted once they are saved into the place. It is one small `.rbxm` per
//! place (a `KeyframeSequence` and an `AnimationGraphDefinition` in a
//! `Folder` that names the rig), so a draft is also an ordinary model file
//! anyone can open.

use std::path::{Path, PathBuf};

use rbx_animation::graph::{self, Graph};
use rbx_animation::{sequence, Clip};
use rbx_dom::{Variant, WeakDom};

const ROOT: &str = "AnimationDraft";
const RIG: &str = "Rig";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Draft {
    /// The name of the rig the work was made on.
    pub(crate) rig: String,
    pub(crate) clip: Option<Clip>,
    pub(crate) graph: Option<Graph>,
}

/// Where drafts live: `<config>/animation-drafts`.
pub(crate) fn folder() -> Option<PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("animation-drafts"))
}

/// The draft file for `place`, named after it with a hash of its full path
/// so two places of the same name keep separate drafts.
pub(crate) fn path(folder: &Path, place: &Path) -> PathBuf {
    let stem = place
        .file_stem()
        .map_or_else(|| "place".into(), |s| s.to_string_lossy());
    let hash = crate::recovery::fnv1a(place.to_string_lossy().as_bytes()) as u32;
    folder.join(format!("{stem} (animation draft {hash:08x}).rbxm"))
}

pub(crate) fn write(file: &Path, draft: &Draft) -> std::io::Result<()> {
    let mut dom = WeakDom::new();
    let root = dom.new_instance("Folder", ROOT, None);
    let rig = dom.new_instance("StringValue", RIG, Some(root));
    let _ = dom.set_property(rig, "Value", Variant::String(draft.rig.clone()));
    if let Some(clip) = &draft.clip {
        sequence::write(&mut dom, Some(root), clip, None);
    }
    if let Some(g) = &draft.graph {
        graph::dom::write(&mut dom, Some(root), g);
    }
    let bytes = rbx_binary::serialize(&dom).map_err(std::io::Error::other)?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Written whole then renamed, so a crash mid-write never leaves half a
    // draft where a good one was.
    let temp = file.with_extension("rbxm.part");
    std::fs::write(&temp, bytes)?;
    std::fs::rename(temp, file)
}

pub(crate) fn read(file: &Path) -> Option<Draft> {
    let dom = rbx_binary::deserialize(&std::fs::read(file).ok()?).ok()?;
    let root = *dom.root_refs().first()?;
    let children = dom.get(root)?.children().to_vec();
    let of_class = |class: &str| {
        children
            .iter()
            .copied()
            .find(|&c| dom.get(c).is_some_and(|i| i.class() == class))
    };
    let rig = children
        .iter()
        .copied()
        .find(|&c| dom.get(c).is_some_and(|i| i.name() == RIG))
        .and_then(|r| match dom.get(r)?.properties().get("Value")? {
            Variant::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default();
    Some(Draft {
        rig,
        clip: of_class("KeyframeSequence").map(|s| sequence::read(&dom, s)),
        graph: of_class("AnimationGraphDefinition").map(|g| graph::dom::read(&dom, g)),
    })
}

pub(crate) fn clear(file: &Path) {
    let _ = std::fs::remove_file(file);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_animation::{Frame, PoseKey};

    #[test]
    fn a_draft_round_trips_and_clears() {
        let dir = std::env::temp_dir().join(format!("rbx_anim_draft_{}", std::process::id()));
        let file = path(&dir, Path::new("/places/Obby.rbxl"));
        assert!(file.to_string_lossy().contains("Obby (animation draft "));
        let mut clip = Clip {
            name: "Wave".into(),
            ..Clip::default()
        };
        clip.set_pose(
            0.5,
            "RightUpperArm",
            PoseKey::new(Frame::angles(0., 0., 1.)),
        );
        let draft = Draft {
            rig: "Rig".into(),
            clip: Some(clip),
            graph: Some(Graph::default()),
        };
        write(&file, &draft).unwrap();
        let back = read(&file).unwrap();
        assert_eq!(back.rig, "Rig");
        assert_eq!(back.clip.unwrap().joints(), ["RightUpperArm"]);
        assert!(back.graph.unwrap().output().is_some());
        clear(&file);
        assert!(read(&file).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn places_of_one_name_keep_separate_drafts() {
        let dir = Path::new("/drafts");
        assert_ne!(
            path(dir, Path::new("/a/Place.rbxl")),
            path(dir, Path::new("/b/Place.rbxl"))
        );
    }
}
