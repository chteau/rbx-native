//! A handle drag's soft snaps: the faces of other parts a Move or Scale
//! handle drag can be pulled flush with (`Utility/getSoftSnaps`,
//! `SoftSnapper`), found once at the press, and the white axis line and dots
//! Studio draws while dragging.
//!
//! Studio finds them by sweeping a flat slab — the selection's cross-section
//! square to the drag axis, a tenth of a stud wider on every side — along the
//! axis with repeated `Blockcast`s: forward to collect the near face of each
//! part it runs into, then back to collect their far faces. Only parts the
//! slab actually sweeps into count, and a part the slab already overlaps
//! where a cast starts is invisible to that cast, as a shapecast's is
//! (`creator-docs`, `WorldRoot:Blockcast`: it "does not detect `BaseParts`
//! that **initially** intersect the shape"). Each cast is solved here in
//! closed form rather than cast: where along the axis the slab enters and
//! leaves every part's box, then the same walk Studio's casts take.

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use rbx_viewer::Pose;

use super::{handle_scale, snap_to, Dot, Line, ACTIVE, PASSIVE, SOFT_SNAP_MARGIN};

/// How much wider than the selection's cross-section the slab is, per side.
const SLAB_MARGIN: f32 = 0.1;
/// How far one `Blockcast` reaches.
const CAST_REACH: f32 = 1023.0;
/// How far past the last part hit the backward pass starts, at most.
const OVERSHOOT: f32 = 500.0;
/// A backward pass stops once it is this far behind where the drag started.
const BEHIND: f32 = 0.1;
/// A face this glancing to the axis (`|n·axis|` below it) is not one a drag
/// along the axis can be flush with; the part is passed over.
const GLANCING: f32 = 0.3;
/// Two contacts nearer than this along the axis are the same contact.
const TIE: f32 = 1e-4;
/// Studio's time budget for the whole search, `os.clock() - t > 0.01`.
pub(crate) const BUDGET: Duration = Duration::from_millis(10);

/// A handle drag's threshold without a grid, in handle scales at the snap.
const REACH: f32 = 0.4;
/// The dots' radii, in handle scales at the dot.
const DOT_RADIUS: f32 = 0.15;
const CURRENT_RADIUS: f32 = 0.2;
/// How far a guide line runs, both ways — Studio's stand-in for forever.
const FOREVER: f32 = 10000.0;

/// The slab a search sweeps: centred on `origin`, square to `axis`, with the
/// half extents `half` along the two directions `across`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Slab {
    pub(crate) origin: Vec3,
    pub(crate) axis: Vec3,
    pub(crate) across: [Vec3; 2],
    pub(crate) half: [f32; 2],
}

impl Slab {
    /// The slab for a selection whose box, in the orthonormal `basis`, is
    /// `size`, dragged along `basis[axis]` from `origin`.
    pub(crate) fn new(origin: Vec3, basis: [Vec3; 3], axis: usize, size: Vec3) -> Slab {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        Slab {
            origin,
            axis: basis[axis],
            across: [basis[u], basis[v]],
            half: [size[u] * 0.5 + SLAB_MARGIN, size[v] * 0.5 + SLAB_MARGIN],
        }
    }
}

/// The selection's own extent along the slab's axis, as the offsets that
/// bring its leading face, its trailing face and its pivot onto a snap
/// plane: `[-leading, trailing, 0]`. Studio finds the two faces by casting
/// the slab back at the selection from 400 studs out on either side.
pub(crate) fn offsets(slab: &Slab, dragged: &[Mat4]) -> Vec<f32> {
    let spans: Vec<(f32, f32)> = dragged
        .iter()
        .filter_map(|model| entry_exit(slab, *model))
        .map(|span| (span.enter, span.exit))
        .collect();
    let leading = spans.iter().map(|span| span.1).fold(None, max);
    let trailing = spans.iter().map(|span| span.0).fold(None, min);
    let mut offsets: Vec<f32> = [leading.map(|at| -at), trailing.map(|at| -at)]
        .into_iter()
        .flatten()
        .collect();
    if !offsets.contains(&0.0) {
        offsets.push(0.0);
    }
    offsets
}

fn max(best: Option<f32>, value: f32) -> Option<f32> {
    Some(best.map_or(value, |best| best.max(value)))
}

fn min(best: Option<f32>, value: f32) -> Option<f32> {
    Some(best.map_or(value, |best| best.min(value)))
}

/// One place a drag can snap to: the travel along the axis from where it
/// started, and the point on the axis line (at the face's plane) its dot is
/// drawn on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SoftSnap {
    pub(crate) point: Vec3,
    pub(crate) distance: f32,
    /// How near the travel has to come to take this snap without a grid:
    /// fixed at the press, from the camera as it was then.
    pub(crate) reach: f32,
}

/// Every face `slab` sweeps into among `candidates`, as snaps for each of
/// `offsets` (see [`offsets`]): at most `max_count / 2` near faces in each
/// direction, and never past `budget` from `started`.
pub(crate) fn soft_snaps(
    slab: &Slab,
    offsets: &[f32],
    candidates: &[Mat4],
    max_count: usize,
    started: Instant,
    pose: Pose,
    orthographic: bool,
) -> Vec<SoftSnap> {
    let spans: Vec<Span> = candidates
        .iter()
        .filter_map(|model| entry_exit(slab, *model))
        .collect();
    let mut snaps = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |along: f32| {
        let point = slab.origin + slab.axis * along;
        for offset in offsets {
            let distance = along + offset;
            if seen.insert((distance * 1000.0 + 0.5).floor() as i64) {
                snaps.push(SoftSnap {
                    point,
                    distance,
                    reach: REACH * handle_scale(point, pose, orthographic),
                });
            }
        }
    };
    let spent = || started.elapsed() > BUDGET;

    for direction in [-1.0f32, 1.0] {
        // Along `direction`, a span is entered at `near` and left at `far`.
        let ends = |span: &Span| {
            if direction > 0.0 {
                (span.enter, span.exit, span.enter_face, span.exit_face)
            } else {
                (-span.exit, -span.enter, span.exit_face, span.enter_face)
            }
        };
        let mut ahead: Vec<&Span> = spans.iter().filter(|span| ends(span).0 > 0.0).collect();
        ahead.sort_by(|a, b| ends(a).0.total_cmp(&ends(b).0));

        // Forward: each cast finds the nearest near face ahead of where the
        // last one stopped, and starts the next from there.
        let (mut travelled, mut count, mut last) = (0.0f32, 0, None);
        for span in ahead {
            if spent() || count >= max_count / 2 {
                break;
            }
            let (near, _, face, _) = ends(span);
            if near - travelled > CAST_REACH {
                break;
            }
            if glancing(face, slab.axis) {
                continue;
            }
            add(direction * near);
            travelled = near;
            count += 1;
            last = Some(span.size);
        }
        if count < max_count / 2 {
            travelled += last.map_or(OVERSHOOT, |size: f32| size.min(OVERSHOOT));
        }

        // Backward: cast back from there, collecting the far faces behind.
        let mut behind: Vec<&Span> = spans
            .iter()
            .filter(|span| ends(span).1 < travelled)
            .collect();
        behind.sort_by(|a, b| ends(b).1.total_cmp(&ends(a).1));
        for span in behind {
            if spent() {
                break;
            }
            let (_, far, _, face) = ends(span);
            if travelled - far > CAST_REACH {
                break;
            }
            if glancing(face, slab.axis) {
                continue;
            }
            travelled = far;
            if travelled < -BEHIND {
                break;
            }
            add(direction * far);
        }
    }
    snaps
}

/// Studio's check on each hit (`getSoftSnaps` PROTO_2): a part whose first
/// contact is a face nearly parallel to the axis is passed over.
///
/// Contact through a face is the one case that can glance. Contact through
/// an edge or corner is taken to be flush-facing: a short ray along the axis
/// there enters through whichever of the box's faces meets the axis most
/// squarely, and a box always has one within 55° of any axis. [inferred:
/// Studio verifies with that ray; this reads the answer off the contact.]
fn glancing(face: Option<Vec3>, axis: Vec3) -> bool {
    face.is_some_and(|normal| normal.dot(axis).abs() < GLANCING)
}

/// Where along the slab's axis it overlaps one box: from `enter` to `exit`,
/// with the box's face normal whenever first (or last) contact is through a
/// face, and the box's diagonal, which bounds how far past it the backward
/// pass starts.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    enter: f32,
    exit: f32,
    enter_face: Option<Vec3>,
    exit_face: Option<Vec3>,
    size: f32,
}

/// Separating-axis test of the slab, slid along its axis, against the box
/// `model` is drawn in. `None` when the slab never touches it.
fn entry_exit(slab: &Slab, model: Mat4) -> Option<Span> {
    let centre = model.w_axis.truncate();
    let columns = [model.x_axis, model.y_axis, model.z_axis].map(|column| column.truncate());
    let offset = centre - slab.origin;
    // The cheap rejection first: a box whose bounding sphere stays clear of
    // the swept prism is never touched.
    let radius = 0.5
        * columns
            .iter()
            .map(|column| column.length_squared())
            .sum::<f32>()
            .sqrt();
    if (0..2).any(|i| offset.dot(slab.across[i]).abs() > slab.half[i] + radius) {
        return None;
    }
    let axes = columns.map(|column| column.normalize_or_zero());

    let mut span = Span {
        enter: f32::NEG_INFINITY,
        exit: f32::INFINITY,
        enter_face: None,
        exit_face: None,
        size: 2.0 * radius,
    };
    let mut test = |direction: Vec3, face: Option<Vec3>| -> bool {
        let Some(direction) = direction.try_normalize() else {
            return true;
        };
        let reach = slab.half[0] * slab.across[0].dot(direction).abs()
            + slab.half[1] * slab.across[1].dot(direction).abs()
            + 0.5
                * columns
                    .iter()
                    .map(|column| column.dot(direction).abs())
                    .sum::<f32>();
        let at = offset.dot(direction);
        let rate = slab.axis.dot(direction);
        if rate.abs() < 1e-6 {
            return at.abs() <= reach;
        }
        // |at - t·rate| <= reach, solved for t.
        let (a, b) = ((at - reach) / rate, (at + reach) / rate);
        let (low, high) = if a < b { (a, b) } else { (b, a) };
        // A later axis takes the contact over only when it is clearly the
        // later one: the box's own faces go first, so a contact that is a
        // face stays a face when an edge axis happens to coincide with it.
        if low > span.enter + TIE {
            span.enter = low;
            span.enter_face = face;
        }
        if high < span.exit - TIE {
            span.exit = high;
            span.exit_face = face;
        }
        true
    };
    let mut candidates: Vec<(Vec3, Option<Vec3>)> = axes.map(|axis| (axis, Some(axis))).to_vec();
    candidates.extend([slab.axis, slab.across[0], slab.across[1]].map(|axis| (axis, None)));
    for axis in axes {
        for across in slab.across {
            candidates.push((across.cross(axis), None));
        }
    }
    for (direction, face) in candidates {
        if !test(direction, face) {
            return None;
        }
    }
    (span.enter <= span.exit).then_some(span)
}

/// Which snap, if any, a drag that has travelled `raw` along its axis takes
/// (`SoftSnapper:updateCurrentSnap`), given the grid in force (`0.0` for
/// none). The nearest snap, when it is within reach — half a grid step with a
/// grid, its own screen-constant reach without — and, with a grid, no further
/// than the grid's own correction (a hundredth of a stud's grace, so a snap
/// the grid happens to agree with wins).
pub(crate) fn choose(snaps: &[SoftSnap], raw: f32, grid: f32) -> Option<usize> {
    let (index, best) = snaps.iter().enumerate().min_by(|(_, a), (_, b)| {
        (a.distance - raw)
            .abs()
            .total_cmp(&(b.distance - raw).abs())
    })?;
    let miss = (best.distance - raw).abs();
    let reach = if grid > 0.0 { 0.5 * grid } else { best.reach } * SOFT_SNAP_MARGIN;
    let grid_miss = (grid > 0.0).then(|| (raw - snap_to(raw, grid)).abs());
    (miss <= reach && grid_miss.is_none_or(|grid_miss| miss < grid_miss + 0.01)).then_some(index)
}

/// The dots a handle drag shows on its snaps (`SoftSnapper:render`): white
/// on every face, yellow and larger on the one the drag is snapped to.
pub(crate) fn dots(
    snaps: &[SoftSnap],
    current: Option<usize>,
    pose: Pose,
    orthographic: bool,
) -> Vec<Dot> {
    let chosen = current.map(|index| snaps[index].point);
    let mut dots: Vec<Dot> = snaps
        .iter()
        .filter(|snap| Some(snap.point) != chosen)
        .map(|snap| Dot {
            centre: snap.point,
            radius: DOT_RADIUS * handle_scale(snap.point, pose, orthographic),
            color: PASSIVE,
            cube: false,
        })
        .collect();
    dots.dedup_by(|a, b| a.centre == b.centre);
    dots.extend(chosen.map(|point| Dot {
        centre: point,
        radius: CURRENT_RADIUS * handle_scale(point, pose, orthographic),
        color: ACTIVE,
        cube: false,
    }));
    dots
}

/// The Move tool's axis line while an arrow is dragged: white, depth-tested,
/// through `base` (where the handles stand now) along the dragged arrow's
/// `direction` as far as the eye can see, broken over the arrow itself —
/// from `base` out to `tip` studs along it.
pub(crate) fn axis_line(base: Vec3, direction: Vec3, tip: f32) -> [Line; 2] {
    [
        Line::hairline(
            base + direction * tip,
            base + direction * FOREVER,
            PASSIVE,
            1.0,
            0.0,
        ),
        Line::hairline(base - direction * FOREVER, base, PASSIVE, 1.0, 0.0),
    ]
}

/// The Scale tool's axis line while a face is dragged: the same line, with no
/// break.
pub(crate) fn extrude_line(centre: Vec3, direction: Vec3) -> Line {
    Line::hairline(
        centre - direction * FOREVER,
        centre + direction * FOREVER,
        PASSIVE,
        1.0,
        0.0,
    )
}

#[cfg(test)]
#[path = "sweep/tests.rs"]
mod tests;
