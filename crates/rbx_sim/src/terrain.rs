//! Smooth terrain at run time: the voxels (read from `Terrain.SmoothGrid`
//! and written back after every edit, so replication and the renderer see
//! them), the `Terrain` API's edits, a collision mesh per chunk, and the
//! water that floats parts and swims characters.

use std::collections::BTreeMap;

use rapier3d::parry::query::{Ray, RayCullingMode};
use rapier3d::prelude::*;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::server::{QueryHit, TerrainOp, TerrainReply};
use rbx_terrain::{mesh, smooth_grid, Cell, ChunkKey, Material, MaterialColors, VoxelGrid};

/// Studs along a voxel edge.
const CELL: Real = 4.0;

/// The collision shapes of one chunk, one per material, and the stamp of
/// their build.
pub(crate) type ChunkShapes = (u64, Vec<(Material, SharedShape)>);

#[derive(Default)]
pub(crate) struct Terrain {
    pub(crate) referent: Option<Ref>,
    pub(crate) grid: VoxelGrid,
    colors: MaterialColors,
    encoder: smooth_grid::Encoder,
    /// The `SmoothGrid` bytes last read or written, to tell our own write
    /// from a replicated one.
    bytes: Vec<u8>,
    pub(crate) shapes: BTreeMap<ChunkKey, ChunkShapes>,
    /// Chunks edited since `shapes` were last brought up to date.
    pending: std::collections::BTreeSet<ChunkKey>,
    /// Bumped whenever `shapes` change.
    pub(crate) version: u64,
}

/// A `BinaryString` property's bytes, whichever variant holds them.
fn blob(value: Option<&Variant>) -> &[u8] {
    match value {
        Some(Variant::String(text)) => text.as_bytes(),
        Some(Variant::Unknown { raw, .. }) => raw,
        _ => &[],
    }
}

/// Bytes as the binary format's `BinaryString` (type 1).
fn binary(bytes: Vec<u8>) -> Variant {
    Variant::Unknown {
        type_id: 1,
        raw: bytes,
    }
}

/// The workspace's `Terrain`.
pub(crate) fn find(dom: &WeakDom) -> Option<Ref> {
    let workspace = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))?;
    dom.get(workspace)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.class() == "Terrain"))
}

fn material(name: &str) -> Result<Material, String> {
    Material::from_name(name).ok_or_else(|| format!("{name} is not a terrain material"))
}

/// The colours a server gives terrain whose `MaterialColors` was never
/// set, which are not what Studio writes into a new place
/// (TerrainProfile `_colors`).
fn engine_colors() -> MaterialColors {
    const COLORS: [(&str, [u8; 3]); 21] = [
        ("WoodPlanks", [139, 109, 79]),
        ("Basalt", [30, 30, 37]),
        ("Slate", [63, 127, 107]),
        ("CrackedLava", [232, 156, 74]),
        ("Concrete", [127, 102, 63]),
        ("Limestone", [206, 173, 148]),
        ("Pavement", [148, 148, 140]),
        ("Brick", [138, 86, 62]),
        ("Cobblestone", [132, 123, 90]),
        ("Rock", [102, 108, 111]),
        ("Sandstone", [137, 90, 71]),
        ("Grass", [106, 127, 63]),
        ("LeafyGrass", [115, 132, 74]),
        ("Sand", [143, 126, 95]),
        ("Snow", [195, 199, 218]),
        ("Mud", [58, 46, 36]),
        ("Ground", [102, 92, 59]),
        ("Asphalt", [115, 123, 107]),
        ("Salt", [198, 189, 181]),
        ("Ice", [129, 194, 224]),
        ("Glacier", [101, 176, 234]),
    ];
    let mut colors = MaterialColors::default();
    for (name, rgb) in COLORS {
        if let Some(material) = Material::from_name(name) {
            colors.set(material, rgb);
        }
    }
    colors
}

/// How full a cell is of a shape whose signed distance (studs, negative
/// inside, the largest of the per-axis distances for a box) is `d` at its
/// centre: a cell half covered along an axis is half full, as Studio's
/// `FillBlock` leaves it (TerrainFill `block`, `rotated`, `ball`).
fn coverage(d: Real) -> Real {
    (0.5 - d / CELL).clamp(0.0, 1.0)
}

/// A shape in its own frame: half extents for the cells it can reach, and
/// how full it makes a cell whose centre is at a local point.
struct Shape {
    pose: Pose,
    reach: Vector,
    fill: Box<dyn Fn(Vector) -> Real>,
}

fn pose(cframe: &rbx_dom::CFrameData) -> Pose {
    crate::physics::pose_of(cframe)
}

fn box_distance(p: Vector, half: Vector) -> Real {
    (p.abs() - half).max_element()
}

impl Shape {
    fn of(op: &TerrainOp) -> Option<(Shape, &str)> {
        let (pose, reach, fill, material) = match op {
            TerrainOp::FillBlock {
                cframe,
                size,
                material,
            } => {
                let half = Vector::from_array(*size) / 2.0;
                let fill = move |p| coverage(box_distance(p, half));
                (
                    pose(cframe),
                    half,
                    Box::new(fill) as Box<dyn Fn(Vector) -> Real>,
                    material,
                )
            }
            TerrainOp::FillBall {
                center,
                radius,
                material,
            } => {
                let r = *radius;
                let fill = move |p: Vector| coverage(p.length() - r);
                let at = Pose::from_translation(Vector::from_array(*center));
                (at, Vector::splat(r), Box::new(fill) as _, material)
            }
            // Measured, not derived (TerrainFill `cylinder*`, `cyl_*`): any
            // cell the height overlaps at all is in, and across the axis a
            // cell is (radius - r) / 4 full, r its centre's distance from
            // the axis (a ball's rule less half a cell).
            TerrainOp::FillCylinder {
                cframe,
                height,
                radius,
                material,
            } => {
                let (r, h) = (*radius, *height / 2.0);
                let fill = move |p: Vector| {
                    if p.y.abs() >= h + CELL / 2.0 {
                        return 0.0;
                    }
                    ((r - Vector::new(p.x, 0.0, p.z).length()) / CELL).clamp(0.0, 1.0)
                };
                (
                    pose(cframe),
                    Vector::new(r, h, r),
                    Box::new(fill) as _,
                    material,
                )
            }
            // A WedgePart's shape: full height at +Z, nothing at -Z. A cell
            // is filled as a block would be up to the slope's height at its
            // low edge (half a cell toward -Z), and not at all once that
            // edge is past the wedge's high end (TerrainFill `wedge*`,
            // TerrainProfile `voxels*`).
            TerrainOp::FillWedge {
                cframe,
                size,
                material,
            } => {
                let half = Vector::from_array(*size) / 2.0;
                let fill = move |p: Vector| {
                    let low = p.z - CELL / 2.0;
                    if low >= half.z {
                        return 0.0;
                    }
                    let top = if half.z > 0.0 {
                        half.y * low / half.z
                    } else {
                        half.y
                    };
                    let sides = (p.x.abs() - half.x).max(p.y.abs() - half.y);
                    coverage(sides).min(coverage(p.y - top))
                };
                (pose(cframe), half, Box::new(fill) as _, material)
            }
            _ => return None,
        };
        Some((Shape { pose, reach, fill }, material))
    }

    /// The cells whose centres it can reach, `max` exclusive.
    fn cells(&self) -> ([i32; 3], [i32; 3]) {
        let (mut lo, mut hi) = (Vector::splat(Real::MAX), Vector::splat(Real::MIN));
        for i in 0..8 {
            let corner = Vector::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            ) * (self.reach + Vector::splat(CELL / 2.0));
            let p = self.pose * corner;
            lo = lo.min(p);
            hi = hi.max(p);
        }
        (
            (lo / CELL).floor().to_array().map(|v| v as i32),
            (hi / CELL).ceil().to_array().map(|v| v as i32),
        )
    }

    fn coverage(&self, v: [i32; 3]) -> f32 {
        let centre = (Vector::new(v[0] as Real, v[1] as Real, v[2] as Real) + 0.5) * CELL;
        (self.fill)(self.pose.inverse() * centre) as f32
    }
}

/// A cell `fraction` full as the shape fills store it: the byte nearest
/// `fraction * 255` (TerrainFill: a 0.375 overlap reads back 97/256), where
/// `WriteVoxels` truncates (`Cell::with_fraction`).
fn filled(material: Material, fraction: f32) -> Cell {
    let byte = (fraction.clamp(0.0, 1.0) * 255.0).round();
    if byte <= 0.0 {
        Cell::AIR
    } else {
        Cell::new(material, byte as u8, 0)
    }
}

/// A ray against one collision mesh, seeing only one side of it.
fn cull(
    shape: &SharedShape,
    ray: &Ray,
    max: Real,
    culling: RayCullingMode,
) -> Option<rapier3d::parry::query::RayIntersection> {
    shape
        .as_trimesh()?
        .cast_local_ray_with_culling(ray, max, culling)
}

/// What `cell` becomes when a fill covers `fill` of it with `material`:
/// the region fill's rule (never emptier than it was), Air carving away
/// (TerrainFill `carved`: 0.75 of Air leaves 65/256).
fn merged(cell: Cell, material: Material, fill: f32) -> Option<Cell> {
    if fill <= 0.0 {
        return None;
    }
    if material == Material::Air {
        if cell.is_air() || fill < 1.0 - cell.fraction() {
            return None;
        }
        return Some(filled(cell.material, 1.0 - fill));
    }
    // A full cell of solid takes a full cell of water over; less leaves it
    // water (Swim's shelf, rock filled into the lake: its top cell, a
    // quarter rock, stays water).
    let over = fill > cell.fraction()
        || (fill >= 1.0 && cell.material == Material::Water && material != Material::Water);
    if over || cell.material == material {
        Some(filled(material, fill.max(cell.fraction())))
    } else {
        None
    }
}

impl Terrain {
    /// Reads the workspace's terrain from `dom`.
    pub(crate) fn load(&mut self, dom: &WeakDom) {
        *self = Terrain::default();
        let Some(referent) = find(dom) else {
            return;
        };
        self.referent = Some(referent);
        let properties = dom.get(referent).map(|i| i.properties());
        let colors = properties.and_then(|p| p.get("MaterialColors").cloned());
        self.colors = MaterialColors::decode(blob(colors.as_ref())).unwrap_or_else(engine_colors);
        self.reload(properties.map_or(&[][..], |p| blob(p.get("SmoothGrid"))));
    }

    fn reload(&mut self, bytes: &[u8]) {
        self.grid = smooth_grid::decode(bytes).unwrap_or_default();
        self.bytes = bytes.to_vec();
        self.shapes.clear();
        self.pending = mesh::meshable_chunks(&self.grid);
        self.grid.take_dirty();
        self.version += 1;
    }

    /// `SmoothGrid` changed in the DOM (replicated from the server): reads
    /// it unless it is what this side wrote.
    pub(crate) fn changed(&mut self, dom: &WeakDom, referent: Ref) {
        if Some(referent) != self.referent {
            return;
        }
        let Some(instance) = dom.get(referent) else {
            return;
        };
        let bytes = blob(instance.properties().get("SmoothGrid"));
        if bytes != self.bytes.as_slice() {
            let bytes = bytes.to_vec();
            self.reload(&bytes);
        }
        if let Some(colors) =
            MaterialColors::decode(blob(instance.properties().get("MaterialColors")))
        {
            self.colors = colors;
        }
    }

    /// Rebuilds the collision shapes of the chunks edited since the last
    /// call: once a frame, not once an edit.
    pub(crate) fn flush(&mut self) {
        for key in std::mem::take(&mut self.pending) {
            let shapes = self.chunk_shapes(key);
            self.version += 1;
            if shapes.is_empty() {
                self.shapes.remove(&key);
            } else {
                self.shapes.insert(key, (self.version, shapes));
            }
        }
    }

    fn chunk_shapes(&self, key: ChunkKey) -> Vec<(Material, SharedShape)> {
        let triangles = self.collision_triangles(key);
        if triangles.is_empty() {
            return Vec::new();
        }
        let mut by_material: BTreeMap<u8, (Vec<Vector>, Vec<[u32; 3]>)> = BTreeMap::new();
        for [a, b, c] in triangles {
            let material = self.solid_near((a + b + c) / 3.0);
            let (points, indices) = by_material.entry(material.slot()).or_default();
            let base = points.len() as u32;
            points.extend([a, b, c]);
            indices.push([base, base + 1, base + 2]);
        }
        by_material
            .into_iter()
            .filter_map(|(slot, (points, indices))| {
                let material = Material::from_slot(slot)?;
                // Oriented, so a ray or a body inside knows it is inside.
                let shape = SharedShape::trimesh_with_flags(
                    points.clone(),
                    indices.clone(),
                    TriMeshFlags::ORIENTED
                        | TriMeshFlags::MERGE_DUPLICATE_VERTICES
                        | TriMeshFlags::FIX_INTERNAL_EDGES,
                )
                .or_else(|_| {
                    SharedShape::trimesh_with_flags(
                        points,
                        indices,
                        TriMeshFlags::MERGE_DUPLICATE_VERTICES,
                    )
                })
                .ok()?;
                Some((material, shape))
            })
            .collect()
    }

    /// The collision surface of one chunk, as triangles in studs. It is not
    /// the drawn surface: Roblox's collides half a cell further out
    /// (TerrainRaycast: a slab filled to a cell boundary is hit 2 studs
    /// above it, one filled 1, 2, 3 studs into its top cell 1, 0, -1 stud
    /// above that boundary). That is a surface net over "any solid at all",
    /// each crossing `f` of the way from a solid voxel's centre (`f` its
    /// fill) toward the empty one, its vertex in each dual cell the mean of
    /// its crossings. The faces between a voxel and the one above, north
    /// or east of it belong to the voxel's chunk.
    fn collision_triangles(&self, key: ChunkKey) -> Vec<[Vector; 3]> {
        let fill = |v: [i32; 3]| self.grid.get(v).solid_fraction();
        let centre =
            |v: [i32; 3]| (Vector::new(v[0] as Real, v[1] as Real, v[2] as Real) + 0.5) * CELL;
        let mut vertices: std::collections::HashMap<[i32; 3], Option<Vector>> = Default::default();
        // The vertex of the dual cell whose least corner is voxel `v`.
        let mut vertex = |v: [i32; 3]| -> Option<Vector> {
            *vertices.entry(v).or_insert_with(|| {
                let corner = |i: usize| {
                    [
                        v[0] + (i & 1) as i32,
                        v[1] + ((i >> 1) & 1) as i32,
                        v[2] + ((i >> 2) & 1) as i32,
                    ]
                };
                let (mut sum, mut n) = (Vector::ZERO, 0.0);
                for i in 0..8 {
                    for bit in [1, 2, 4] {
                        let j = i | bit;
                        if j == i {
                            continue;
                        }
                        let (a, b) = (corner(i), corner(j));
                        let (fa, fb) = (fill(a), fill(b));
                        if (fa > 0.0) == (fb > 0.0) {
                            continue;
                        }
                        let (solid, empty, f) = if fa > 0.0 { (a, b, fa) } else { (b, a, fb) };
                        sum += centre(solid).lerp(centre(empty), Real::from(f));
                        n += 1.0;
                    }
                }
                (n > 0.0).then(|| sum / n)
            })
        };
        let origin = key.origin();
        let mut triangles = Vec::new();
        for y in 0..rbx_terrain::CHUNK {
            for z in 0..rbx_terrain::CHUNK {
                for x in 0..rbx_terrain::CHUNK {
                    let v = [origin[0] + x, origin[1] + y, origin[2] + z];
                    let here = fill(v) > 0.0;
                    for axis in 0..3 {
                        let mut n = v;
                        n[axis] += 1;
                        if (fill(n) > 0.0) == here {
                            continue;
                        }
                        let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
                        let cell = |db: i32, dc: i32| {
                            let mut m = v;
                            m[b] -= db;
                            m[c] -= dc;
                            m
                        };
                        let quad =
                            [cell(0, 0), cell(1, 0), cell(1, 1), cell(0, 1)].map(&mut vertex);
                        // Wound to face the empty side.
                        if let [Some(p0), Some(p1), Some(p2), Some(p3)] = quad {
                            if here {
                                triangles.push([p0, p1, p3]);
                                triangles.push([p1, p2, p3]);
                            } else {
                                triangles.push([p0, p3, p1]);
                                triangles.push([p1, p3, p2]);
                            }
                        }
                    }
                }
            }
        }
        triangles
    }

    /// The solid material the surface at `p` belongs to: the fullest solid
    /// voxel of the eight whose centres surround it.
    pub(crate) fn solid_near(&self, p: Vector) -> Material {
        let base = (p / CELL).floor();
        let mut best = (0.0, Real::MAX, Material::Air);
        for i in 0..27 {
            let v = [i % 3, (i / 3) % 3, i / 9].map(|d| d - 1);
            let v = [
                base.x as i32 + v[0],
                base.y as i32 + v[1],
                base.z as i32 + v[2],
            ];
            let cell = self.grid.get(v);
            let fill = cell.solid_fraction();
            let far =
                ((Vector::new(v[0] as Real, v[1] as Real, v[2] as Real) + 0.5) * CELL).distance(p);
            if fill > best.0 || (fill == best.0 && fill > 0.0 && far < best.1) {
                best = (fill, far, cell.material);
            }
        }
        best.2
    }

    /// How much water is at `p`, 0 to 1: the voxels' water fractions
    /// blended between their centres, so 0.5 marks the surface.
    pub(crate) fn water_at(&self, p: Vector) -> Real {
        let q = p / CELL - 0.5;
        let base = q.floor();
        let t = q - base;
        let mut sum = 0.0;
        for i in 0..8 {
            let (dx, dy, dz) = (i & 1, (i >> 1) & 1, (i >> 2) & 1);
            let v = [base.x as i32 + dx, base.y as i32 + dy, base.z as i32 + dz];
            let w = self.grid.get(v).water_fraction();
            if w == 0.0 {
                continue;
            }
            let weight = (if dx == 1 { t.x } else { 1.0 - t.x })
                * (if dy == 1 { t.y } else { 1.0 - t.y })
                * (if dz == 1 { t.z } else { 1.0 - t.z });
            sum += weight * Real::from(w);
        }
        sum
    }

    pub(crate) fn has_water(&self) -> bool {
        self.grid.materials_in(None)[Material::Water.slot() as usize]
    }

    /// One `Terrain` method. Returns the reply and whether the voxels
    /// changed.
    pub(crate) fn apply(&mut self, op: TerrainOp) -> Result<(TerrainReply, bool), String> {
        match &op {
            TerrainOp::Read { min, max } => {
                let size = [0, 1, 2].map(|a| (max[a] - min[a]).max(0) as usize);
                let mut materials = Vec::with_capacity(size.iter().product());
                let mut occupancies = Vec::with_capacity(materials.capacity());
                for z in 0..size[2] {
                    for y in 0..size[1] {
                        for x in 0..size[0] {
                            let cell = self.grid.get([
                                min[0] + x as i32,
                                min[1] + y as i32,
                                min[2] + z as i32,
                            ]);
                            materials.push(cell.material.name().to_owned());
                            occupancies.push(f64::from(cell.fraction()));
                        }
                    }
                }
                return Ok((
                    TerrainReply::Voxels {
                        size,
                        materials,
                        occupancies,
                    },
                    false,
                ));
            }
            TerrainOp::GetColor(name) | TerrainOp::SetColor(name, _)
                if matches!(name.as_str(), "Air" | "Water") =>
            {
                // Neither has a colour (TerrainProfile `_colors`).
                return Err(format!("{name} is not a valid terrain material"));
            }
            TerrainOp::GetColor(name) => {
                let [r, g, b] = self.colors.get(material(name)?);
                return Ok((
                    TerrainReply::Color([r, g, b].map(|c| f32::from(c) / 255.0)),
                    false,
                ));
            }
            TerrainOp::SetColor(name, color) => {
                let m = material(name)?;
                self.colors
                    .set(m, color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
                return Ok((TerrainReply::Done, true));
            }
            TerrainOp::Clear => self.grid.clear(),
            TerrainOp::FillRegion {
                min,
                max,
                material: m,
            } => {
                let m = material(m)?;
                self.each(*min, *max, |_, _| Some(Cell::full(m)));
            }
            TerrainOp::ReplaceMaterial { min, max, from, to } => {
                let (from, to) = (material(from)?, material(to)?);
                self.each(*min, *max, |_, cell| {
                    (cell.material == from).then(|| match (from, to) {
                        (_, Material::Air) => Cell::AIR,
                        (Material::Air, to) => Cell::full(to),
                        (_, to) => Cell::new(to, cell.occupancy, cell.liquid),
                    })
                });
            }
            TerrainOp::Write {
                min,
                size,
                materials,
                occupancies,
            } => {
                let max = [0, 1, 2].map(|a| min[a] + size[a] as i32);
                let mut written = Vec::with_capacity(materials.len());
                for (name, occ) in materials.iter().zip(occupancies) {
                    written.push(Cell::with_fraction(material(name)?, *occ as f32));
                }
                self.each(*min, max, |v, _| {
                    let [x, y, z] = [0, 1, 2].map(|a| (v[a] - min[a]) as usize);
                    Some(written[x + size[0] * (y + size[1] * z)])
                });
            }
            _ => {
                let (shape, name) = Shape::of(&op).ok_or("unknown terrain edit")?;
                let m = material(name)?;
                let (lo, hi) = shape.cells();
                self.each(lo, hi, |v, cell| merged(cell, m, shape.coverage(v)));
            }
        }
        let dirty = self.grid.take_dirty();
        self.pending.extend(dirty);
        self.version += 1;
        Ok((TerrainReply::Done, true))
    }

    fn each(
        &mut self,
        min: [i32; 3],
        max: [i32; 3],
        mut f: impl FnMut([i32; 3], Cell) -> Option<Cell>,
    ) {
        for y in min[1]..max[1] {
            for z in min[2]..max[2] {
                for x in min[0]..max[0] {
                    let v = [x, y, z];
                    if let Some(cell) = f(v, self.grid.get(v)) {
                        self.grid.set(v, cell);
                    }
                }
            }
        }
    }

    /// Writes the voxels and colours back to the DOM.
    pub(crate) fn write_back(&mut self, dom: &mut WeakDom) {
        let Some(referent) = self.referent else {
            return;
        };
        self.bytes = self.encoder.encode(&self.grid);
        let _ = dom.set_property(referent, "SmoothGrid", binary(self.bytes.clone()));
        let _ = dom.set_property(referent, "MaterialColors", binary(self.colors.encode()));
    }

    /// The nearest terrain hit along `direction` (its whole length), against
    /// the collision mesh and, unless `ignore_water`, the water's surface.
    pub(crate) fn raycast(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        ignore_water: bool,
    ) -> Option<QueryHit> {
        let referent = self.referent?;
        let (from, dir) = (Vector::from_array(origin), Vector::from_array(direction));
        let length = dir.length();
        if length <= 0.0 {
            return None;
        }
        let ray = Ray::new(from, dir);
        let mut best: Option<(Real, Vector, Material)> = None;
        for (_, shapes) in self.shapes.values() {
            for (material, shape) in shapes {
                // Only surfaces it goes into: a ray from inside terrain
                // finds nothing there (TerrainRaycast `up_from_inside`).
                let hit = cull(shape, &ray, 1.0, RayCullingMode::IgnoreBackfaces);
                if let Some(hit) = hit.filter(|h| best.is_none_or(|b| h.time_of_impact < b.0)) {
                    best = Some((hit.time_of_impact, hit.normal, *material));
                }
            }
        }
        if !ignore_water && self.has_water() {
            let limit = best.map_or(1.0, |b| b.0);
            if let Some(t) = self.water_entry(from, dir, limit) {
                best = Some((t, self.water_normal(from + dir * t), Material::Water));
            }
        }
        let (t, mut normal, material) = best?;
        if normal.dot(dir) > 0.0 {
            normal = -normal;
        }
        Some(QueryHit {
            part: referent,
            position: (from + dir * t).to_array(),
            normal: normal.to_array(),
            distance: t * length,
            material: Some(material.name()),
        })
    }

    /// How far below the collision surface `p` is, if it is inside terrain:
    /// straight up, the first surface met is one going out.
    pub(crate) fn depth(&self, p: Vector) -> Option<Real> {
        let ray = Ray::new(p, Vector::Y);
        let (mut out, mut into) = (Real::MAX, Real::MAX);
        for (_, shapes) in self.shapes.values() {
            for (_, shape) in shapes {
                let aabb = shape.compute_local_aabb();
                if p.x < aabb.mins.x
                    || p.x > aabb.maxs.x
                    || p.z < aabb.mins.z
                    || p.z > aabb.maxs.z
                    || p.y > aabb.maxs.y
                {
                    continue;
                }
                if let Some(hit) = cull(shape, &ray, 1.0e4, RayCullingMode::IgnoreFrontfaces) {
                    out = out.min(hit.time_of_impact);
                }
                if let Some(hit) = cull(shape, &ray, 1.0e4, RayCullingMode::IgnoreBackfaces) {
                    into = into.min(hit.time_of_impact);
                }
            }
        }
        (out < into).then_some(out)
    }

    /// `Blockcast`/`Spherecast` against the collision mesh (a part's own
    /// shape is not cast against terrain yet).
    pub(crate) fn shapecast(
        &self,
        shape: &rbx_lua::server::QueryShape,
        from: &Pose,
        direction: [f64; 3],
    ) -> Option<QueryHit> {
        use rbx_lua::server::QueryShape;
        let referent = self.referent?;
        let moving = match shape {
            QueryShape::Box(size) => {
                SharedShape::cuboid(size[0] / 2.0, size[1] / 2.0, size[2] / 2.0)
            }
            QueryShape::Ball(radius) => SharedShape::ball(*radius),
            QueryShape::Part(_) => return None,
        };
        let dir = Vector::from_array(direction);
        let options = rapier3d::parry::query::ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let mut best: Option<(Real, Vector, Vector, Material)> = None;
        for (_, shapes) in self.shapes.values() {
            for (material, target) in shapes {
                let hit = rapier3d::parry::query::cast_shapes(
                    from,
                    dir,
                    &*moving,
                    &Pose::IDENTITY,
                    Vector::ZERO,
                    &**target,
                    options,
                );
                if let Ok(Some(hit)) = hit {
                    if hit.time_of_impact > 0.0 && best.is_none_or(|b| hit.time_of_impact < b.0) {
                        best = Some((hit.time_of_impact, hit.witness2, hit.normal2, *material));
                    }
                }
            }
        }
        let (t, position, normal, material) = best?;
        Some(QueryHit {
            part: referent,
            position: position.to_array(),
            normal: normal.to_array(),
            distance: t * dir.length(),
            material: Some(material.name()),
        })
    }

    /// Where along `dir` (as a fraction of it, below `limit`) the ray first
    /// enters water from outside it.
    fn water_entry(&self, from: Vector, dir: Vector, limit: Real) -> Option<Real> {
        let step = 0.5 / dir.length();
        let wet = |t: Real| self.water_at(from + dir * t) >= 0.5;
        if wet(0.0) {
            return None;
        }
        let mut t = 0.0;
        while t < limit {
            let next = (t + step).min(limit);
            if wet(next) {
                let (mut lo, mut hi) = (t, next);
                for _ in 0..20 {
                    let mid = (lo + hi) / 2.0;
                    if wet(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(hi);
            }
            t = next;
        }
        None
    }

    fn water_normal(&self, p: Vector) -> Vector {
        let e = 0.25;
        let g = Vector::new(
            self.water_at(p - Vector::X * e) - self.water_at(p + Vector::X * e),
            self.water_at(p - Vector::Y * e) - self.water_at(p + Vector::Y * e),
            self.water_at(p - Vector::Z * e) - self.water_at(p + Vector::Z * e),
        );
        g.try_normalize().unwrap_or(Vector::Y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(terrain: &mut Terrain, y: f32, height: f64, material: &str) {
        let cframe = crate::humanoid::translated(crate::humanoid::identity(), [0.0, y, 0.0]);
        terrain
            .apply(TerrainOp::FillBlock {
                cframe,
                size: [32.0, height, 32.0],
                material: material.into(),
            })
            .unwrap();
    }

    fn lake() -> Terrain {
        let mut terrain = Terrain {
            referent: Some(Ref::new(1)),
            colors: engine_colors(),
            ..Terrain::default()
        };
        // Water down to -16, rock below it, surfaces at 0 and -16.
        block(&mut terrain, -8.0, 16.0, "Water");
        block(&mut terrain, -24.0, 16.0, "Rock");
        terrain.flush();
        terrain
    }

    #[test]
    fn fill_coverage_is_measured_per_cell() {
        assert_eq!(coverage(-2.0), 1.0);
        assert_eq!(coverage(2.0), 0.0);
        assert_eq!(coverage(0.0), 0.5);
        // A quarter of solid leaves a full water cell water; a full cell
        // takes it over.
        let water = Cell::full(Material::Water);
        assert!(merged(water, Material::Rock, 0.25).is_none());
        assert_eq!(
            merged(water, Material::Rock, 1.0).map(|c| c.material),
            Some(Material::Rock)
        );
    }

    #[test]
    fn water_surface_and_rays() {
        let terrain = lake();
        assert!(terrain.has_water());
        assert!((terrain.water_at(Vector::new(0.0, 0.0, 0.0)) - 0.5).abs() < 0.05);
        assert!(terrain.water_at(Vector::new(0.0, -8.0, 0.0)) > 0.9);
        assert_eq!(terrain.water_at(Vector::new(0.0, 8.0, 0.0)), 0.0);
        let down = [0.0, -40.0, 0.0];
        let wet = terrain.raycast([0.5, 20.0, 0.5], down, false).unwrap();
        assert_eq!(wet.material, Some("Water"));
        assert!(wet.position[1].abs() < 0.1, "{wet:?}");
        // The rock under the water sits 2 above its fill, like in air.
        let dry = terrain.raycast([0.5, 20.0, 0.5], down, true).unwrap();
        assert_eq!(dry.material, Some("Rock"));
        assert!((dry.position[1] + 14.0).abs() < 0.1, "{dry:?}");
        // Nothing from inside the rock.
        assert!(terrain
            .raycast([0.5, -24.0, 0.5], [0.0, 40.0, 0.0], true)
            .is_none());
        assert!(terrain.depth(Vector::new(0.5, -20.0, 0.5)).is_some());
    }

    #[test]
    fn air_and_water_have_no_colour() {
        let mut terrain = lake();
        assert!(terrain.apply(TerrainOp::GetColor("Water".into())).is_err());
        let (reply, _) = terrain.apply(TerrainOp::GetColor("Grass".into())).unwrap();
        let TerrainReply::Color(c) = reply else {
            panic!("{reply:?}")
        };
        assert_eq!(c.map(|v| (v * 255.0).round() as u8), [106, 127, 63]);
    }
}
