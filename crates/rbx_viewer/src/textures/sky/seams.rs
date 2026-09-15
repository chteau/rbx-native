//! Ground truth for the skybox assembly: which panel border meets which.
//!
//! The table here is what [`super::SkyFace::basis`] is derived from, and the
//! tests check the bases against it — one on synthetic panels (no GPU, no
//! network), one on the real images the table was measured from (ignored).

mod real_capture;

use glam::Vec3;

use super::{quad, SkyFace, SKY_FACES};
use crate::textures::Quad;

/// A border of a panel image, walked left-to-right (top, bottom) or
/// top-to-bottom (left, right).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

pub(super) const EDGES: [Edge; 4] = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right];

/// The twelve cube edges, as the two panel borders that land on each of them.
///
/// `reversed` means the two borders run in opposite directions, so one has to be
/// walked backwards for its pixels to line up with the other's.
///
/// Measured off the six images of a real sky capture — see
/// [`real_capture::the_real_capture_borders_agree_only_as_this_table_says`], which re-derives
/// every row from the pixels.
pub(super) const ADJACENT_BORDERS: [(SkyFace, Edge, SkyFace, Edge, bool); 12] = [
    (SkyFace::Rt, Edge::Top, SkyFace::Up, Edge::Bottom, false),
    (SkyFace::Rt, Edge::Bottom, SkyFace::Dn, Edge::Top, false),
    (SkyFace::Rt, Edge::Left, SkyFace::Bk, Edge::Right, false),
    (SkyFace::Rt, Edge::Right, SkyFace::Ft, Edge::Left, false),
    (SkyFace::Lf, Edge::Top, SkyFace::Up, Edge::Top, true),
    (SkyFace::Lf, Edge::Bottom, SkyFace::Dn, Edge::Bottom, true),
    (SkyFace::Lf, Edge::Left, SkyFace::Ft, Edge::Right, false),
    (SkyFace::Lf, Edge::Right, SkyFace::Bk, Edge::Left, false),
    (SkyFace::Up, Edge::Left, SkyFace::Bk, Edge::Top, false),
    (SkyFace::Up, Edge::Right, SkyFace::Ft, Edge::Top, true),
    (SkyFace::Dn, Edge::Left, SkyFace::Bk, Edge::Bottom, true),
    (SkyFace::Dn, Edge::Right, SkyFace::Ft, Edge::Bottom, false),
];

/// Side of a panel whose borders the synthetic test numbers: small enough to read
/// in a failure message, wide enough to have interior border texels.
const PANEL: usize = 8;

/// Pixel coordinates of the `step`-th texel of a border.
fn texel(edge: Edge, step: usize) -> (usize, usize) {
    match edge {
        Edge::Top => (step, 0),
        Edge::Bottom => (step, PANEL - 1),
        Edge::Left => (0, step),
        Edge::Right => (PANEL - 1, step),
    }
}

/// Where that same texel touches the cube edge, in image UV.
///
/// The across-border coordinate is pinned to the border itself (0 or 1) rather
/// than to the texel centre, so two panels sharing a cube edge produce the very
/// same point and can be compared exactly.
fn border_uv(edge: Edge, step: usize) -> [f32; 2] {
    let along = (step as f32 + 0.5) / PANEL as f32;
    match edge {
        Edge::Top => [along, 0.0],
        Edge::Bottom => [along, 1.0],
        Edge::Left => [0.0, along],
        Edge::Right => [1.0, along],
    }
}

/// The world direction a UV lands on, read off the quad the renderer is handed.
fn direction(quad: &Quad, [x, y]: [f32; 2]) -> Vec3 {
    let at = |corner: usize| Vec3::from(quad.positions[corner]);

    at(0) * (1.0 - x) * (1.0 - y) + at(1) * x * (1.0 - y) + at(2) * x * y + at(3) * (1.0 - x) * y
}

/// Six panels whose border texels are numbered so that a value says both which
/// cube edge it belongs to and how far along that edge it sits.
///
/// Built from [`ADJACENT_BORDERS`] alone, never from a basis: the numbering is
/// the claim, and lining the numbers back up through the quads is the check.
/// Corner texels are left at zero — they belong to three panels at once, so no
/// single edge number fits them.
fn numbered_panels() -> Vec<[u8; PANEL * PANEL]> {
    let mut panels = vec![[0u8; PANEL * PANEL]; SKY_FACES.len()];

    for (cube_edge, &(a_face, a_edge, b_face, b_edge, reversed)) in
        ADJACENT_BORDERS.iter().enumerate()
    {
        for step in 1..PANEL - 1 {
            let value = (cube_edge as u8 + 1) * 10 + step as u8;
            let mirrored = if reversed { PANEL - 1 - step } else { step };

            for (face, edge, step) in [(a_face, a_edge, step), (b_face, b_edge, mirrored)] {
                let (x, y) = texel(edge, step);
                panels[index_of(face)][y * PANEL + x] = value;
            }
        }
    }
    panels
}

fn index_of(face: SkyFace) -> usize {
    SKY_FACES
        .iter()
        .position(|&candidate| candidate == face)
        .expect("every face is in SKY_FACES")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // The table must describe a cube: twenty-four borders, each spoken for once.
    #[test]
    fn every_border_of_every_panel_is_claimed_exactly_once() {
        let claimed: Vec<(SkyFace, Edge)> = ADJACENT_BORDERS
            .iter()
            .flat_map(|&(a_face, a_edge, b_face, b_edge, _)| [(a_face, a_edge), (b_face, b_edge)])
            .collect();

        assert_eq!(claimed.len(), 24);
        for face in SKY_FACES {
            for edge in EDGES {
                let times = claimed.iter().filter(|&&pair| pair == (face, edge)).count();
                assert_eq!(times, 1, "{face:?} {edge:?}");
            }
        }
    }

    /// The continuity test: numbered borders, mapped through the real quads, must
    /// meet their partner on the cube with the same number on both sides.
    #[test]
    fn numbered_borders_meet_their_partner_with_the_same_number() {
        let panels = numbered_panels();
        let mut on_cube: HashMap<[i64; 3], Vec<(SkyFace, u8)>> = HashMap::new();

        for face in SKY_FACES {
            let quad = quad(face);
            for edge in EDGES {
                for step in 1..PANEL - 1 {
                    let (x, y) = texel(edge, step);
                    let value = panels[index_of(face)][y * PANEL + x];
                    assert_ne!(value, 0, "{face:?} {edge:?} texel {step} unnumbered");

                    let point = direction(&quad, border_uv(edge, step));
                    let key = [point.x, point.y, point.z].map(|c| (c * 1e4).round() as i64);
                    on_cube.entry(key).or_default().push((face, value));
                }
            }
        }

        assert_eq!(on_cube.len(), 12 * (PANEL - 2));
        for (point, mut meeting) in on_cube {
            assert_eq!(meeting.len(), 2, "at {point:?}: {meeting:?}");
            let (first, second) = (meeting.pop().unwrap(), meeting.pop().unwrap());
            assert_ne!(first.0, second.0, "a panel met itself at {point:?}");
            assert_eq!(first.1, second.1, "seam at {point:?}: {first:?} {second:?}");
        }
    }

    // A panel's own borders must stay inside its own plane: the UV walk is only
    // meaningful if the quad is the flat face the basis says it is.
    #[test]
    fn a_numbered_border_stays_on_the_face_it_belongs_to() {
        for face in SKY_FACES {
            let quad = quad(face);
            let normal = Vec3::from(quad.normal);

            for edge in EDGES {
                let point = direction(&quad, border_uv(edge, 3));
                assert!((point.dot(normal) - 1.0).abs() < 1e-6, "{face:?} {edge:?}");
            }
        }
    }
}
