//! A `Content` row (`Decal.TextureContent`, `MeshPart.MeshContent`): an
//! asset URI field, or an object picked from the Explorer the way a `Ref`
//! row picks one (`Content.fromObject`, creator-docs `Content.yaml`).
//!
//! Both sources commit through the panel's one textual path, so an object is
//! spelled as text too: [`OBJECT_PREFIX`] and the target's referent. No asset
//! URI scheme is spelled that way (creator-docs `projects/assets` lists
//! `rbxassetid://`, `rbxasset://`, `rbxthumb://`, `rbxhttp://` and
//! `https://`), so the text alone says which source it is.

use rbx_dom::{Content, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

const OBJECT_PREFIX: &str = "object:";

/// The text an object pick commits.
pub(crate) fn object_text(target: Ref) -> String {
    format!("{OBJECT_PREFIX}{}", target.value())
}

/// The URI itself, an empty field for `None`, or [`object_text`] for an
/// object.
pub(super) fn content_text(content: &Content) -> String {
    match content {
        Content::None => String::new(),
        Content::Uri(uri) => uri.clone(),
        Content::Object(target) => object_text(*target),
    }
}

/// The class of object `name` on `class` may hold, or `None` where it holds
/// a URI only. Creator-docs names three, all runtime `Object`s rather than
/// `Instance`s: an `EditableImage` "can be used in any Content property
/// which takes an image" (`EditableImage.yaml`), `MeshPart.MeshContent`
/// "supports asset URIs and EditableMesh objects", and
/// `VideoFrame.VideoContent` takes a `VideoCapture`. The dump does not type
/// a Content property's object, so taking an image, a mesh or a video is
/// read off the property's name.
///
/// A legacy ContentId (`Decal.Texture`, `MeshPart.MeshId`) is saved as a
/// String, which has no spelling for an object (see `rbx_binary`'s
/// `unify_content_ids`); its `…Content` twin is where an object goes.
/// `TexturePackContent` is engine-managed packed data (`Decal.yaml`).
pub(crate) fn object_class(
    db: &ReflectionDatabase,
    class: &str,
    name: &str,
) -> Option<&'static str> {
    const IMAGE: [&str; 8] = [
        "Image",
        "Texture",
        "Map",
        "Icon",
        "Skybox",
        "Template",
        "Graphic",
        "EmissiveMask",
    ];
    if db.is_content_id(class, name) || name.contains("TexturePack") {
        None
    } else if name.contains("Video") {
        Some("VideoCapture")
    } else if name.contains("Mesh") {
        Some("EditableMesh")
    } else if IMAGE.iter().any(|word| name.contains(word)) {
        Some("EditableImage")
    } else {
        None
    }
}

/// Whether `target` may be what `name` on `class` names: an instance that
/// exists, of exactly the class [`object_class`] allows. Exact, because
/// none of the three has a subclass, and none is in the bundled dump for
/// `is_subclass_of` to walk. Studio's panel documents no object picker to
/// copy a refusal from, but Roblox says `Content.fromObject` will throw for
/// anything other than `EditableImage` and `EditableMesh` (devforum, "Major
/// updates to in-experience Mesh & Image APIs"), so this panel declines to
/// write it.
pub(super) fn check_object(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    class: &str,
    name: &str,
    target: Ref,
) -> Result<(), String> {
    let instance = dom
        .get(target)
        .ok_or_else(|| "that instance no longer exists".to_owned())?;
    let Some(wanted) = object_class(db, class, name) else {
        return Err(format!("{name} holds an asset URI, not an object"));
    };
    let got = instance.class();
    if got == wanted {
        return Ok(());
    }
    let article = if got.starts_with(['A', 'E', 'I', 'O', 'U']) {
        "an"
    } else {
        "a"
    };
    Err(format!("{name} takes an {wanted}, not {article} {got}"))
}

/// Follows Roblox's own two constructors (creator-docs, `Content.yaml`):
/// `Content.fromUri` returns `Content.none` for an empty string, and
/// `Content.fromAssetId(id)` is `fromUri("rbxassetid://" .. id)` with `0`
/// also meaning none. So an empty field clears the value, a bare number is
/// an asset id, and anything else is kept as the URI it was typed as —
/// asset URIs have several schemes (`rbxasset://`, `rbxthumb://`, plain
/// `https://`), and refusing the ones not listed here would refuse real
/// values. A number that is no asset id (negative, fractional, past `u64`)
/// is refused rather than kept as a URI nothing can load.
///
/// A file can hold an empty URI rather than none (rbx_xml reads an empty
/// `<url>` that way); an empty field leaves it as it is, so committing an
/// untouched row is no edit.
pub(super) fn parse_content(current: &Content, text: &str) -> Result<Variant, String> {
    let text = text.trim();
    if let Some(id) = text.strip_prefix(OBJECT_PREFIX) {
        return id
            .parse::<u32>()
            .map(|id| Variant::Content(Content::Object(Ref::new(id))))
            .map_err(|_| format!("{text:?} is not an instance"));
    }
    let content = match text.parse::<u64>() {
        Ok(0) => Content::None,
        Ok(id) => Content::Uri(format!("rbxassetid://{id}")),
        Err(_) if text.is_empty() => match current {
            Content::Uri(uri) if uri.is_empty() => current.clone(),
            _ => Content::None,
        },
        Err(_) if text.parse::<f64>().is_ok() => {
            return Err(format!("{text:?} is not an asset id"))
        }
        Err(_) => Content::Uri(text.to_owned()),
    };
    Ok(Variant::Content(content))
}

/// A file stores a legacy ContentId (`Decal.Texture` in most places) as a
/// String; an edit of one reads the text as [`parse_content`] does and keeps
/// the String shape, so the instance still matches its siblings when saved.
pub(super) fn parse_content_id(stored: &str, text: &str) -> Result<Variant, String> {
    let uri = match parse_content(&Content::Uri(stored.to_owned()), text)? {
        Variant::Content(Content::Uri(uri)) => uri,
        Variant::Content(Content::Object(_)) => {
            return Err("a ContentId holds an asset URI, not an object".to_owned())
        }
        _ => String::new(),
    };
    Ok(Variant::String(uri))
}

#[cfg(test)]
mod tests;
