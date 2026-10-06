//! Legacy ContentId properties (`Decal.Texture`, `MeshPart.MeshId`): saved as
//! a String, read as `""` when unset, and given their default only under a
//! newer `Content` twin (`TextureContent`, `MeshContent`).
//!
//! Release 645 split the two: a dump from before it spells every ContentId
//! `Content` and lists none of the true Content properties, while the bundled
//! one, like any newer dump, spells them `ContentId`. Whichever spelling the
//! dump uses is the one matched here.

use rbx_dom::{Content, Variant};

use crate::class::ClassDescriptor;
use crate::database::ReflectionDatabase;

/// The dump's name for a ContentId: `ContentId` once it has the split,
/// `Content` before it.
pub(crate) fn spelling<'a>(classes: impl IntoIterator<Item = &'a ClassDescriptor>) -> &'static str {
    let split = classes
        .into_iter()
        .flat_map(|class| &class.properties)
        .any(|property| property.value_type == "ContentId");
    if split {
        "ContentId"
    } else {
        "Content"
    }
}

/// Short names the sky's ContentIds use for its faces, by twin.
const SKY_FACES: [(&str, &str); 6] = [
    ("SkyboxBk", "SkyboxBack"),
    ("SkyboxDn", "SkyboxDown"),
    ("SkyboxFt", "SkyboxFront"),
    ("SkyboxLf", "SkyboxLeft"),
    ("SkyboxRt", "SkyboxRight"),
    ("SkyboxUp", "SkyboxUp"),
];

impl ReflectionDatabase {
    /// Whether `class` (or a superclass) declares `name` as a ContentId.
    pub fn is_content_id(&self, class: &str, name: &str) -> bool {
        self.resolve_property(class, name)
            .is_some_and(|property| property.value_type == self.content_id_type)
    }

    /// What an unset ContentId holds on a fresh `class`, as the String it is
    /// saved as: its `Content` twin's default URI where the class records
    /// one (`ParticleEmitter.Texture` is the sparkles texture), else `""`.
    /// `None` for any other property.
    ///
    /// The twin is found by name, as `<name>Content`, `<name minus Id>Content`
    /// or a sky face's long name; a ContentId with no twin under any of them
    /// (`Sound.SoundId`, `Shirt.ShirtTemplate`) has an empty default.
    pub fn content_id_default(&self, class: &str, name: &str) -> Option<Variant> {
        if !self.is_content_id(class, name) {
            return None;
        }
        let stem = name
            .strip_suffix("Id")
            .or_else(|| name.strip_suffix("ID"))
            .unwrap_or(name);
        let sky = SKY_FACES
            .iter()
            .find(|(short, _)| *short == name)
            .map(|(_, long)| *long);
        let uri = [Some(name), Some(stem), sky]
            .into_iter()
            .flatten()
            .find_map(
                |base| match self.default_value(class, &format!("{base}Content")) {
                    Some(Variant::Content(Content::Uri(uri))) => Some(uri.clone()),
                    Some(Variant::Content(_)) => Some(String::new()),
                    _ => None,
                },
            )
            .unwrap_or_default();
        Some(Variant::String(uri))
    }
}

#[cfg(all(test, feature = "embedded-dump"))]
mod tests {
    use super::*;

    fn default(class: &str, name: &str) -> Option<Variant> {
        ReflectionDatabase::shared().content_id_default(class, name)
    }

    fn string(text: &str) -> Option<Variant> {
        Some(Variant::String(text.to_owned()))
    }

    #[test]
    fn legacy_asset_ids_are_content_ids_and_their_content_twins_are_not() {
        let db = ReflectionDatabase::shared();
        assert!(db.is_content_id("Decal", "Texture"));
        assert!(db.is_content_id("MeshPart", "MeshId"));
        // Declared on `Decal`, inherited by `Texture`.
        assert!(db.is_content_id("Texture", "Texture"));
        assert!(!db.is_content_id("Decal", "TextureContent"));
        assert!(!db.is_content_id("Decal", "Transparency"));
        assert!(!db.is_content_id("NotARealClass", "Texture"));
    }

    #[test]
    fn an_unset_content_id_takes_its_twins_default() {
        assert_eq!(
            default("ParticleEmitter", "Texture"),
            string("rbxasset://textures/particles/sparkles_main.dds")
        );
        assert!(matches!(
            default("Sky", "SkyboxBk"),
            Some(Variant::String(uri)) if uri.contains("sky512_bk")
        ));
        assert!(matches!(
            default("Sky", "SunTextureId"),
            Some(Variant::String(uri)) if uri.contains("sun")
        ));
        assert_eq!(default("Decal", "Texture"), string(""));
        assert_eq!(default("Sound", "SoundId"), string(""));
        assert_eq!(default("Decal", "Transparency"), None);
    }
}
