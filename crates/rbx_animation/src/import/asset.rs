//! A published animation, fetched by its `AnimationId`. Roblox serves a
//! `KeyframeSequence` animation as an ordinary model file (what
//! `game:GetObjects("rbxassetid://…")` reads), so it is read like any other
//! model. The download itself is the caller's: the editor uses the same
//! anonymous-then-keyed route as every other asset. A `CurveAnimation` is
//! reported rather than guessed at, since this crate does not read curves.

use rbx_dom::{Ref, WeakDom};

use crate::clip::Clip;
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

/// Reads the first `KeyframeSequence` in a downloaded model file.
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
    if find(&dom, "CurveAnimation").is_some() {
        return Err(ImportError::Format(
            "this animation is a CurveAnimation, which the editor cannot read yet".into(),
        ));
    }
    Err(ImportError::Format(
        "the asset holds no KeyframeSequence".into(),
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
        dom.new_instance("CurveAnimation", "Emote", None);
        let err = clip_from_model(&rbx_binary::serialize(&dom).unwrap(), "x").unwrap_err();
        assert!(err.to_string().contains("CurveAnimation"));
        assert!(clip_from_model(b"junk", "x").is_err());
    }
}
