//! The `HandleAdornment` shapes: a box, a sphere, a cylinder, a cone, a line
//! and an image, each placed by the frame [`Context::handle_frame`] works
//! out from the adornee, `SizeRelativeOffset` and the adornment's own
//! `CFrame`.

use glam::{Vec2, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::Variant;
use rbx_reflection::ReflectionDatabase;

use super::{Context, Line, Mesh, Picture, Piece, Solid};
use crate::scene::props::{bool_or, float_or, vector3_or};
use crate::textures::asset_uri;

/// A full cylinder: `Angle` is documented as cutting a "pie slice" sector
/// out of one, so a file that carries no angle at all (or a zero one, which
/// would be a slice with no width) draws the whole thing.
const FULL_TURN: f32 = 360.0;

/// The pieces `class` draws, or `None` when it is not one of these shapes at
/// all, the caller's cue to try the other adornment families.
pub(super) fn pieces(
    database: &ReflectionDatabase,
    class: &str,
    context: &Context<'_>,
) -> Option<Vec<Piece>> {
    let is = |ancestor: &str| database.is_subclass_of(class, ancestor);
    let properties = context.properties;
    let frame = context.handle_frame();
    let double_sided = bool_or(properties, "DoubleSided", false);
    let solid = |mesh: Mesh| {
        Piece::Solid(Solid {
            mesh,
            frame,
            color: context.color,
            alpha: context.alpha,
            double_sided,
        })
    };

    if is("BoxHandleAdornment") {
        let size = vector3_or(properties, "Size", Vec3::ONE);
        return Some(vec![solid(Mesh::Box { size })]);
    }
    if is("SphereHandleAdornment") {
        let radius = float_or(properties, "Radius", 1.0).max(0.0);
        return Some(vec![solid(Mesh::Sphere { radius })]);
    }
    if is("CylinderHandleAdornment") {
        let radius = float_or(properties, "Radius", 1.0).max(0.0);
        let sweep = match float_or(properties, "Angle", 0.0) {
            angle if angle <= 0.0 || angle >= FULL_TURN => FULL_TURN,
            angle => angle,
        };
        return Some(vec![solid(Mesh::Cylinder {
            radius,
            inner: float_or(properties, "InnerRadius", 0.0).clamp(0.0, radius),
            height: float_or(properties, "Height", 1.0).max(0.0),
            sweep,
        })]);
    }
    if is("ConeHandleAdornment") {
        return Some(vec![solid(Mesh::Cone {
            radius: float_or(properties, "Radius", 1.0).max(0.0),
            height: float_or(properties, "Height", 1.0).max(0.0),
        })]);
    }
    if is("PyramidHandleAdornment") {
        // Studio 0.748 clamps `Sides` to 3..=32 and takes `Size` and
        // `Height` as given, zero and negative included; a zero `Size` draws
        // nothing there.
        let size = float_or(properties, "Size", 1.0);
        if size == 0.0 {
            return Some(Vec::new());
        }
        let sides = match properties.get("Sides") {
            Some(Variant::Int32(sides)) => *sides,
            _ => 4,
        };
        return Some(vec![solid(Mesh::Pyramid {
            sides: sides.clamp(3, 32) as u32,
            size,
            height: float_or(properties, "Height", 2.0),
        })]);
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
            alpha: line_alpha(context.alpha),
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
            color: context.color,
            alpha: context.alpha,
            double_sided,
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

/// `Transparency` against the opacity a `LineHandleAdornment` actually
/// draws with, measured in Studio 2026-11-24 over flat white, grey and
/// black (both `AlwaysOnTop` and depth-tested agree to 0.006): a line fades
/// much faster than every other shape, which draws at exactly
/// `1 - Transparency`. Piecewise linear between the measured points.
const LINE_OPACITY: [(f32, f32); 6] = [
    (0.0, 1.0),
    (0.25, 0.469),
    (0.5, 0.27),
    (0.75, 0.123),
    (0.9, 0.047),
    (1.0, 0.0),
];

/// What a line with opacity `alpha` (`1 - Transparency`) really draws at,
/// see [`LINE_OPACITY`].
pub(super) fn line_alpha(alpha: f32) -> f32 {
    let transparency = 1.0 - alpha.clamp(0.0, 1.0);
    LINE_OPACITY
        .windows(2)
        .find(|pair| transparency <= pair[1].0)
        .map_or(0.0, |pair| {
            let ((t0, a0), (t1, a1)) = (pair[0], pair[1]);
            a0 + (a1 - a0) * (transparency - t0) / (t1 - t0)
        })
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
