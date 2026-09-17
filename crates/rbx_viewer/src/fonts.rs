//! Roblox font families: what a `FontFace` names, how a family's JSON lists
//! its faces, and which face answers a weight/style request.
//!
//! Shared by the loader (which face file to fetch — see
//! `load::Loaded::resolve_fonts`) and the renderer (which bytes to hand the
//! font system — see `renderer::gui::text`), so the two can never disagree on
//! the face. Fonts are Roblox content the repository never ships: every byte
//! here arrives through the asset pipeline at run time.

use std::collections::HashMap;
use std::sync::Arc;

use rbx_assets::AssetRef;

/// `Enum.FontWeight.Regular`, and the weight a `FontFace` with none is read at.
pub(crate) const REGULAR: u16 = 400;

/// The family a text object built in code falls back to. Roblox's docs give
/// no default for `FontFace`; Studio's Properties window shows `SourceSansPro`
/// for a fresh `TextLabel`, which is what this reproduces.
pub(crate) const DEFAULT_FAMILY: &str = "SourceSansPro";

/// One face request: a family and the weight/style asked of it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Face {
    /// The family's JSON, `rbxasset://fonts/families/<Name>.json`.
    pub(crate) family: AssetRef,
    pub(crate) weight: u16,
    pub(crate) italic: bool,
}

impl Face {
    /// A face of one of Roblox's own families, by the family's file name.
    pub(crate) fn named(family: &str, weight: u16, italic: bool) -> Self {
        Face {
            family: AssetRef::Native(format!("fonts/families/{family}.json")),
            weight,
            italic,
        }
    }
}

impl Default for Face {
    fn default() -> Self {
        Face::named(DEFAULT_FAMILY, REGULAR, false)
    }
}

/// One entry of a family's `faces` list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub(crate) weight: u16,
    pub(crate) italic: bool,
    /// The face file: `rbxasset://fonts/<File>` or `rbxassetid://<id>`.
    pub(crate) asset: AssetRef,
}

/// A family as its JSON describes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Family {
    /// The typographic name (`"Gotham SSm"`), which is not the file name.
    pub(crate) name: String,
    pub(crate) faces: Vec<Entry>,
}

impl Family {
    /// `{"name","faces":[{"name","weight","style","assetId"}]}`; a face whose
    /// asset does not parse is dropped, a file that is not that shape at all
    /// is `None`.
    pub(crate) fn parse(json: &[u8]) -> Option<Family> {
        let value: serde_json::Value = serde_json::from_slice(json).ok()?;
        let name = value.get("name")?.as_str()?.to_string();
        let faces = value
            .get("faces")?
            .as_array()?
            .iter()
            .filter_map(|face| {
                Some(Entry {
                    weight: u16::try_from(face.get("weight")?.as_u64()?).ok()?,
                    italic: face.get("style")?.as_str()? == "italic",
                    asset: AssetRef::parse(face.get("assetId")?.as_str()?).ok()?,
                })
            })
            .collect();
        Some(Family { name, faces })
    }

    /// The face nearest a request. Roblox's docs do not say how a weight the
    /// family lacks is served; this takes the smallest weight difference among
    /// faces of the requested style (a bolder face winning a tie), and only
    /// falls back to the other style when the family has none of the
    /// requested one — so a bold request on a family with a single regular
    /// face still gets that face rather than nothing.
    pub(crate) fn closest(&self, weight: u16, italic: bool) -> Option<&Entry> {
        let distance =
            |face: &&Entry| (face.weight.abs_diff(weight), std::cmp::Reverse(face.weight));
        self.faces
            .iter()
            .filter(|face| face.italic == italic)
            .min_by_key(distance)
            .or_else(|| self.faces.iter().min_by_key(distance))
    }
}

/// Everything the loader has answered so far, keyed the way a request names
/// it: family JSONs by their reference, face files by theirs. A reference
/// absent from both is still on its way or failed for good.
#[derive(Debug, Default, Clone)]
pub(crate) struct Library {
    pub(crate) families: HashMap<AssetRef, Family>,
    pub(crate) faces: HashMap<AssetRef, Arc<Vec<u8>>>,
}

impl Library {
    /// The face file `face` resolves to, once its family JSON is in.
    pub(crate) fn entry_of(&self, face: &Face) -> Option<&Entry> {
        self.families
            .get(&face.family)?
            .closest(face.weight, face.italic)
    }

    /// The face file's bytes, once both stages of the fetch have landed.
    pub(crate) fn bytes_of(&self, face: &Face) -> Option<(&AssetRef, &Arc<Vec<u8>>)> {
        let asset = &self.entry_of(face)?.asset;
        Some((asset, self.faces.get(asset)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOTHAM: &[u8] = br#"{
        "name": "Gotham SSm",
        "faces": [
            {"name": "Book", "weight": 400, "style": "normal", "assetId": "rbxasset://fonts/GothamSSm-Book.otf"},
            {"name": "Bold", "weight": 700, "style": "normal", "assetId": "rbxasset://fonts/GothamSSm-Bold.otf"},
            {"name": "Bold Italic", "weight": 700, "style": "italic", "assetId": "rbxassetid://12345"},
            {"name": "Black", "weight": 900, "style": "normal", "assetId": "rbxasset://fonts/GothamSSm-Black.otf"}
        ]
    }"#;

    fn gotham() -> Family {
        Family::parse(GOTHAM).expect("the fixture parses")
    }

    #[test]
    fn a_family_json_parses_its_name_and_every_face() {
        let family = gotham();

        assert_eq!(family.name, "Gotham SSm");
        assert_eq!(family.faces.len(), 4);
        assert_eq!(
            family.faces[2],
            Entry {
                weight: 700,
                italic: true,
                asset: AssetRef::Id(12345),
            }
        );
        assert_eq!(
            family.faces[0].asset,
            AssetRef::Native("fonts/GothamSSm-Book.otf".to_string())
        );
    }

    #[test]
    fn a_file_that_is_not_a_family_is_none() {
        assert!(Family::parse(b"not json").is_none());
        assert!(Family::parse(b"{\"name\": \"x\"}").is_none());
    }

    #[test]
    fn the_exact_face_wins_and_a_missing_weight_takes_the_nearest() {
        let family = gotham();

        assert_eq!(family.closest(700, false).unwrap().weight, 700);
        // 500 is 100 from 400 and 200 from 700.
        assert_eq!(family.closest(500, false).unwrap().weight, 400);
        // 800 sits exactly between 700 and 900: the bolder face wins the tie.
        assert_eq!(family.closest(800, false).unwrap().weight, 900);
    }

    #[test]
    fn style_is_matched_before_weight_and_only_then_given_up() {
        let family = gotham();

        // The only italic face is the bold one, whatever weight was asked.
        assert_eq!(
            family.closest(400, true).unwrap().asset,
            AssetRef::Id(12345)
        );

        // A family with no italic at all answers with its upright face —
        // `FredokaOne`'s single regular face serves a 700 request too.
        let single = Family::parse(
            br#"{"name": "Fredoka One", "faces": [
                {"name": "Regular", "weight": 400, "style": "normal", "assetId": "rbxasset://fonts/FredokaOne-Regular.ttf"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(single.closest(700, true).unwrap().weight, 400);
        assert!(Family {
            name: String::new(),
            faces: Vec::new()
        }
        .closest(400, false)
        .is_none());
    }

    #[test]
    fn a_library_answers_only_once_both_stages_are_in() {
        let face = Face::named("GothamSSm", 700, false);
        let mut library = Library::default();
        assert!(library.bytes_of(&face).is_none());

        library.families.insert(face.family.clone(), gotham());
        let bold = AssetRef::Native("fonts/GothamSSm-Bold.otf".to_string());
        assert_eq!(library.entry_of(&face).unwrap().asset, bold);
        assert!(library.bytes_of(&face).is_none());

        library.faces.insert(bold.clone(), Arc::new(vec![1, 2, 3]));
        let (asset, bytes) = library.bytes_of(&face).unwrap();
        assert_eq!(asset, &bold);
        assert_eq!(bytes.as_slice(), &[1, 2, 3]);
    }
}
