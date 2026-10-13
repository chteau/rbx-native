//! A published animation, fetched by its `AnimationId`. Roblox serves a
//! `KeyframeSequence` animation as an ordinary model file (what
//! `game:GetObjects("rbxassetid://…")` reads), so it is read like any other
//! model. The download itself is the caller's: the editor uses the same
//! anonymous-then-keyed route as every other asset. A `CurveAnimation` (what
//! catalog emotes are) is read through [`crate::curve::file`] and baked to a
//! clip.

use rbx_dom::{Ref, WeakDom};

use crate::clip::Clip;
use crate::curve::file;
use crate::import::ImportError;
use crate::sequence;

/// The asset id in `rbxassetid://123`, a catalog or Creator Store URL, or a
/// bare number.
pub fn parse_id(text: &str) -> Option<u64> {
    let digits: String = text
        .trim()
        .trim_start_matches("rbxassetid://")
        .split(|c: char| !c.is_ascii_digit())
        .find(|run| run.len() >= 3)?
        .to_owned();
    digits.parse().ok()
}

/// The asset id an `AnimationId` property names, strictly: a bare number,
/// `rbxassetid://N`, or a `roblox.com` asset URL carrying `id=N`. Unlike
/// [`parse_id`], a clip *name* such as "Walk2" is not an id.
pub fn content_id(text: &str) -> Option<u64> {
    let lower = text.trim().to_ascii_lowercase();
    let digits = if let Some(rest) = lower.strip_prefix("rbxassetid://") {
        rest
    } else if lower.bytes().all(|b| b.is_ascii_digit()) {
        &lower
    } else {
        let rest = lower
            .strip_prefix("https://")
            .or_else(|| lower.strip_prefix("http://"))?;
        let rest = [
            "www.roblox.com/asset",
            "roblox.com/asset",
            "assetdelivery.roblox.com/v1/asset",
        ]
        .iter()
        .find_map(|path| rest.strip_prefix(path))?;
        let query = rest.strip_prefix('/').unwrap_or(rest).strip_prefix('?')?;
        query
            .split('&')
            .find_map(|param| param.strip_prefix("id="))?
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|&id| id != 0)
}

fn find(dom: &WeakDom, class: &str) -> Option<Ref> {
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let instance = dom.get(r)?;
        if instance.class() == class {
            return Some(r);
        }
        stack.extend(instance.children().iter().rev());
    }
    None
}

/// Frames per second a `CurveAnimation`'s cubic segments are baked at.
const CURVE_BAKE_FPS: f32 = 30.;

/// Reads the first `KeyframeSequence` in a downloaded model file, or failing
/// that its first `CurveAnimation`.
pub fn clip_from_model(bytes: &[u8], name: &str) -> Result<Clip, ImportError> {
    let dom = if rbx_xml::is_xml(bytes) {
        let text =
            std::str::from_utf8(bytes).map_err(|_| ImportError::Format("not UTF-8 XML".into()))?;
        rbx_xml::deserialize(text).map_err(|e| ImportError::Format(e.to_string()))?
    } else {
        rbx_binary::deserialize(bytes).map_err(|e| ImportError::Format(e.to_string()))?
    };
    if let Some(seq) = find(&dom, "KeyframeSequence") {
        let mut clip = sequence::read(&dom, seq);
        if clip.name.is_empty() || clip.name == "KeyframeSequence" {
            clip.name = name.to_owned();
        }
        return Ok(clip);
    }
    if let Some(r) = find(&dom, "CurveAnimation") {
        let curves = file::read(&dom, r).map_err(ImportError::Format)?;
        let mut clip = file::bake(&curves, CURVE_BAKE_FPS);
        if clip.name.is_empty() || clip.name == "CurveAnimation" {
            clip.name = name.to_owned();
        }
        return Ok(clip);
    }
    Err(ImportError::Format(
        "the asset holds no KeyframeSequence or CurveAnimation".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::PoseKey;
    use crate::frame::Frame;

    #[test]
    fn parses_ids_in_every_shape_builders_paste() {
        assert_eq!(
            parse_id("rbxassetid://140610898812528"),
            Some(140610898812528)
        );
        assert_eq!(parse_id(" 507770239 "), Some(507770239));
        assert_eq!(
            parse_id("https://create.roblox.com/store/asset/140610898812528/Walk"),
            Some(140610898812528)
        );
        assert_eq!(parse_id("walk"), None);
    }

    #[test]
    fn content_id_accepts_every_property_shape() {
        for text in [
            "507777826",
            "  507777826 ",
            "rbxassetid://507777826",
            "RBXASSETID://507777826",
            "http://www.roblox.com/asset/?id=507777826",
            "https://www.roblox.com/asset?id=507777826",
            "HTTPS://WWW.ROBLOX.COM/asset/?id=507777826",
            "https://roblox.com/asset/?id=507777826",
            "https://assetdelivery.roblox.com/v1/asset/?id=507777826",
            "https://www.roblox.com/asset/?id=507777826&version=2",
            "https://www.roblox.com/asset/?version=2&id=507777826",
        ] {
            assert_eq!(content_id(text), Some(507777826), "{text}");
        }
    }

    #[test]
    fn content_id_rejects_names_and_malformed_values() {
        for text in [
            "",
            "   ",
            "Walk",
            "Walk2",
            "idle 3",
            "rbxassetid://",
            "rbxassetid://12abc",
            "0",
            "rbxassetid://0",
            "99999999999999999999999",
            "https://example.com/asset/?id=507777826",
            "https://www.roblox.com/asset/?id=",
            "https://www.roblox.com/asset/?id=12abc",
            "https://www.roblox.com/asset/",
        ] {
            assert_eq!(content_id(text), None, "{text}");
        }
    }

    #[test]
    fn reads_the_sequence_out_of_binary_and_xml_models() {
        let mut clip = Clip {
            name: "Wave".into(),
            ..Clip::default()
        };
        clip.set_pose(
            0.5,
            "RightUpperArm",
            PoseKey::new(Frame::angles(0., 0., 1.)),
        );
        let mut dom = WeakDom::new();
        sequence::write(&mut dom, None, &clip, None);
        let binary = rbx_binary::serialize(&dom).unwrap();
        let xml = rbx_xml::serialize(&dom).unwrap();
        for bytes in [binary, xml.into_bytes()] {
            let back = clip_from_model(&bytes, "Asset").unwrap();
            assert_eq!(back.name, "Wave");
            assert_eq!(back.joints(), ["RightUpperArm"]);
        }
    }

    #[test]
    fn says_what_it_cannot_read() {
        let mut dom = WeakDom::new();
        dom.new_instance("Folder", "Empty", None);
        let err = clip_from_model(&rbx_binary::serialize(&dom).unwrap(), "x").unwrap_err();
        assert!(err.to_string().contains("CurveAnimation"));
        assert!(clip_from_model(b"junk", "x").is_err());
    }
}
