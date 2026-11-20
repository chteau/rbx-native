//! The workspace's parts as parry shapes, for `WorldRoot` spatial queries
//! (`Raycast`, `Blockcast`, `GetPartBoundsInBox`, ...).
//!
//! The index answers for the DOM as it is now: it is rebuilt when physics
//! stepped since it was built, or when scripts changed the DOM.

use rapier3d::parry::bounding_volume::{Aabb, BoundingVolume};
use rapier3d::parry::partitioning::{Bvh, BvhBuildStrategy, BvhNode};
use rapier3d::parry::query::{cast_shapes, intersection_test, Ray, ShapeCastOptions};
use rapier3d::parry::shape::Shape;
use rapier3d::prelude::*;
use std::collections::HashMap;

use rbx_dom::{Ref, WeakDom};
use rbx_lua::server::{CollisionGroups, QueryFilter, QueryHit, QueryShape};
use rbx_reflection::ReflectionDatabase;

use crate::physics::part::PartData;
use crate::physics::pose_of;

/// A hit at time of impact zero is a start inside, not a hit.
const EPSILON: Real = 1e-6;

#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) referent: Ref,
    pub(crate) shape: SharedShape,
    pub(crate) pose: Pose,
    pub(crate) size: [Real; 3],
    pub(crate) can_query: bool,
    pub(crate) can_collide: bool,
    pub(crate) group: String,
}

/// The raw properties a part's query shape and flags come from: while they
/// stay the same, a rebuild only moves the entry to the part's new CFrame.
const SHAPE_KEYS: &[&str] = &[
    "Size",
    "size",
    "Shape",
    "MeshId",
    "MeshContent",
    "CollisionFidelity",
    "CanQuery",
    "CanCollide",
    "CollisionGroup",
    "PhysicalConfigData",
    "InitialSize",
];

/// What a part's entry was built from, and the entry (`None` for a part
/// with no query shape).
struct Built {
    class: String,
    keys: Vec<Option<rbx_dom::Variant>>,
    entry: Option<Entry>,
}

#[derive(Default)]
pub struct Spatial {
    entries: Vec<Entry>,
    /// The entries' world boxes, so a query only tests the parts near it.
    tree: Bvh,
    /// Per part, what its entry was last built from.
    built_from: HashMap<Ref, Built>,
    /// Per class, whether it's a `BasePart`.
    part_classes: HashMap<String, bool>,
    built: bool,
    generation: u64,
    seen_changes: usize,
    /// Built over a non-empty change log, which the VM drains without
    /// telling us: until physics steps, nothing can be trusted.
    unsettled: bool,
}

fn to_vec(v: [f64; 3]) -> Vector {
    Vector::new(v[0], v[1], v[2])
}

fn to_array(v: Vector) -> [f64; 3] {
    v.to_array()
}

fn allowed(
    entry: &Entry,
    filter: &QueryFilter,
    groups: &CollisionGroups,
    skip: Option<Ref>,
) -> bool {
    entry.can_query
        && Some(entry.referent) != skip
        && (!filter.respect_can_collide || entry.can_collide)
        && groups.collides(&entry.group, &filter.collision_group)
        && filter.parts.contains(&entry.referent) == filter.include
}

impl Spatial {
    /// Builds an index straight from entries.
    #[cfg(test)]
    pub(crate) fn from_entries(entries: Vec<Entry>) -> Self {
        let mut spatial = Spatial {
            entries,
            ..Default::default()
        };
        spatial.index();
        spatial
    }

    /// Rebuilds from `dom` unless nothing could have changed since the
    /// last build.
    pub fn refresh(&mut self, dom: &WeakDom, db: &ReflectionDatabase, generation: u64) {
        let changes = dom.changes().len();
        if self.built
            && self.generation == generation
            && self.seen_changes == changes
            && !self.unsettled
        {
            return;
        }
        let mut previous = std::mem::take(&mut self.built_from);
        self.entries.clear();
        if let Some(workspace) = dom
            .root_refs()
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
        {
            let mut stack = vec![workspace];
            while let Some(at) = stack.pop() {
                let Some(instance) = dom.get(at) else {
                    continue;
                };
                stack.extend(instance.children().iter().rev());
                let class = instance.class();
                let is_part = match self.part_classes.get(class) {
                    Some(known) => *known,
                    None => {
                        let known = db.is_subclass_of(class, "BasePart");
                        self.part_classes.insert(class.to_owned(), known);
                        known
                    }
                };
                if !is_part {
                    continue;
                }
                let properties = instance.properties();
                let keys: Vec<_> = SHAPE_KEYS
                    .iter()
                    .map(|k| properties.get(*k).cloned())
                    .collect();
                let mut built = match previous.remove(&at) {
                    Some(old) if old.class == class && old.keys == keys => old,
                    _ => Built {
                        class: class.to_owned(),
                        keys,
                        entry: PartData::read(db, instance).map(|part| Entry {
                            referent: part.referent,
                            shape: part.shape_collider().build().shared_shape().clone(),
                            pose: pose_of(&part.cframe),
                            size: [part.size.x, part.size.y, part.size.z].map(Real::from),
                            can_query: part.can_query,
                            can_collide: part.can_collide,
                            group: part.collision_group,
                        }),
                    },
                };
                if let Some(entry) = &mut built.entry {
                    if let Some(rbx_dom::Variant::CFrame(cf)) = properties.get("CFrame") {
                        entry.pose = pose_of(cf);
                    }
                    self.entries.push(entry.clone());
                }
                self.built_from.insert(at, built);
            }
        }
        self.index();
        self.built = true;
        self.generation = generation;
        self.seen_changes = changes;
        self.unsettled = changes > 0;
    }

    fn index(&mut self) {
        self.tree = Bvh::from_iter(
            BvhBuildStrategy::Binned,
            self.entries
                .iter()
                .enumerate()
                .map(|(i, e)| (i, e.shape.compute_aabb(&e.pose))),
        );
    }

    /// The entries whose box `hits`, in tree order.
    fn near(&self, hits: impl Fn(&BvhNode) -> bool) -> Vec<&Entry> {
        let mut found: Vec<u32> = self.tree.leaves(hits).collect();
        found.sort_unstable();
        found
            .into_iter()
            .map(|i| &self.entries[i as usize])
            .collect()
    }

    /// The entries whose box meets `aabb`, in tree order.
    fn near_box(&self, aabb: &Aabb) -> Vec<&Entry> {
        let mut found: Vec<u32> = self.tree.intersect_aabb(aabb).collect();
        found.sort_unstable();
        found
            .into_iter()
            .map(|i| &self.entries[i as usize])
            .collect()
    }

    fn entry(&self, part: Ref) -> Option<&Entry> {
        self.entries.iter().find(|e| e.referent == part)
    }

    /// The query shape, and the part it must never hit.
    fn resolve(&self, shape: &QueryShape) -> Option<(SharedShape, Option<Ref>)> {
        match shape {
            QueryShape::Box(size) => {
                let half = to_vec(*size) / 2.0;
                Some((SharedShape::cuboid(half.x, half.y, half.z), None))
            }
            QueryShape::Ball(radius) => Some((SharedShape::ball(*radius), None)),
            QueryShape::Part(part) => self.entry(*part).map(|e| (e.shape.clone(), Some(*part))),
        }
    }

    pub fn raycast(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        filter: &QueryFilter,
        groups: &CollisionGroups,
    ) -> Option<QueryHit> {
        let (origin, dir) = (to_vec(origin), to_vec(direction));
        let length = dir.length();
        if length <= 0.0 {
            return None;
        }
        let ray = Ray::new(origin, dir);
        let mut best: Option<(Real, Vector, Ref)> = None;
        for entry in self
            .near(|node| node.cast_ray(&ray, 1.0) < Real::MAX)
            .into_iter()
            .filter(|e| allowed(e, filter, groups, None))
        {
            let shape: &dyn Shape = &*entry.shape;
            let Some(hit) = shape.cast_ray_and_get_normal(&entry.pose, &ray, 1.0, true) else {
                continue;
            };
            let toi = hit.time_of_impact;
            if toi > EPSILON && best.is_none_or(|(t, _, _)| toi < t) {
                best = Some((toi, hit.normal, entry.referent));
            }
        }
        let (toi, normal, part) = best?;
        Some(QueryHit {
            part,
            position: to_array(origin + dir * toi),
            normal: to_array(normal),
            distance: toi * length,
            material: None,
        })
    }

    pub fn shapecast(
        &self,
        shape: &QueryShape,
        from: &rbx_dom::CFrameData,
        direction: [f64; 3],
        filter: &QueryFilter,
        groups: &CollisionGroups,
    ) -> Option<QueryHit> {
        let (moving, skip) = self.resolve(shape)?;
        let start = pose_of(from);
        let dir = to_vec(direction);
        let length = dir.length();
        if length <= 0.0 {
            return None;
        }
        let moving: &dyn Shape = &*moving;
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let mut best: Option<(Real, Vector, Vector, Ref)> = None;
        let from_box = moving.compute_aabb(&start);
        let swept = from_box.merged(&Aabb::new(from_box.mins + dir, from_box.maxs + dir));
        for entry in self
            .near_box(&swept)
            .into_iter()
            .filter(|e| allowed(e, filter, groups, skip))
        {
            let target: &dyn Shape = &*entry.shape;
            let overlapping = intersection_test(&start, moving, &entry.pose, target)
                .map(|i| i.intersecting)
                .unwrap_or(false);
            if overlapping {
                continue;
            }
            let Ok(Some(hit)) = cast_shapes(
                &start,
                dir,
                moving,
                &entry.pose,
                Vector::ZERO,
                target,
                options,
            ) else {
                continue;
            };
            if best.is_none_or(|(t, _, _, _)| hit.time_of_impact < t) {
                best = Some((
                    hit.time_of_impact,
                    entry.pose.transform_point(hit.witness2),
                    entry.pose.rotation * hit.normal2,
                    entry.referent,
                ));
            }
        }
        let (toi, position, normal, part) = best?;
        Some(QueryHit {
            part,
            position: to_array(position),
            normal: to_array(normal),
            distance: toi * length,
            material: None,
        })
    }

    pub fn overlap(
        &self,
        shape: &QueryShape,
        at: &rbx_dom::CFrameData,
        exact: bool,
        filter: &QueryFilter,
        groups: &CollisionGroups,
    ) -> Vec<Ref> {
        let Some((query, skip)) = self.resolve(shape) else {
            return Vec::new();
        };
        let pose = pose_of(at);
        let query: &dyn Shape = &*query;
        let limit = if filter.max_parts == 0 {
            usize::MAX
        } else {
            filter.max_parts
        };
        // The inexact test reads each part's own box, which its world box
        // holds, so the world boxes are a sound first cut for both.
        let area = query.compute_aabb(&pose);
        self.near_box(&area)
            .into_iter()
            .filter(|e| allowed(e, filter, groups, skip))
            .filter(|entry| {
                let hit = if exact {
                    intersection_test(&pose, query, &entry.pose, &*entry.shape)
                } else {
                    let half = Vector::new(entry.size[0], entry.size[1], entry.size[2]) / 2.0;
                    let oriented = SharedShape::cuboid(half.x, half.y, half.z);
                    intersection_test(&pose, query, &entry.pose, &*oriented)
                };
                hit.map(|i| i.intersecting).unwrap_or(false)
            })
            .map(|e| e.referent)
            .take(limit)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rbx_dom::CFrameData;

    use super::*;

    static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

    fn block(at: [Real; 3], size: [Real; 3]) -> Entry {
        Entry {
            referent: Ref::new(NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
            shape: SharedShape::cuboid(size[0] / 2.0, size[1] / 2.0, size[2] / 2.0),
            pose: Pose::from_parts(Vector::new(at[0], at[1], at[2]), Rotation::IDENTITY),
            size,
            can_query: true,
            can_collide: true,
            group: "Default".to_owned(),
        }
    }

    fn filter() -> QueryFilter {
        QueryFilter {
            parts: HashSet::new(),
            include: false,
            respect_can_collide: false,
            collision_group: "Default".to_owned(),
            max_parts: 0,
            ignore_water: false,
        }
    }

    fn cframe(x: f32, y: f32, z: f32) -> CFrameData {
        CFrameData {
            position: rbx_dom::Vector3Data { x, y, z },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn ray_hits_the_near_face() {
        let wall = block([0.0, 0.0, 10.0], [4.0, 4.0, 4.0]);
        let part = wall.referent;
        let spatial = Spatial::from_entries(vec![wall]);
        let hit = spatial
            .raycast(
                [0.0; 3],
                [0.0, 0.0, 30.0],
                &filter(),
                &CollisionGroups::default(),
            )
            .expect("hit");
        assert_eq!(hit.part, part);
        assert!((hit.distance - 8.0).abs() < 1e-4);
        assert!((hit.position[2] - 8.0).abs() < 1e-4);
        assert!((hit.normal[2] + 1.0).abs() < 1e-4);
    }

    #[test]
    fn ray_from_inside_misses() {
        let spatial = Spatial::from_entries(vec![block([0.0, 0.0, 10.0], [4.0, 4.0, 4.0])]);
        let hit = spatial.raycast(
            [0.0, 0.0, 10.0],
            [0.0, 0.0, 30.0],
            &filter(),
            &CollisionGroups::default(),
        );
        assert!(hit.is_none());
    }

    #[test]
    fn blockcast_ignores_what_it_starts_in() {
        let spatial = Spatial::from_entries(vec![block([0.0; 3], [4.0, 4.0, 4.0])]);
        let hit = spatial.shapecast(
            &QueryShape::Box([2.0; 3]),
            &cframe(0.0, 0.0, 0.0),
            [0.0, 0.0, 30.0],
            &filter(),
            &CollisionGroups::default(),
        );
        assert!(hit.is_none());
    }

    #[test]
    fn bounds_query_finds_overlapping_boxes() {
        let near = block([0.0; 3], [4.0, 4.0, 4.0]);
        let found = near.referent;
        let spatial = Spatial::from_entries(vec![near, block([20.0, 0.0, 0.0], [4.0; 3])]);
        let parts = spatial.overlap(
            &QueryShape::Box([2.0; 3]),
            &cframe(2.5, 0.0, 0.0),
            false,
            &filter(),
            &CollisionGroups::default(),
        );
        assert_eq!(parts, vec![found]);
    }
}
