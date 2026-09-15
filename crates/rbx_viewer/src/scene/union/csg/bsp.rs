//! The BSP tree behind [`super::Solid`]'s booleans: planes, convex polygons
//! and the clip/invert/build walks csg.js composes them from. Every walk is
//! iterative on purpose — the trees a carved rock builds run deep enough to
//! overflow a thread's stack when recursed.

use glam::DVec3;

// Distance under which a point counts as lying on a plane; csg.js's own value.
const EPSILON: f64 = 1e-5;

#[derive(Debug, Clone, Copy)]
pub(super) struct Plane {
    pub(super) normal: DVec3,
    pub(super) w: f64,
}

// Which side(s) of a plane a polygon's vertices fall on, OR-combined.
const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

impl Plane {
    /// `None` for a degenerate (collinear or non-finite) triangle.
    pub(super) fn from_points(a: DVec3, b: DVec3, c: DVec3) -> Option<Self> {
        let cross = (b - a).cross(c - a);
        if !cross.is_finite() || cross.length_squared() < 1e-24 {
            return None;
        }
        let normal = cross.normalize();
        Some(Plane {
            normal,
            w: normal.dot(a),
        })
    }

    fn flip(&mut self) {
        self.normal = -self.normal;
        self.w = -self.w;
    }

    /// Sorts `polygon` into the four buckets, cutting it in two when it
    /// straddles the plane. Each cut half's plane is recomputed from its own
    /// corners (matching csg.js's `Polygon` constructor) rather than reused
    /// from the parent: keeping the stale parent plane left a fragment whose
    /// actual vertices no longer satisfied it, and later splits against that
    /// mismatch is what produced leaks on real (rotated, many-cut) input —
    /// falls back to the parent plane only for a degenerate near-collinear cut.
    fn split(&self, polygon: Polygon, buckets: &mut Buckets) {
        let mut kind = COPLANAR;
        let types: Vec<u8> = polygon
            .vertices
            .iter()
            .map(|&vertex| {
                let distance = self.normal.dot(vertex) - self.w;
                let side = if distance < -EPSILON {
                    BACK
                } else if distance > EPSILON {
                    FRONT
                } else {
                    COPLANAR
                };
                kind |= side;
                side
            })
            .collect();

        match kind {
            COPLANAR => {
                if self.normal.dot(polygon.plane.normal) > 0.0 {
                    buckets.coplanar_front.push(polygon);
                } else {
                    buckets.coplanar_back.push(polygon);
                }
            }
            FRONT => buckets.front.push(polygon),
            BACK => buckets.back.push(polygon),
            _ => {
                let count = polygon.vertices.len();
                let mut front = Vec::with_capacity(count + 1);
                let mut back = Vec::with_capacity(count + 1);
                for i in 0..count {
                    let j = (i + 1) % count;
                    let (ti, tj) = (types[i], types[j]);
                    let (vi, vj) = (polygon.vertices[i], polygon.vertices[j]);
                    if ti != BACK {
                        front.push(vi);
                    }
                    if ti != FRONT {
                        back.push(vi);
                    }
                    if (ti | tj) == SPANNING {
                        let t = (self.w - self.normal.dot(vi)) / self.normal.dot(vj - vi);
                        let cut = vi.lerp(vj, t);
                        front.push(cut);
                        back.push(cut);
                    }
                }
                // Clipping a planar polygon can only ever truncate it — the
                // surviving front/back pieces still lie on `polygon`'s own
                // plane, so its *normal* never needs re-deriving. Only `w`
                // is refreshed here, from one of the fragment's own (possibly
                // lerp-drifted) vertices, so later splits see a plane that
                // matches this fragment's actual vertex data (the leak this
                // guarded against — see the doc above). Re-deriving the
                // normal too, via a fresh 3-point cross product, was the bug:
                // for a thin sliver fragment (common after many overlapping
                // cuts) that cross product is dominated by rounding noise, so
                // coplanar siblings of the same original face drifted apart
                // by a fraction of a degree each — invisible to
                // `is_watertight`'s position-only check, but glaringly
                // visible to a shader that shades per (near-)flat facet.
                if front.len() >= 3 {
                    let plane = Plane {
                        normal: polygon.plane.normal,
                        w: polygon.plane.normal.dot(front[0]),
                    };
                    buckets.front.push(Polygon {
                        vertices: front,
                        plane,
                    });
                }
                if back.len() >= 3 {
                    let plane = Plane {
                        normal: polygon.plane.normal,
                        w: polygon.plane.normal.dot(back[0]),
                    };
                    buckets.back.push(Polygon {
                        vertices: back,
                        plane,
                    });
                }
            }
        }
    }
}

#[derive(Default)]
struct Buckets {
    coplanar_front: Vec<Polygon>,
    coplanar_back: Vec<Polygon>,
    front: Vec<Polygon>,
    back: Vec<Polygon>,
}

/// A convex, planar face; the vertex ring winds counter-clockwise seen from
/// the side `plane.normal` points to.
#[derive(Debug, Clone)]
pub(super) struct Polygon {
    pub(super) vertices: Vec<DVec3>,
    pub(super) plane: Plane,
}

impl Polygon {
    fn flip(&mut self) {
        self.vertices.reverse();
        self.plane.flip();
    }
}

/// One BSP node: the polygons lying on `plane`, and the subtrees either side.
#[derive(Default)]
pub(super) struct BspNode {
    plane: Option<Plane>,
    front: Option<Box<BspNode>>,
    back: Option<Box<BspNode>>,
    polygons: Vec<Polygon>,
}

impl BspNode {
    pub(super) fn from_polygons(polygons: Vec<Polygon>) -> Self {
        let mut node = BspNode::default();
        node.build(polygons);
        node
    }

    /// Turns the solid inside out. Iterative, like every walk below: the
    /// trees built from a carved rock run deep enough to overflow a thread's
    /// stack when recursed.
    pub(super) fn invert(&mut self) {
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            node.polygons.iter_mut().for_each(Polygon::flip);
            if let Some(plane) = &mut node.plane {
                plane.flip();
            }
            std::mem::swap(&mut node.front, &mut node.back);
            stack.extend(node.front.as_deref_mut());
            stack.extend(node.back.as_deref_mut());
        }
    }

    /// Everything in `polygons` that lies outside this solid.
    fn clip_polygons(&self, polygons: Vec<Polygon>) -> Vec<Polygon> {
        let mut kept = Vec::new();
        let mut stack = vec![(self, polygons)];
        while let Some((node, polygons)) = stack.pop() {
            let Some(plane) = &node.plane else {
                kept.extend(polygons);
                continue;
            };
            let mut buckets = Buckets::default();
            for polygon in polygons {
                plane.split(polygon, &mut buckets);
            }
            let mut front = buckets.front;
            front.extend(buckets.coplanar_front);
            let mut back = buckets.back;
            back.extend(buckets.coplanar_back);
            match &node.front {
                Some(child) => stack.push((child, front)),
                None => kept.extend(front),
            }
            // No back child means the back half-space is solid: those polygons
            // are inside and vanish.
            if let Some(child) = &node.back {
                stack.push((child, back));
            }
        }
        kept
    }

    /// Removes every polygon of this tree that lies inside `other`.
    pub(super) fn clip_to(&mut self, other: &BspNode) {
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            node.polygons = other.clip_polygons(std::mem::take(&mut node.polygons));
            stack.extend(node.front.as_deref_mut());
            stack.extend(node.back.as_deref_mut());
        }
    }

    pub(super) fn all_polygons(&self) -> Vec<Polygon> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            out.extend(node.polygons.iter().cloned());
            stack.extend(node.front.as_deref());
            stack.extend(node.back.as_deref());
        }
        out
    }

    /// Adds `polygons` to the tree, taking each node's first polygon as its
    /// splitting plane when it has none yet.
    pub(super) fn build(&mut self, polygons: Vec<Polygon>) {
        let mut stack = vec![(self, polygons)];
        while let Some((node, polygons)) = stack.pop() {
            let Some(first) = polygons.first() else {
                continue;
            };
            let plane = *node.plane.get_or_insert(first.plane);
            let mut buckets = Buckets::default();
            for polygon in polygons {
                plane.split(polygon, &mut buckets);
            }
            node.polygons.extend(buckets.coplanar_front);
            node.polygons.extend(buckets.coplanar_back);
            if !buckets.front.is_empty() {
                stack.push((node.front.get_or_insert_default(), buckets.front));
            }
            if !buckets.back.is_empty() {
                stack.push((node.back.get_or_insert_default(), buckets.back));
            }
        }
    }
}
