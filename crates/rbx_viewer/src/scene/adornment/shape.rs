//! The `HandleAdornment` shapes: a box, a sphere, a cylinder, a cone, a line
//! and an image, each placed by the frame [`Context::handle_frame`] works
//! out from the adornee, `SizeRelativeOffset` and the adornment's own
//! `CFrame`.

use glam::{Vec2, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::Variant;
use rbx_reflection::ReflectionDatabase;

use super::{Context, Line, Mesh, Picture, Piece};
use crate::scene::props::{float_or, vector3_or};
use crate::textures::asset_uri;

/// A full cylinder: `Angle` is documented as cutting a "pie slice" sector
/// out of one, so a file that carries no angle at all (or a zero one, which
/// would be a slice with no width) draws the whole thing.
const FULL_TURN: f32 = 360.0;

/// The pieces `class` draws, or `None` when it is not one of these shapes at
/// all — the caller's cue to try the other adornment families.
pub(super) fn pieces(
    database: &ReflectionDatabase,
    class: &str,
    context: &Context<'_>,
) -> Option<Vec<Piece>> {
    let is = |ancestor: &str| database.is_subclass_of(class, ancestor);
    let properties = context.properties;
    let frame = context.handle_frame();

    if is("BoxHandleAdornment") {
        let size = vector3_or(properties, "Size", Vec3::ONE);
        return Some(vec![context.solid(Mesh::Box { size }, frame)]);
    }
    if is("SphereHandleAdornment") {
        let radius = float_or(properties, "Radius", 1.0).max(0.0);
        return Some(vec![context.solid(Mesh::Sphere { radius }, frame)]);
    }
    if is("CylinderHandleAdornment") {
        let radius = float_or(properties, "Radius", 1.0).max(0.0);
        let sweep = match float_or(properties, "Angle", 0.0) {
            angle if angle <= 0.0 || angle >= FULL_TURN => FULL_TURN,
            angle => angle,
        };
        return Some(vec![context.solid(
            Mesh::Cylinder {
                radius,
                inner: float_or(properties, "InnerRadius", 0.0).clamp(0.0, radius),
                height: float_or(properties, "Height", 1.0).max(0.0),
                sweep,
            },
            frame,
        )]);
    }
    if is("ConeHandleAdornment") {
        return Some(vec![context.solid(
            Mesh::Cone {
                radius: float_or(properties, "Radius", 1.0).max(0.0),
                height: float_or(properties, "Height", 1.0).max(0.0),
            },
            frame,
        )]);
    }
    if is("LineHandleAdornment") {
        // Documented in studs along the handle's own length, and in pixels
        // across it.
        let length = float_or(properties, "Length", 1.0);
        let from = frame.w_axis.truncate();
        return Some(vec![Piece::Line(Line {
            from,
            to: from - frame.z_axis.truncate().normalize_or_zero() * length,
            pixels: float_or(properties, "Thickness", 1.0).max(0.0),
            color: context.color,
            alpha: context.alpha,
        })]);
    }
    if is("ImageHandleAdornment") {
        let texture = image_of(context)?;
        let size = match properties.get("Size") {
            Some(Variant::Vector2(value)) => Vec2::new(value.x, value.y),
            _ => Vec2::ONE,
        };
        return Some(vec![Piece::Picture(Picture {
            frame,
            size,
            texture,
            alpha: context.alpha,
        })]);
    }
    if is("WireframeHandleAdornment") {
        // Every line a wireframe adornment holds arrives through `AddLine`,
        // `AddLines` or `AddPath` at runtime; the instance itself serializes
        // no geometry at all, so a place file's one has nothing to draw.
        // Recognized rather than left unhandled, so it is not mistaken for a
        // class this plan forgot.
        return Some(Vec::new());
    }
    None
}

/// `ImageContent` (the `Content` property Roblox moved to) or the older
/// `Image` string, whichever the file carries.
fn image_of(context: &Context<'_>) -> Option<AssetRef> {
    ["ImageContent", "Image"]
        .into_iter()
        .filter_map(|key| context.properties.get(key))
        .filter_map(asset_uri)
        .filter_map(|uri| AssetRef::parse(uri).ok())
        .find(|reference| *reference != AssetRef::Empty)
}
