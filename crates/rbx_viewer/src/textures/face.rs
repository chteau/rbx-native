//! Which part of a surface a `Decal`/`Texture` covers, and how studs become UVs.

use glam::Vec3;

use crate::scene::ShapeKind;

/// `Enum.NormalId`: the part face a `FaceInstance` is pinned to, in object space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NormalId {
    Right,
    Top,
    Back,
    Left,
    Bottom,
    Front,
}

/// How a face instance lays its image on the part surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Mapping {
    /// `Decal`: scales the image to fit the entire face.
    Stretched,
    /// `Texture`: tiles the image at a fixed physical size per-stud, with optional offset.
    Tiled { studs: [f32; 2], offset: [f32; 2] },
}

/// A planar projection of one image onto a part's surface, in the part's own
/// object space (the unit mesh's `[-0.5, 0.5]³` box).
///
/// A surface point `p` of that mesh takes the UV
/// `((p · u + 0.5, p · v + 0.5)) * uv_scale + uv_offset`, and only belongs to
/// this projection when [`dominant`] sends its surface normal back to `normal`.
/// That clipping rule is what makes the image follow a curved surface instead of
/// hovering in front of it as a flat rectangle.
///
/// The one exception is a wedge's slope, which the shader hands to the Front
/// face whichever axis its 45° normal leans on — see [`projection`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Projection {
    pub(crate) normal: Vec3,
    pub(crate) u: Vec3,
    pub(crate) v: Vec3,
    pub(crate) uv_scale: [f32; 2],
    pub(crate) uv_offset: [f32; 2],
}

/// Object-space orientation of a face: outward `normal`, plus the image's right
/// (`u`) and *down* (`v`) axes.
///
/// The invariant `u × v = -normal` ensures the image is unmirrored when viewed
/// from outside the part: the handedness rule means right × down points into the
/// screen (standard for 2D images), which flips to point inward when rotated to
/// face outward in 3D.
struct Basis {
    normal: Vec3,
    u: Vec3,
    v: Vec3,
}

impl NormalId {
    pub(crate) fn from_ordinal(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(NormalId::Right),
            1 => Some(NormalId::Top),
            2 => Some(NormalId::Back),
            3 => Some(NormalId::Left),
            4 => Some(NormalId::Bottom),
            5 => Some(NormalId::Front),
            _ => None,
        }
    }

    /// The part-local outward axis of this face, which is the direction a
    /// `SpotLight`/`SurfaceLight` pinned to it emits along (see
    /// `crate::lighting::local`).
    pub(crate) fn axis(self) -> Vec3 {
        self.basis().normal
    }

    /// The whole object-space frame of this face: outward normal, image right,
    /// image down.
    ///
    /// `pub(crate)` because `scene::gui` lays a `SurfaceGui`'s canvas on the
    /// very rectangle a stretched `Decal` covers here, and the two must agree
    /// corner for corner.
    pub(crate) fn axes(self) -> (Vec3, Vec3, Vec3) {
        let Basis { normal, u, v } = self.basis();
        (normal, u, v)
    }

    /// Maps face ordinals to axes.
    ///
    /// Roblox front is -Z, up is +Y. For side faces, `v` (image down) aligns
    /// with -Y to keep images upright; `u` follows from the handedness invariant.
    /// Top and Bottom have no intrinsic up, so they follow the standard
    /// convention: image right runs along +X.
    fn basis(self) -> Basis {
        let (normal, u, v) = match self {
            NormalId::Right => (Vec3::X, -Vec3::Z, -Vec3::Y),
            NormalId::Left => (-Vec3::X, Vec3::Z, -Vec3::Y),
            NormalId::Top => (Vec3::Y, Vec3::X, Vec3::Z),
            NormalId::Bottom => (-Vec3::Y, Vec3::X, -Vec3::Z),
            NormalId::Back => (Vec3::Z, Vec3::X, -Vec3::Y),
            NormalId::Front => (-Vec3::Z, -Vec3::X, -Vec3::Y),
        };
        Basis { normal, u, v }
    }
}

/// The face an object-space surface normal belongs to: the axis it leans on most.
///
/// This is what splits a curved surface between the six faces — the upper
/// quarter of a cylinder's side is Top, the rest of the ring belongs to the four
/// horizontal faces. Ties go to Y, then X, so a `WedgePart`'s 45° slope reads as
/// Top rather than as one of its sides; `renderer/textured.wgsl` hands that Top
/// back to Front on wedge meshes, since a wedge has no top surface at all.
///
/// Mirrored by `dominant_axis` in `renderer/lighting.wgsl`, which does the same
/// classification per fragment; the two must agree or a decal lands on the wrong
/// face.
fn dominant(normal: Vec3) -> NormalId {
    let axis = normal.abs();
    if axis.y >= axis.x && axis.y >= axis.z {
        positive(normal.y, NormalId::Top, NormalId::Bottom)
    } else if axis.x >= axis.z {
        positive(normal.x, NormalId::Right, NormalId::Left)
    } else {
        positive(normal.z, NormalId::Back, NormalId::Front)
    }
}

fn positive(component: f32, high: NormalId, low: NormalId) -> NormalId {
    if component >= 0.0 {
        high
    } else {
        low
    }
}

/// Builds the object-space projection of one face instance on a part of `size`,
/// whose unit mesh is `kind`.
pub(crate) fn projection(
    face: NormalId,
    kind: ShapeKind,
    size: Vec3,
    mapping: Mapping,
) -> Projection {
    let Basis { normal, u, v } = face.basis();
    // The fragment shader keeps only the surface whose normal classifies back to
    // this one, so a basis normal that is not its own face's dominant axis would
    // silently draw nothing at all rather than land slightly off.
    debug_assert_eq!(dominant(normal), face);

    let (width, height) = extent(face, kind, size, u, v);
    let (u_range, v_range) = ranges(mapping, width, height);

    Projection {
        normal,
        u,
        v,
        uv_scale: [u_range.1 - u_range.0, v_range.1 - v_range.0],
        uv_offset: [u_range.0, v_range.0],
    }
}

/// How many studs of surface the image spans, along `u` then `v`.
///
/// A face's own width and height are the part's size measured along the two axes
/// the image runs on, whatever their sign — except on a wedge's slope, which is
/// longer than the part is tall. Roblox lays the image on the slope itself, so a
/// 13x11 wedge fits sqrt(13² + 11²) studs of image on it rather than 13.
fn extent(face: NormalId, kind: ShapeKind, size: Vec3, u: Vec3, v: Vec3) -> (f32, f32) {
    let height = match (kind, face) {
        (ShapeKind::Wedge, NormalId::Front) => size.y.hypot(size.z),
        _ => size.dot(v.abs()),
    };
    (size.dot(u.abs()), height)
}

/// UV span across the face, along `u` then `v`.
///
/// A tile size that is zero, negative or not finite would send the whole image
/// to infinity, so those degenerate `StudsPerTile` values fall back to the
/// stretched mapping rather than producing an unrenderable surface.
fn ranges(mapping: Mapping, width: f32, height: f32) -> ((f32, f32), (f32, f32)) {
    let Mapping::Tiled { studs, offset } = mapping else {
        return ((0.0, 1.0), (0.0, 1.0));
    };

    let axis = |studs: f32, offset: f32, extent: f32| {
        if studs.is_finite() && studs > 0.0 {
            (offset / studs, (extent + offset) / studs)
        } else {
            (0.0, 1.0)
        }
    };
    (
        axis(studs[0], offset[0], width),
        axis(studs[1], offset[1], height),
    )
}

#[cfg(test)]
#[path = "face/tests.rs"]
mod tests;
