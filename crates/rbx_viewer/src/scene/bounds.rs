//! The axis-aligned extent of a scene, which is all the camera framing needs.

use glam::{Mat4, Vec3};

use super::Part;

/// Axis-aligned extent of every box in a scene.
///
/// Computed from the corners of rotated boxes, not from their centers, to ensure
/// tight framing of the entire scene.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds {
    // Readable throughout `scene`, which is where the extent is asserted on.
    pub(super) min: Vec3,
    pub(super) max: Vec3,
}

impl Bounds {
    pub(crate) fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// The eight corners, which is what a shadow map's light-space fit needs:
    /// the box is axis-aligned in world space and stops being so as soon as it
    /// is looked at from the sun.
    pub(crate) fn corners(&self) -> [Vec3; 8] {
        std::array::from_fn(|index| {
            let pick = |axis: usize, low: f32, high: f32| {
                if index & (1 << axis) == 0 {
                    low
                } else {
                    high
                }
            };
            Vec3::new(
                pick(0, self.min.x, self.max.x),
                pick(1, self.min.y, self.max.y),
                pick(2, self.min.z, self.max.z),
            )
        })
    }

    /// Radius of the sphere enclosing the box, i.e. half its diagonal.
    pub(crate) fn radius(&self) -> f32 {
        (self.max - self.min).length() * 0.5
    }
}

/// Grows an axis-aligned box around the eight corners of every part.
///
/// Corners rather than centers: a rotated part sticks out of its own position, and the
/// camera framing is only as good as this box.
pub(crate) fn of(parts: &[Part]) -> Option<Bounds> {
    parts
        .iter()
        .flat_map(|part| unit_cube_corners(part.transform))
        .fold(None, grow)
}

/// One part's own axis-aligned extent — the same corner construction as
/// [`of`], stopped at a single part instead of folded across a whole scene.
/// This is what a cull test checks a drawable against: cheaper than
/// intersecting the part's actual (possibly rotated) shape, and still tight
/// enough to only ever be conservative in the cull's favor, never in it.
pub(crate) fn of_part(part: &Part) -> Bounds {
    of_transform(part.transform)
}

/// [`of_part`] without needing a whole [`Part`] — what a caller holding only a
/// model matrix (a shadow caster's patched transform, say) can still use.
pub(crate) fn of_transform(transform: Mat4) -> Bounds {
    unit_cube_corners(transform)
        .fold(None, grow)
        .expect("a unit cube always has eight corners")
}

/// The eight corners of the unit cube `[-0.5, 0.5]^3`, transformed into world
/// space — every part's unit mesh fits inside this cube regardless of
/// `ShapeKind`, so it bounds any of them conservatively.
fn unit_cube_corners(transform: Mat4) -> impl Iterator<Item = Vec3> {
    [-0.5f32, 0.5].into_iter().flat_map(move |x| {
        [-0.5f32, 0.5].into_iter().flat_map(move |y| {
            [-0.5f32, 0.5]
                .into_iter()
                .map(move |z| transform.transform_point3(Vec3::new(x, y, z)))
        })
    })
}

fn grow(acc: Option<Bounds>, corner: Vec3) -> Option<Bounds> {
    Some(match acc {
        None => Bounds {
            min: corner,
            max: corner,
        },
        Some(bounds) => Bounds {
            min: bounds.min.min(corner),
            max: bounds.max.max(corner),
        },
    })
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::{Bounds, Vec3};

    pub(crate) fn bounds_from(min: Vec3, max: Vec3) -> Bounds {
        Bounds { min, max }
    }
}
