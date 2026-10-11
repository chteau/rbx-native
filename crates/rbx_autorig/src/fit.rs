//! Fitting the R15 skeleton: normalise the mesh into Roblox space, find the
//! five extremities, then place the 15 joints from cross-section profiles
//! and the proportions of Roblox's Mannequin body.
//!
//! Every distance threshold below is a fraction of the height `h`, so the
//! same rules hold at any scale. The Mannequin numbers come from Roblox's
//! reference attachment tables: neck 0.844·h, shoulders 0.789·h, hips 0.511·h,
//! ankles 0.06·h, the elbow halfway from shoulder to wrist and the knee 0.486
//! of the way from hip to ankle.

use glam::Vec3;

use crate::mesh::{bounds, Mesh};
use crate::orient::{self, Orientation};
use crate::rig::{Joint, Side};
use crate::voxel::{Geodesic, Grid, Section};
use crate::Error;

/// The five tips the skeleton is hung from, in Roblox space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Extremities {
    pub head: Vec3,
    pub left_hand: Vec3,
    pub right_hand: Vec3,
    pub left_foot: Vec3,
    pub right_foot: Vec3,
}

impl Extremities {
    pub fn hand(&self, side: Side) -> Vec3 {
        match side {
            Side::Left => self.left_hand,
            Side::Right => self.right_hand,
        }
    }

    pub fn foot(&self, side: Side) -> Vec3 {
        match side {
            Side::Left => self.left_foot,
            Side::Right => self.right_foot,
        }
    }

    fn hand_mut(&mut self, side: Side) -> &mut Vec3 {
        match side {
            Side::Left => &mut self.left_hand,
            Side::Right => &mut self.right_hand,
        }
    }

    fn foot_mut(&mut self, side: Side) -> &mut Vec3 {
        match side {
            Side::Left => &mut self.left_foot,
            Side::Right => &mut self.right_foot,
        }
    }
}

/// A skeleton fitted to a mesh, in Roblox space and studs. Every field is
/// meant to be edited (markers dragged, the front turned) before [`crate::split`].
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// How the file is turned into Roblox space.
    pub orientation: Orientation,
    /// Studs per file unit.
    pub scale: f32,
    /// Added after rotating and scaling: puts the feet on y = 0 and the
    /// character's plane of symmetry on x = 0.
    pub translation: Vec3,
    /// Joint positions, indexed by [`Joint`] (`joints[Joint::Neck as usize]`).
    pub joints: [Vec3; 15],
    pub extremities: Extremities,
    /// Things the user should look at: guesses made, poses that fit less well.
    pub warnings: Vec<String>,
}

impl Fit {
    pub fn joint(&self, joint: Joint) -> Vec3 {
        self.joints[joint.index()]
    }

    pub fn set_joint(&mut self, joint: Joint, position: Vec3) {
        self.joints[joint.index()] = position;
    }

    /// A point of the source file in Roblox space.
    pub fn to_roblox(&self, p: Vec3) -> Vec3 {
        self.orientation.rotation() * p * self.scale + self.translation
    }

    /// A direction (normal) of the source file in Roblox space.
    pub fn direction_to_roblox(&self, v: Vec3) -> Vec3 {
        self.orientation.rotation() * v
    }
}

/// Knobs for [`fit_with`]; [`crate::fit`] uses the defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitOptions {
    /// Total height (feet to head top) in studs.
    pub height_studs: f32,
    /// Skips orientation detection.
    pub orientation: Option<Orientation>,
    /// Voxels along the longest side of the bounds.
    pub resolution: usize,
    /// Mirror-average the left and right estimates, as Roblox's own Avatar
    /// Setup expects symmetric bodies.
    pub symmetric: bool,
}

impl Default for FitOptions {
    fn default() -> Self {
        FitOptions {
            height_studs: 5.2,
            orientation: None,
            resolution: 128,
            symmetric: true,
        }
    }
}

/// Fits an R15 skeleton to `mesh`, scaled so it stands `height_studs` tall.
pub fn fit(mesh: &Mesh, height_studs: f32) -> Result<Fit, Error> {
    fit_with(
        mesh,
        &FitOptions {
            height_studs,
            ..FitOptions::default()
        },
    )
}

pub fn fit_with(mesh: &Mesh, options: &FitOptions) -> Result<Fit, Error> {
    if !(options.height_studs.is_finite() && options.height_studs > 0.) {
        return Err(Error::Height);
    }
    let triangles: Vec<[u32; 3]> = mesh.valid_triangles().collect();
    if triangles.is_empty() {
        return Err(Error::Empty);
    }
    let mut warnings = Vec::new();
    let orientation = match options.orientation {
        Some(o) => o,
        None => orient::detect(mesh, &mut warnings),
    };
    let (scale, translation) = normalise(mesh, &triangles, orientation, options.height_studs)?;
    let rotation = orientation.rotation();
    let points: Vec<Vec3> = mesh
        .positions
        .iter()
        .map(|&p| rotation * p * scale + translation)
        .collect();
    let grid = Grid::build(&points, &triangles, options.resolution);
    let mut fit = landmarks(&grid, options.height_studs, &mut warnings)?;
    fit.orientation = orientation;
    fit.scale = scale;
    fit.translation = translation;

    let cx = centre_line(&fit);
    if options.symmetric {
        symmetrise(&mut fit, cx, options.height_studs, &mut warnings);
    }
    // Put the plane of symmetry on x = 0.
    for p in fit.joints.iter_mut() {
        p.x -= cx;
    }
    let e = &mut fit.extremities;
    for p in [
        &mut e.head,
        &mut e.left_hand,
        &mut e.right_hand,
        &mut e.left_foot,
        &mut e.right_foot,
    ] {
        p.x -= cx;
    }
    fit.translation.x -= cx;
    fit.warnings = warnings;
    Ok(fit)
}

/// Scale and translation that stand the rotated mesh on y = 0, `height`
/// tall, centred on x and z.
fn normalise(
    mesh: &Mesh,
    triangles: &[[u32; 3]],
    orientation: Orientation,
    height: f32,
) -> Result<(f32, Vec3), Error> {
    let rotation = orientation.rotation();
    let (lo, hi) = bounds(
        triangles
            .iter()
            .flatten()
            .map(|&i| rotation * mesh.positions[i as usize]),
    );
    let tall = hi.y - lo.y;
    if !(tall.is_finite() && tall > 0.) {
        return Err(Error::Empty);
    }
    let scale = height / tall;
    let translation = Vec3::new(
        -(lo.x + hi.x) / 2. * scale,
        -lo.y * scale,
        -(lo.z + hi.z) / 2. * scale,
    );
    Ok((scale, translation))
}

/// The mean x of the left/right pairs: the character's plane of symmetry.
fn centre_line(fit: &Fit) -> f32 {
    let pairs = [
        Joint::LeftShoulder,
        Joint::LeftElbow,
        Joint::LeftWrist,
        Joint::LeftHip,
        Joint::LeftKnee,
        Joint::LeftAnkle,
    ];
    pairs
        .iter()
        .map(|&j| (fit.joint(j).x + fit.joint(j.mirror()).x) / 2.)
        .sum::<f32>()
        / pairs.len() as f32
}

fn reflect(p: Vec3, cx: f32) -> Vec3 {
    Vec3::new(2. * cx - p.x, p.y, p.z)
}

/// Averages each left/right pair with its mirror image about `cx` and puts
/// the centre-line joints on it.
fn symmetrise(fit: &mut Fit, cx: f32, h: f32, warnings: &mut Vec<String>) {
    let mut worst: Option<(f32, Joint)> = None;
    for joint in Joint::ALL {
        match joint.side() {
            Some(Side::Left) => {
                let (l, r) = (fit.joint(joint), fit.joint(joint.mirror()));
                let gap = (l - reflect(r, cx)).length();
                if gap > 0.06 * h && worst.is_none_or(|(g, _)| gap > g) {
                    worst = Some((gap, joint));
                }
                let left = (l + reflect(r, cx)) / 2.;
                fit.set_joint(joint, left);
                fit.set_joint(joint.mirror(), reflect(left, cx));
            }
            Some(Side::Right) => {}
            None => fit.joints[joint.index()].x = cx,
        }
    }
    if let Some((gap, joint)) = worst {
        warnings.push(format!(
            "The two sides differ by {:.0}% of the height at the {}: the markers were averaged; check them or turn symmetry off.",
            100. * gap / h,
            joint.name().trim_start_matches("Left").to_lowercase()
        ));
    }
    let e = &mut fit.extremities;
    e.head.x = cx;
    let hand = (e.left_hand + reflect(e.right_hand, cx)) / 2.;
    (e.left_hand, e.right_hand) = (hand, reflect(hand, cx));
    let foot = (e.left_foot + reflect(e.right_foot, cx)) / 2.;
    (e.left_foot, e.right_foot) = (foot, reflect(foot, cx));
}

/// Copies `from`'s side onto the other, mirrored about x = 0, and puts the
/// centre-line joints and the head on x = 0: "symmetric markers".
pub fn mirror(fit: &mut Fit, from: Side) {
    for joint in Joint::ALL {
        match joint.side() {
            Some(side) if side == from => {
                let p = fit.joint(joint);
                fit.set_joint(joint.mirror(), reflect(p, 0.));
            }
            Some(_) => {}
            None => fit.joints[joint.index()].x = 0.,
        }
    }
    let e = &mut fit.extremities;
    e.head.x = 0.;
    let (hand, foot) = (e.hand(from), e.foot(from));
    *e.hand_mut(from.mirror()) = reflect(hand, 0.);
    *e.foot_mut(from.mirror()) = reflect(foot, 0.);
}

/// A path from an extremity towards the body's core, resampled, pulled onto
/// the medial axis, with its cross-section profile.
struct Limb {
    points: Vec<Vec3>,
    tangents: Vec<Vec3>,
    /// Arc length from the tip.
    s: Vec<f32>,
    sections: Vec<Section>,
}

fn resample(raw: &[Vec3], step: f32) -> Vec<Vec3> {
    let mut out = vec![raw[0]];
    let mut carried = 0.;
    for pair in raw.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let len = (b - a).length();
        let mut at = step - carried;
        while at <= len {
            out.push(a + (b - a) * (at / len));
            at += step;
        }
        carried = len - (at - step);
    }
    out
}

fn smooth(points: &mut [Vec3], radius: usize) {
    let copy = points.to_vec();
    let n = copy.len();
    // The tip stays put: it is a measured extremity.
    for (i, p) in points.iter_mut().enumerate().skip(1) {
        let (a, b) = (i.saturating_sub(radius), (i + radius).min(n - 1));
        *p = copy[a..=b].iter().sum::<Vec3>() / (b - a + 1) as f32;
    }
}

impl Limb {
    fn trace(grid: &Grid, geo: &Geodesic, tip: usize, h: f32) -> Limb {
        let mut raw = Vec::new();
        let mut v = tip;
        loop {
            raw.push(grid.centre(v));
            let p = geo.prev[v];
            if p == u32::MAX {
                break;
            }
            v = p as usize;
        }
        if raw.len() < 2 {
            raw.push(raw[0] + Vec3::Y * grid.cell);
        }
        let mut points = resample(&raw, 1.5 * grid.cell);
        smooth(&mut points, 2);
        let limit = 0.12 * h;
        let mut limb = Limb {
            points,
            tangents: Vec::new(),
            s: Vec::new(),
            sections: Vec::new(),
        };
        for pass in 0..2 {
            limb.measure(grid, limit);
            if pass == 0 {
                for (p, s) in limb.points.iter_mut().zip(&limb.sections).skip(1) {
                    if s.found && !s.truncated {
                        *p = s.centroid;
                    }
                }
                smooth(&mut limb.points, 2);
            }
        }
        limb
    }

    fn measure(&mut self, grid: &Grid, limit: f32) {
        let n = self.points.len();
        self.tangents = (0..n)
            .map(|i| {
                let (a, b) = (i.saturating_sub(2), (i + 2).min(n - 1));
                (self.points[b] - self.points[a])
                    .try_normalize()
                    .unwrap_or(Vec3::Y)
            })
            .collect();
        self.s = std::iter::once(0.)
            .chain(self.points.windows(2).scan(0., |acc, w| {
                *acc += (w[1] - w[0]).length();
                Some(*acc)
            }))
            .collect();
        self.sections = (0..n)
            .map(|i| grid.section(self.points[i], self.tangents[i], limit))
            .collect();
    }

    fn area(&self, i: usize) -> f32 {
        if self.sections[i].found {
            self.sections[i].area
        } else {
            f32::INFINITY
        }
    }

    fn indices_where(&self, keep: impl Fn(usize) -> bool) -> Vec<usize> {
        (0..self.points.len()).filter(|&i| keep(i)).collect()
    }

    /// The narrowest section among `candidates`; near-ties (within 8%) go
    /// to the one `prior` scores lowest, so a uniform tube picks the
    /// expected spot rather than whichever sample is a voxel thinner.
    fn narrowest(&self, candidates: &[usize], prior: impl Fn(usize) -> f32) -> Option<usize> {
        let least = candidates
            .iter()
            .map(|&i| self.area(i))
            .fold(f32::INFINITY, f32::min);
        candidates
            .iter()
            .copied()
            .filter(|&i| self.area(i) <= least * 1.08)
            .min_by(|&a, &b| prior(a).total_cmp(&prior(b)))
    }

    /// The first sample after `from` whose section has joined something much
    /// bigger: the limb entering the torso.
    fn merge(&self, from: usize) -> usize {
        let mut seen: Vec<f32> = Vec::new();
        for i in from..self.points.len() {
            let s = &self.sections[i];
            if seen.len() >= 3 {
                let mut sorted = seen.clone();
                sorted.sort_by(f32::total_cmp);
                let median = sorted[sorted.len() / 2];
                if !s.found || s.truncated || s.area > 2.5 * median {
                    return i;
                }
            }
            if s.found {
                seen.push(s.area);
            }
        }
        self.points.len() - 1
    }

    /// The largest distance of samples `range` from segment `a`–`b`, and where.
    fn farthest_from(
        &self,
        range: std::ops::Range<usize>,
        a: Vec3,
        b: Vec3,
    ) -> Option<(f32, usize)> {
        range
            .map(|i| (segment_distance(self.points[i], a, b), i))
            .max_by(|x, y| x.0.total_cmp(&y.0))
    }
}

pub(crate) fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(f32::MIN_POSITIVE)).clamp(0., 1.);
    (p - (a + ab * t)).length()
}

/// Least-squares line through `points`: centroid and unit direction.
fn line_fit(points: &[Vec3]) -> (Vec3, Vec3) {
    let c = points.iter().sum::<Vec3>() / points.len().max(1) as f32;
    let mut cov = [[0f32; 3]; 3];
    for p in points {
        let d = *p - c;
        for (r, row) in cov.iter_mut().enumerate() {
            for (k, v) in row.iter_mut().enumerate() {
                *v += d[r] * d[k];
            }
        }
    }
    let m = glam::Mat3::from_cols_array_2d(&cov);
    let mut v = points
        .last()
        .zip(points.first())
        .and_then(|(a, b)| (*a - *b).try_normalize())
        .unwrap_or(Vec3::Y);
    for _ in 0..32 {
        v = (m * v).try_normalize().unwrap_or(v);
    }
    (c, v)
}

/// Landmarks of the solid `grid`, in Roblox space with the feet on y = 0.
fn landmarks(grid: &Grid, h: f32, warnings: &mut Vec<String>) -> Result<Fit, Error> {
    let cell = grid.cell;
    let thickness = grid.distance();

    // The core: the thickest voxel, nudged towards the middle of the body so
    // a flat-chested torso's long ridge of equal maxima picks a central one.
    let solid: Vec<usize> = (0..grid.solid.len()).filter(|&v| grid.solid[v]).collect();
    if solid.is_empty() {
        return Err(Error::Empty);
    }
    let middle = solid.iter().map(|&v| grid.centre(v)).sum::<Vec3>() / solid.len() as f32;
    let core = solid
        .iter()
        .copied()
        .max_by(|&a, &b| {
            let score = |v: usize| thickness[v] - 0.02 * (grid.centre(v) - middle).length() / cell;
            score(a).total_cmp(&score(b))
        })
        .expect("non-empty");
    let geo = grid.geodesic(&[(core, 0)]);

    let tips = extremities(grid, &geo, core, h);
    if tips.len() < 5 {
        return Err(Error::Extremities { found: tips.len() });
    }
    if tips.len() > 5 {
        warnings.push(format!(
            "Found {} extremities (a tail, wings or long hair?); kept the five farthest. Check the head, hand and foot markers.",
            tips.len()
        ));
    }
    let mut tips: Vec<usize> = tips.into_iter().take(5).collect();
    tips.sort_by(|&a, &b| grid.centre(a).y.total_cmp(&grid.centre(b).y));
    let (feet, upper) = tips.split_at(2);
    let feet_x = (grid.centre(feet[0]).x + grid.centre(feet[1]).x) / 2.;
    let head = *upper
        .iter()
        .min_by(|&&a, &&b| {
            (grid.centre(a).x - feet_x)
                .abs()
                .total_cmp(&(grid.centre(b).x - feet_x).abs())
        })
        .expect("three tips");
    let mut hands: Vec<usize> = upper.iter().copied().filter(|&t| t != head).collect();
    let mut feet = feet.to_vec();
    let by_x = |a: &usize, b: &usize| grid.centre(*a).x.total_cmp(&grid.centre(*b).x);
    hands.sort_by(by_x);
    feet.sort_by(by_x);
    // Left is −X in Roblox space.
    let (left_hand, right_hand, left_foot, right_foot) = (hands[0], hands[1], feet[0], feet[1]);
    if grid.centre(head).y < grid.centre(left_hand).y.max(grid.centre(right_hand).y) {
        warnings.push(
            "A hand is above the head; arms-up poses are not supported, so check the head and hand markers."
                .into(),
        );
    }
    let cx = (feet_x + (grid.centre(left_hand).x + grid.centre(right_hand).x) / 2.) / 2.;

    let mut joints = [Vec3::ZERO; 15];
    let mut set = |j: Joint, p: Vec3| joints[j.index()] = p;

    // Neck: the narrow run below the head, its lower end where the section
    // starts widening into the shoulders (the Mannequin's head keeps its neck).
    let head_limb = Limb::trace(grid, &geo, head, h);
    let neck = neck(&head_limb, h, cx, warnings);
    set(Joint::Neck, neck);

    let crotch = match crotch(grid, grid.centre(left_foot), grid.centre(right_foot), h) {
        Some(y) if y > 0.3 * h => y,
        _ => {
            warnings.push(
                "Couldn\u{2019}t find the crotch (legs touching all the way down?); placed the hips by proportion."
                    .into(),
            );
            0.47 * h
        }
    };

    for (side, hand) in [(Side::Left, left_hand), (Side::Right, right_hand)] {
        let limb = Limb::trace(grid, &geo, hand, h);
        let (shoulder, elbow, wrist) = arm(&limb, h, side, neck.y, warnings);
        let (s, e, w) = match side {
            Side::Left => (Joint::LeftShoulder, Joint::LeftElbow, Joint::LeftWrist),
            Side::Right => (Joint::RightShoulder, Joint::RightElbow, Joint::RightWrist),
        };
        set(s, shoulder);
        set(e, elbow);
        set(w, wrist);
    }
    for (side, foot) in [(Side::Left, left_foot), (Side::Right, right_foot)] {
        let limb = Limb::trace(grid, &geo, foot, h);
        let (hip, knee, ankle) = leg(&limb, h, crotch);
        let (a, b, c) = match side {
            Side::Left => (Joint::LeftHip, Joint::LeftKnee, Joint::LeftAnkle),
            Side::Right => (Joint::RightHip, Joint::RightKnee, Joint::RightAnkle),
        };
        set(a, hip);
        set(b, knee);
        set(c, ankle);
    }

    // Root and waist on the centre line, by the Mannequin's proportions.
    let hips = (joints[Joint::LeftHip.index()] + joints[Joint::RightHip.index()]) / 2.;
    joints[Joint::Root.index()] = hips + Vec3::Y * 0.022 * h;
    joints[Joint::Waist.index()] = waist(grid, hips, neck, cx, h);

    Ok(Fit {
        orientation: Orientation::ROBLOX,
        scale: 1.,
        translation: Vec3::ZERO,
        joints,
        extremities: Extremities {
            head: grid.centre(head),
            left_hand: grid.centre(left_hand),
            right_hand: grid.centre(right_hand),
            left_foot: grid.centre(left_foot),
            right_foot: grid.centre(right_foot),
        },
        warnings: Vec::new(),
    })
}

/// Tips of the geodesic field that end a real branch, farthest first.
///
/// Local maxima of the geodesic distance from the core sit at every bump:
/// heels, thumbs, the corners of a boxy hand. A tip counts only if it lies
/// at least 0.16·h *through the body* from the paths of the tips already
/// taken (each path running from its tip to the core). A heel is a few
/// centimetres from its own toe's path across the foot and is dropped; the
/// head and the hands are far from every leg's path, even with an I-pose
/// hand hanging beside the thigh, because the way round runs through the
/// shoulder. (Where two shortest paths part is no use for this: inside a
/// convex shin the paths to toe and heel part at the knee.) The head lies
/// 0.25·h or more from the arms' paths; a squared-off shoulder only 0.13·h.
fn extremities(grid: &Grid, geo: &Geodesic, core: usize, h: f32) -> Vec<usize> {
    let mut candidates: Vec<usize> = (0..grid.solid.len())
        .filter(|&v| grid.solid[v] && geo.dist[v].is_finite() && v != core)
        .filter(|&v| grid.is_local_max(v, &geo.dist))
        .collect();
    candidates.sort_by(|&a, &b| geo.dist[b].total_cmp(&geo.dist[a]));
    let branch = 0.16 * h / grid.cell;
    let mut apart = vec![f32::INFINITY; grid.solid.len()];
    let mut tips = Vec::new();
    for c in candidates {
        if apart[c] < branch || geo.dist[c] < branch {
            continue;
        }
        tips.push(c);
        if tips.len() == 8 {
            break;
        }
        let mut path = Vec::new();
        let mut w = c;
        loop {
            path.push((w, 0));
            let p = geo.prev[w];
            if p == u32::MAX {
                break;
            }
            w = p as usize;
        }
        let from_path = grid.geodesic(&path);
        for (a, d) in apart.iter_mut().zip(&from_path.dist) {
            *a = a.min(*d);
        }
    }
    tips
}

fn neck(limb: &Limb, h: f32, cx: f32, warnings: &mut Vec<String>) -> Vec3 {
    let n = limb.points.len();
    let start = (0..n).find(|&i| limb.s[i] >= 0.03 * h).unwrap_or(0);
    let mut least = f32::INFINITY;
    let mut entry = None;
    for i in start..n {
        if limb.s[i] > 0.4 * h {
            break;
        }
        let s = &limb.sections[i];
        if limb.s[i] >= 0.05 * h && (!s.found || s.truncated || s.area > 2.2 * least) {
            entry = Some(i);
            break;
        }
        least = least.min(limb.area(i));
    }
    let fallback = |warnings: &mut Vec<String>| {
        warnings.push(
            "Couldn\u{2019}t find a distinct neck; placed it at 82% of the height. Check the neck marker."
                .into(),
        );
        Vec3::new(cx, 0.82 * h, limb.points[0].z)
    };
    let Some(entry) = entry.filter(|&e| e > start + 1) else {
        return fallback(warnings);
    };
    let run: Vec<usize> = (start..entry).collect();
    let Some(min) = run
        .iter()
        .copied()
        .min_by(|&a, &b| limb.area(a).total_cmp(&limb.area(b)))
    else {
        return fallback(warnings);
    };
    let widest = (start..=min).map(|i| limb.area(i)).fold(0., f32::max);
    let mut lower = min;
    while lower + 1 < entry && limb.area(lower + 1) <= 1.25 * limb.area(min) {
        lower += 1;
    }
    let p = limb.points[lower];
    if limb.area(min) > 0.8 * widest || !(0.7 * h..=0.93 * h).contains(&p.y) {
        return fallback(warnings);
    }
    p
}

/// Shoulder, elbow and wrist from a hand's limb.
fn arm(
    limb: &Limb,
    h: f32,
    side: Side,
    neck_y: f32,
    warnings: &mut Vec<String>,
) -> (Vec3, Vec3, Vec3) {
    let n = limb.points.len();
    let window = limb.indices_where(|i| (0.05 * h..=0.17 * h).contains(&limb.s[i]));
    let wrist_i = limb
        .narrowest(&window, |i| (limb.s[i] - 0.10 * h).abs())
        .unwrap_or_else(|| {
            (0..n)
                .min_by(|&a, &b| {
                    (limb.s[a] - 0.1 * h)
                        .abs()
                        .total_cmp(&(limb.s[b] - 0.1 * h).abs())
                })
                .unwrap_or(0)
        });
    let wrist = limb.points[wrist_i];
    let merge = limb.merge(wrist_i + 1).max(wrist_i + 1).min(n - 1);

    // The arm's axis, from the samples between the wrist and the torso.
    let from = wrist_i + (merge - wrist_i) * 2 / 5;
    let samples: Vec<Vec3> = if merge - from >= 3 {
        limb.points[from..merge].to_vec()
    } else {
        limb.points[wrist_i..=merge].to_vec()
    };
    let (c, mut d) = line_fit(&samples);
    if d.dot(limb.points[0] - c) < 0. {
        d = -d;
    }
    let radius = {
        let mut a: Vec<f32> = (wrist_i..merge.max(wrist_i + 1))
            .map(|i| limb.area(i))
            .filter(|a| a.is_finite())
            .collect();
        a.sort_by(f32::total_cmp);
        a.get(a.len() / 2)
            .map_or(0.03 * h, |m| (m / std::f32::consts::PI).sqrt())
    };

    // The shoulder lies on the arm's axis. A hanging arm (A- or I-pose)
    // fixes it by height, the Mannequin's 0.055·h below the neck; a
    // horizontal arm (T-pose) by where its section merged with the torso's
    // side. Poses in between blend the two.
    let on_axis_y = |y: f32| (d.y.abs() > 0.2).then(|| c + d * ((y - c.y) / d.y));
    let on_axis_x = |x: f32| (d.x.abs() > 0.2).then(|| c + d * ((x - c.x) / d.x));
    let by_height = on_axis_y(neck_y - 0.055 * h);
    let by_side = on_axis_x(limb.points[merge].x - side.sign() * 0.2 * radius);
    let steep = ((d.y.abs() - 0.5) / 0.3).clamp(0., 1.);
    let mut shoulder = match (by_height, by_side) {
        (Some(a), Some(b)) => b.lerp(a, steep),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => limb.points[merge],
    };
    shoulder.y = shoulder.y.min(neck_y - 0.01 * h);
    if d.y.abs() > 0.94 {
        warnings.push(format!(
            "The {} arm hangs close to the body (I-pose); an A- or T-pose fits better. Check the shoulder and elbow markers.",
            side.name().to_lowercase()
        ));
    }

    // The elbow halfway from shoulder to wrist (Mannequin ratio 0.50), or at
    // the bend if the arm is bent.
    let elbow = bend(limb, wrist_i..merge, shoulder, wrist, 0.5, h);
    (shoulder, elbow, wrist)
}

/// The point `ratio` of the way from `a` to `b`, or the limb's bend if the
/// limb strays more than 0.04·h from the straight line.
fn bend(limb: &Limb, range: std::ops::Range<usize>, a: Vec3, b: Vec3, ratio: f32, h: f32) -> Vec3 {
    match limb.farthest_from(range, a, b) {
        Some((d, i)) if d > 0.04 * h => limb.points[i],
        _ => a.lerp(b, ratio),
    }
}

/// Hip, knee and ankle from a foot's limb.
fn leg(limb: &Limb, h: f32, crotch: f32) -> (Vec3, Vec3, Vec3) {
    let n = limb.points.len();
    let window = limb.indices_where(|i| {
        (0.02 * h..=0.16 * h).contains(&limb.points[i].y) && limb.tangents[i].y.abs() > 0.6
    });
    let ankle_i = limb
        .narrowest(&window, |i| (limb.points[i].y - 0.06 * h).abs())
        .unwrap_or_else(|| {
            (0..n)
                .min_by(|&a, &b| {
                    (limb.points[a].y - 0.06 * h)
                        .abs()
                        .total_cmp(&(limb.points[b].y - 0.06 * h).abs())
                })
                .unwrap_or(0)
        });
    let ankle = limb.points[ankle_i];
    let top = (ankle_i..n)
        .find(|&i| limb.points[i].y >= crotch)
        .unwrap_or(n - 1)
        .max(ankle_i + 1)
        .min(n - 1);
    let from = ankle_i + (top - ankle_i) / 2;
    let samples: Vec<Vec3> = if top - from >= 3 {
        limb.points[from..=top].to_vec()
    } else {
        limb.points[ankle_i..=top].to_vec()
    };
    let (c, d) = line_fit(&samples);
    // Mannequin: hip joint 0.04·h above the crotch.
    let hip_y = crotch + 0.04 * h;
    let hip = if d.y.abs() > 0.3 {
        c + d * ((hip_y - c.y) / d.y)
    } else {
        Vec3::new(c.x, hip_y, c.z)
    };
    // Mannequin: the knee 0.486 of the way from hip to ankle.
    let knee = bend(limb, ankle_i..top, hip, ankle, 0.486, h);
    (hip, knee, ankle)
}

/// Height of the crotch: tracking each foot's horizontal cross-section
/// upwards, the lowest layer where the two legs have become one piece.
/// Pieces that appear on the way (hands hanging beside the thighs) carry
/// neither leg's mark and are ignored.
fn crotch(grid: &Grid, left: Vec3, right: Vec3, h: f32) -> Option<f32> {
    let [nx, ny, nz] = grid.dims;
    let layer_of = |p: Vec3| (((p.y - grid.origin.y) / grid.cell) as usize).min(ny - 1);
    let column = |p: Vec3| {
        let q = (p - grid.origin) / grid.cell;
        (q.x as usize).min(nx - 1) + nx * (q.z as usize).min(nz - 1)
    };
    let seeds = [
        (layer_of(left), column(left), 1u8),
        (layer_of(right), column(right), 2u8),
    ];
    let start = seeds[0].0.min(seeds[1].0);
    let stop = ((0.7 * h - grid.origin.y) / grid.cell) as usize;
    let mut marks = vec![0u8; nx * nz];
    let mut comp = vec![u32::MAX; nx * nz];
    for y in start..stop.min(ny) {
        // Components of this layer.
        comp.fill(u32::MAX);
        let mut flags: Vec<u8> = Vec::new();
        for start_cell in 0..nx * nz {
            let (x, z) = (start_cell % nx, start_cell / nx);
            if comp[start_cell] != u32::MAX || !grid.solid[grid.index(x, y, z)] {
                continue;
            }
            let id = flags.len() as u32;
            flags.push(0);
            comp[start_cell] = id;
            let mut stack = vec![start_cell];
            while let Some(c) = stack.pop() {
                let (cx, cz) = ((c % nx) as i64, (c / nx) as i64);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let (ax, az) = (cx + dx, cz + dz);
                        if ax < 0 || az < 0 || ax >= nx as i64 || az >= nz as i64 {
                            continue;
                        }
                        let a = ax as usize + nx * az as usize;
                        if comp[a] == u32::MAX
                            && grid.solid[grid.index(ax as usize, y, az as usize)]
                        {
                            comp[a] = id;
                            stack.push(a);
                        }
                    }
                }
            }
        }
        for (c, &id) in comp.iter().enumerate() {
            if id != u32::MAX {
                flags[id as usize] |= marks[c];
            }
        }
        for &(layer, col, flag) in &seeds {
            if layer == y && comp[col] != u32::MAX {
                flags[comp[col] as usize] |= flag;
            }
        }
        if flags.contains(&3) {
            return Some(grid.origin.y + (y as f32 + 0.5) * grid.cell);
        }
        for (c, m) in marks.iter_mut().enumerate() {
            *m = if comp[c] == u32::MAX {
                0
            } else {
                flags[comp[c] as usize]
            };
        }
        let seeded = seeds.iter().all(|s| s.0 <= y);
        if seeded && !marks.iter().any(|&m| m != 0) {
            return None;
        }
    }
    None
}

/// The waist: 27% of the way from hips to neck (Mannequin 0.602·h between
/// 0.511 and 0.844), moved to the narrowest horizontal torso section nearby
/// when there clearly is one.
fn waist(grid: &Grid, hips: Vec3, neck: Vec3, cx: f32, h: f32) -> Vec3 {
    let span = neck.y - hips.y;
    let z = (neck.z + hips.z) / 2.;
    let mut y = hips.y + 0.27 * span;
    let samples: Vec<(f32, Section)> = (0..=10)
        .map(|k| {
            let y = hips.y + span * (0.15 + 0.025 * k as f32);
            (y, grid.section(Vec3::new(cx, y, z), Vec3::Y, 0.3 * h))
        })
        .filter(|(_, s)| s.found && !s.truncated)
        .collect();
    if samples.len() >= 3 {
        let ends = samples[0].1.area.min(samples[samples.len() - 1].1.area);
        if let Some((best_y, best)) = samples
            .iter()
            .min_by(|a, b| a.1.area.total_cmp(&b.1.area))
            .copied()
        {
            if best.area < 0.92 * ends {
                y = best_y;
            }
        }
    }
    let section = grid.section(Vec3::new(cx, y, z), Vec3::Y, 0.3 * h);
    let z = if section.found { section.centroid.z } else { z };
    Vec3::new(cx, y, z)
}
