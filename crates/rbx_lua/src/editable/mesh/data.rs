//! An `EditableMesh`'s contents and the edits that keep them consistent:
//! faces of three or four corners, each corner naming its own normal, UV
//! and colour, the per-vertex face lists the queries walk, the limits.
//!
//! What a new face's corners get (the EditableMeshTopology golden): a
//! vertex already on a face lends that face's attributes, a vertex on none
//! gets fresh ones, created in corner order. A vertex alone brings nothing.

use std::collections::{BTreeMap, HashMap};

use mlua::Result;
use rbx_dom::CFrameData;

use super::geometry::{angle_weight, cross, dot, normalized, sub, V3};
use super::ids::{Id, Kind, Pool};

pub(crate) const MAX_VERTICES: usize = 60_000;
pub(crate) const MAX_TRIANGLES: usize = 20_000;

#[derive(Clone, Default)]
pub(crate) struct Vertex {
    pub(crate) position: V3,
    pub(crate) bones: Vec<Id>,
    /// Skin weights as bytes, as Roblox keeps them.
    pub(crate) weights: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct Face {
    pub(crate) vertices: Vec<Id>,
    pub(crate) normals: Vec<Id>,
    pub(crate) uvs: Vec<Id>,
    pub(crate) colors: Vec<Id>,
}

impl Face {
    pub(crate) fn slot(&self, kind: Kind) -> &Vec<Id> {
        match kind {
            Kind::Normal => &self.normals,
            Kind::Uv => &self.uvs,
            Kind::Color => &self.colors,
            _ => &self.vertices,
        }
    }

    pub(crate) fn slot_mut(&mut self, kind: Kind) -> &mut Vec<Id> {
        match kind {
            Kind::Normal => &mut self.normals,
            Kind::Uv => &mut self.uvs,
            Kind::Color => &mut self.colors,
            _ => &mut self.vertices,
        }
    }

    fn triangles(&self) -> usize {
        self.vertices.len() - 2
    }
}

#[derive(Clone)]
pub(crate) struct Bone {
    pub(crate) name: String,
    /// 0 for a root bone.
    pub(crate) parent: Id,
    pub(crate) cframe: CFrameData,
    pub(crate) is_virtual: bool,
}

/// One FACS pose: bone → its transform.
pub(crate) type Pose = Vec<(Id, CFrameData)>;

#[derive(Clone)]
pub(crate) struct MeshData {
    pub(crate) vertices: Pool<Vertex>,
    /// `None`: worked out from the faces using it.
    pub(crate) normals: Pool<Option<V3>>,
    pub(crate) uvs: Pool<[f32; 2]>,
    /// RGB then alpha, the alpha kept to a byte as Roblox does.
    pub(crate) colors: Pool<[f32; 4]>,
    pub(crate) faces: Pool<Face>,
    pub(crate) bones: Pool<Bone>,
    /// The faces on each vertex, in the order they reached it.
    pub(crate) vertex_faces: HashMap<Id, Vec<Id>>,
    /// Base poses by `FacsActionUnit` value.
    pub(crate) poses: BTreeMap<u32, Pose>,
    /// Corrective poses by their sorted action values.
    pub(crate) correctives: BTreeMap<Vec<u32>, Pose>,
    pub(crate) fixed: bool,
    /// Made in Edit, where Studio lets a fixed-size mesh grow and shrink
    /// anyway (its error says "in-experience"; measured in the command bar).
    pub(crate) in_edit: bool,
    pub(crate) version: u64,
}

impl Default for MeshData {
    fn default() -> Self {
        MeshData {
            vertices: Pool::new(Kind::Vertex),
            normals: Pool::new(Kind::Normal),
            uvs: Pool::new(Kind::Uv),
            colors: Pool::new(Kind::Color),
            faces: Pool::new(Kind::Face),
            bones: Pool::new(Kind::Bone),
            vertex_faces: HashMap::new(),
            poses: BTreeMap::new(),
            correctives: BTreeMap::new(),
            fixed: false,
            in_edit: false,
            version: 0,
        }
    }
}

impl MeshData {
    /// Whether adding or removing elements is refused.
    pub(crate) fn frozen(&self) -> bool {
        self.fixed && !self.in_edit
    }
}

pub(crate) fn fixed_error() -> mlua::Error {
    mlua::Error::runtime(
        "EditableMesh is fixed-size, adding or removing elements is not allowed in-experience",
    )
}

/// Which diagonal a quad splits along: 0 for corners 0-2, 1 for 1-3.
/// Roblox avoids a split that folds the quad, then takes the one whose
/// smallest angle is larger, 0-2 on a tie (the EditableMeshTopology
/// `split_*` rows: a square stays 0-2, a skewed quad, a kite or a dart
/// turns when 1-3 is the better split).
pub(crate) fn diagonal(p: [V3; 4]) -> usize {
    let tris = |d: usize| -> [[V3; 3]; 2] {
        match d {
            0 => [[p[0], p[1], p[2]], [p[2], p[3], p[0]]],
            _ => [[p[1], p[2], p[3]], [p[3], p[0], p[1]]],
        }
    };
    let folds = |t: [[V3; 3]; 2]| {
        let n = t.map(|[a, b, c]| cross(sub(b, a), sub(c, a)));
        dot(n[0], n[1]) < 0.0
    };
    let smallest = |t: [[V3; 3]; 2]| {
        t.iter()
            .flat_map(|&[a, b, c]| [angle(a, b, c), angle(b, c, a), angle(c, a, b)])
            .fold(f32::INFINITY, f32::min)
    };
    let (a, b) = (tris(0), tris(1));
    match (folds(a), folds(b)) {
        (true, false) => 1,
        (false, true) => 0,
        _ if smallest(b) > smallest(a) + 1e-4 => 1,
        _ => 0,
    }
}

/// The angle at `a` in triangle `abc`.
fn angle(a: V3, b: V3, c: V3) -> f32 {
    let (u, v) = (normalized(sub(b, a)), normalized(sub(c, a)));
    dot(u, v).clamp(-1.0, 1.0).acos()
}

/// A face's corners as triangles, by corner index.
pub(crate) fn corner_triangles(face: &Face, positions: &[V3]) -> Vec<[usize; 3]> {
    match face.vertices.len() {
        4 => match diagonal([positions[0], positions[1], positions[2], positions[3]]) {
            0 => vec![[0, 1, 2], [2, 3, 0]],
            _ => vec![[1, 2, 3], [3, 0, 1]],
        },
        _ => vec![[0, 1, 2]],
    }
}

impl MeshData {
    pub(crate) fn position(&self, vertex: Id) -> V3 {
        self.vertices.get(vertex).map_or([0.0; 3], |v| v.position)
    }

    pub(crate) fn corners(&self, face: &Face) -> Vec<V3> {
        face.vertices.iter().map(|&v| self.position(v)).collect()
    }

    pub(crate) fn triangle_count(&self) -> usize {
        self.faces.iter().map(|(_, f)| f.triangles()).sum()
    }

    /// A face's normal: Newell's, which a quad that isn't flat needs.
    pub(crate) fn face_normal(&self, face: &Face) -> V3 {
        let p = self.corners(face);
        let mut n = [0.0; 3];
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        normalized(n)
    }

    /// A normal's value: a set one as set, a computed one from the faces
    /// using it, each weighed by the square of the sine of its angle at
    /// the corner (what the EditableMeshQuery `weight_*` rows fit: equal
    /// for right angles whatever the area, 2:1 for 90° against 135°).
    pub(crate) fn normal(&self, id: Id) -> Option<V3> {
        match self.normals.get(id)? {
            Some(set) => Some(*set),
            None => Some(self.computed_normal(id)),
        }
    }

    fn computed_normal(&self, id: Id) -> V3 {
        self.computed_normals(Some(id))
            .remove(&id)
            .unwrap_or([0.0; 3])
    }

    /// Every computed normal (or just `only`) in one pass over the faces.
    pub(crate) fn computed_normals(&self, only: Option<Id>) -> HashMap<Id, V3> {
        let mut sums: HashMap<Id, V3> = HashMap::new();
        for (_, face) in self.faces.iter() {
            if only.is_some_and(|id| !face.normals.contains(&id)) {
                continue;
            }
            let p = self.corners(face);
            let count = p.len();
            let n = self.face_normal(face);
            for corner in 0..count {
                let id = face.normals[corner];
                if only.is_some_and(|o| o != id) || matches!(self.normals.get(id), Some(Some(_))) {
                    continue;
                }
                let w = angle_weight(
                    p[corner],
                    p[(corner + count - 1) % count],
                    p[(corner + 1) % count],
                );
                let sum = sums.entry(id).or_insert([0.0; 3]);
                *sum = std::array::from_fn(|i| sum[i] + n[i] * w);
            }
        }
        sums.into_iter()
            .map(|(id, sum)| (id, normalized(sum)))
            .collect()
    }

    pub(crate) fn bounds(&self) -> (V3, V3) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for (_, v) in self.vertices.iter() {
            for axis in 0..3 {
                min[axis] = min[axis].min(v.position[axis]);
                max[axis] = max[axis].max(v.position[axis]);
            }
        }
        (min, max)
    }

    pub(crate) fn add_vertex(&mut self, position: V3) -> Result<Id> {
        if self.vertices.len() >= MAX_VERTICES {
            return Err(mlua::Error::runtime("Vertex count above limit"));
        }
        Ok(self.vertices.insert(Vertex {
            position,
            ..Vertex::default()
        }))
    }

    /// The attributes a corner on `vertex` starts with: those of the first
    /// face already on it, else fresh defaults.
    fn lend(&mut self, vertex: Id) -> [Id; 3] {
        let lent = self
            .vertex_faces
            .get(&vertex)
            .and_then(|faces| faces.first())
            .and_then(|&f| self.faces.get(f))
            .and_then(|face| {
                let i = face.vertices.iter().position(|&v| v == vertex)?;
                Some([face.normals[i], face.uvs[i], face.colors[i]])
            });
        lent.unwrap_or_else(|| {
            [
                self.normals.insert(None),
                self.uvs.insert([0.0, 0.0]),
                self.colors.insert([1.0; 4]),
            ]
        })
    }

    fn check_corners(count: usize, method: &str) -> Result<()> {
        match count {
            3 | 4 => Ok(()),
            n => Err(mlua::Error::runtime(format!(
                "{method} called with {n} vertices, currently only supports 3 or 4 vertices"
            ))),
        }
    }

    /// The live vertices `ids` name by slot, as `AddFace` reads them: any
    /// id kind passes, only the slot has to hold a vertex.
    pub(crate) fn face_vertices(&self, ids: &[f64]) -> Result<Vec<Id>> {
        ids.iter()
            .map(|&id| {
                let slot = (id >= 0.0 && id.fract() == 0.0).then_some(id as u64 as u32);
                slot.and_then(|s| self.vertices.by_slot(s)).ok_or_else(|| {
                    mlua::Error::runtime(format!(
                        "Unable to add new triangle, vertex id {id} is invalid"
                    ))
                })
            })
            .collect()
    }

    fn room_for(&self, triangles: usize) -> Result<()> {
        match self.triangle_count() + triangles > MAX_TRIANGLES {
            true => Err(mlua::Error::runtime("Triangle count above limit")),
            false => Ok(()),
        }
    }

    /// A face under `id` (from `self.faces.reserve`), corners as given.
    pub(crate) fn place_face(&mut self, id: Id, vertices: Vec<Id>) {
        let mut face = Face {
            normals: Vec::with_capacity(vertices.len()),
            uvs: Vec::with_capacity(vertices.len()),
            colors: Vec::with_capacity(vertices.len()),
            vertices,
        };
        for &v in &face.vertices {
            let [n, u, c] = self.lend(v);
            face.normals.push(n);
            face.uvs.push(u);
            face.colors.push(c);
        }
        self.link(id, &face.vertices, false);
        self.faces.place(id, face);
    }

    fn link(&mut self, face: Id, vertices: &[Id], front: bool) {
        for &v in vertices {
            let list = self.vertex_faces.entry(v).or_default();
            if !list.contains(&face) {
                match front {
                    true => list.insert(0, face),
                    false => list.push(face),
                }
            }
        }
    }

    fn unlink(&mut self, face: Id, vertices: &[Id]) {
        for v in vertices {
            if let Some(list) = self.vertex_faces.get_mut(v) {
                list.retain(|&f| f != face);
            }
        }
    }

    pub(crate) fn add_face(&mut self, ids: &[f64], method: &str) -> Result<Id> {
        Self::check_corners(ids.len(), method)?;
        let vertices = self.face_vertices(ids)?;
        if self.frozen() {
            return Err(fixed_error());
        }
        self.room_for(vertices.len() - 2)?;
        let id = self.faces.reserve();
        self.place_face(id, vertices);
        Ok(id)
    }

    /// New corners keep the attributes at their index; a corner past the
    /// old count is lent as a new face's would be. The slot dance mirrors
    /// Roblox's: a successful change takes and gives back one face slot,
    /// which shows in the version of the next face to reuse it.
    pub(crate) fn set_face_vertices(&mut self, face: Id, vertices: Vec<Id>) -> Result<()> {
        let old = self.faces.get(face).expect("checked").clone();
        if vertices.len() > old.vertices.len() {
            self.room_for(vertices.len() - old.vertices.len())?;
        }
        let mut new = Face {
            vertices: vertices.clone(),
            normals: Vec::new(),
            uvs: Vec::new(),
            colors: Vec::new(),
        };
        for (i, &v) in vertices.iter().enumerate() {
            let [n, u, c] = match i < old.vertices.len() {
                true => [old.normals[i], old.uvs[i], old.colors[i]],
                false => self.lend(v),
            };
            new.normals.push(n);
            new.uvs.push(u);
            new.colors.push(c);
        }
        let scratch = self.faces.reserve();
        self.faces.release(scratch);
        self.unlink(face, &old.vertices);
        self.link(face, &vertices, true);
        *self.faces.get_mut(face).expect("checked") = new;
        Ok(())
    }

    /// A face with its corners' attributes given, as a mesh file has them.
    pub(crate) fn put_face(&mut self, face: Face) -> Id {
        let id = self.faces.reserve();
        self.link(id, &face.vertices, false);
        self.faces.place(id, face);
        id
    }

    pub(crate) fn remove_face(&mut self, face: Id) -> Option<Face> {
        let removed = self.faces.remove(face)?;
        self.unlink(face, &removed.vertices);
        Some(removed)
    }

    /// Every vertex, normal, UV and colour no face uses, removed; their ids.
    pub(crate) fn remove_unused(&mut self) -> Vec<Id> {
        let mut used: HashMap<Kind, std::collections::HashSet<Id>> = HashMap::new();
        for (_, face) in self.faces.iter() {
            for kind in [Kind::Vertex, Kind::Normal, Kind::Uv, Kind::Color] {
                used.entry(kind).or_default().extend(face.slot(kind));
            }
        }
        let unused = |kind: Kind, ids: Vec<Id>| -> Vec<Id> {
            let used = used.get(&kind);
            ids.into_iter()
                .filter(|id| !used.is_some_and(|u| u.contains(id)))
                .collect()
        };
        let gone_vertices = unused(Kind::Vertex, self.vertices.ids());
        let gone_normals = unused(Kind::Normal, self.normals.ids());
        let gone_uvs = unused(Kind::Uv, self.uvs.ids());
        let gone_colors = unused(Kind::Color, self.colors.ids());
        for &id in &gone_vertices {
            self.vertices.remove(id);
            self.vertex_faces.remove(&id);
        }
        for &id in &gone_normals {
            self.normals.remove(id);
        }
        for &id in &gone_uvs {
            self.uvs.remove(id);
        }
        for &id in &gone_colors {
            self.colors.remove(id);
        }
        [gone_vertices, gone_normals, gone_uvs, gone_colors].concat()
    }

    /// Faces get ids as Roblox gives a batch of them: taken in a run, then
    /// handed out last first (the `split_*` and `add_faces` rows).
    pub(crate) fn reserve_faces(&mut self, count: usize) -> Vec<Id> {
        let mut ids: Vec<Id> = (0..count).map(|_| self.faces.reserve()).collect();
        ids.reverse();
        ids
    }

    /// Every quad split in two along its [`diagonal`]; the triangles keep
    /// the quad's corner attributes.
    pub(crate) fn triangulate(&mut self) -> Result<()> {
        let quads: Vec<Id> = self
            .faces
            .iter()
            .filter(|(_, f)| f.vertices.len() == 4)
            .map(|(id, _)| id)
            .collect();
        if quads.is_empty() {
            return Ok(());
        }
        if self.frozen() {
            return Err(fixed_error());
        }
        let removed: Vec<Face> = quads.iter().filter_map(|&q| self.remove_face(q)).collect();
        let ids = self.reserve_faces(removed.len() * 2);
        for (quad, pair) in removed.iter().zip(ids.chunks(2)) {
            let positions = self.corners(quad);
            for (corners, &id) in corner_triangles(quad, &positions).iter().zip(pair) {
                let pick = |list: &Vec<Id>| corners.iter().map(|&c| list[c]).collect();
                let face = Face {
                    vertices: pick(&quad.vertices),
                    normals: pick(&quad.normals),
                    uvs: pick(&quad.uvs),
                    colors: pick(&quad.colors),
                };
                self.link(id, &face.vertices, false);
                self.faces.place(id, face);
            }
        }
        Ok(())
    }

    /// Vertices on faces within `tolerance` of an earlier one are replaced
    /// by it on every face; the replaced ones stay, unused. Roblox hands
    /// back an empty map from this (the `merge` row), whatever it merged.
    pub(crate) fn merge_vertices(&mut self, tolerance: f32) {
        let on_faces: Vec<Id> = self
            .vertices
            .ids()
            .into_iter()
            .filter(|v| self.vertex_faces.get(v).is_some_and(|f| !f.is_empty()))
            .collect();
        let mut into: HashMap<Id, Id> = HashMap::new();
        for (i, &v) in on_faces.iter().enumerate() {
            if into.contains_key(&v) {
                continue;
            }
            let p = self.position(v);
            for &w in &on_faces[i + 1..] {
                let d = sub(self.position(w), p);
                if !into.contains_key(&w) && dot(d, d).sqrt() <= tolerance {
                    into.insert(w, v);
                }
            }
        }
        if into.is_empty() {
            return;
        }
        let faces: Vec<Id> = self.faces.ids();
        for f in faces {
            let old = self.faces.get(f).expect("listed").vertices.clone();
            if !old.iter().any(|v| into.contains_key(v)) {
                continue;
            }
            let new: Vec<Id> = old.iter().map(|v| *into.get(v).unwrap_or(v)).collect();
            self.unlink(f, &old);
            self.link(f, &new, false);
            self.faces.get_mut(f).expect("listed").vertices = new;
        }
    }

    /// Bone, skin and pose data dropped with a removed bone.
    pub(crate) fn forget_bone(&mut self, bone: Id) {
        for vertex in self.vertices.values_mut() {
            if let Some(i) = vertex.bones.iter().position(|&b| b == bone) {
                vertex.bones.remove(i);
                if i < vertex.weights.len() {
                    vertex.weights.remove(i);
                }
            }
        }
        for b in self.bones.values_mut() {
            if b.parent == bone {
                b.parent = 0;
            }
        }
        for pose in self.poses.values_mut().chain(self.correctives.values_mut()) {
            pose.retain(|(b, _)| *b != bone);
        }
        self.poses.retain(|_, pose| !pose.is_empty());
        self.correctives.retain(|_, pose| !pose.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::super::ids::debug_string;
    use super::*;

    fn names(ids: &[Id]) -> String {
        ids.iter()
            .map(|&id| debug_string(id as f64))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn id(v: Id) -> f64 {
        v as f64
    }

    #[test]
    fn faces_lend_attributes_of_the_faces_a_vertex_is_already_on() {
        let mut m = MeshData::default();
        let v: Vec<Id> = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ]
        .into_iter()
        .map(|p| m.add_vertex(p).unwrap())
        .collect();
        assert_eq!(m.normals.len(), 0);
        let quad = m
            .add_face(&[id(v[0]), id(v[3]), id(v[2]), id(v[1])], "AddFace")
            .unwrap();
        assert_eq!(names(&m.faces.get(quad).unwrap().normals), "n1 n2 n3 n4");
        let v5 = m.add_vertex([2.0, 0.0, 0.0]).unwrap();
        let tri = m
            .add_face(&[id(v[1]), id(v[2]), id(v5)], "AddTriangle")
            .unwrap();
        assert_eq!(names(&m.faces.get(tri).unwrap().normals), "n4 n3 n5");
    }

    #[test]
    fn triangulate_hands_out_ids_like_roblox() {
        // The golden's split_two: f4 f3 f1,2 f2,2.
        let mut m = MeshData::default();
        let p = [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 0.0, 1.0],
        ];
        let v: Vec<f64> = p.iter().map(|&p| id(m.add_vertex(p).unwrap())).collect();
        m.add_face(&[v[0], v[1], v[2], v[3]], "AddFace").unwrap();
        m.add_face(&[v[3], v[2], v[5], v[4]], "AddFace").unwrap();
        m.triangulate().unwrap();
        assert_eq!(names(&m.faces.ids()), "f4 f3 f1,2 f2,2");
        let first = m.faces.get(m.faces.ids()[0]).unwrap();
        assert_eq!(names(&first.vertices), "v1 v2 v3");
    }

    #[test]
    fn quads_split_along_the_diagonal_with_the_better_triangles() {
        let kite = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [4.0, 0.0, 0.0],
            [1.0, 0.0, -1.0],
        ];
        assert_eq!(diagonal(kite), 1);
        let skew = [
            [2.0, 0.0, 0.0],
            [2.0, 0.0, 1.0],
            [3.0, 0.5, 1.0],
            [3.0, 0.0, 0.0],
        ];
        assert_eq!(diagonal(skew), 1);
        let square = [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ];
        assert_eq!(diagonal(square), 0);
        let dart = [
            [-1.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, -2.0],
        ];
        assert_eq!(diagonal(dart), 1);
    }

    #[test]
    fn computed_normals_weigh_faces_by_their_corner_angle() {
        // weight_d: a flat right angle and a tilted 135° corner.
        let mut m = MeshData::default();
        let o = id(m.add_vertex([0.0, 0.0, 0.0]).unwrap());
        let x = id(m.add_vertex([1.0, 0.0, 0.0]).unwrap());
        let z = id(m.add_vertex([0.0, 0.0, -1.0]).unwrap());
        let t = id(m.add_vertex([-1.0, 1.0, 0.0]).unwrap());
        let flat = m.add_face(&[o, x, z], "AddTriangle").unwrap();
        m.add_face(&[o, t, x], "AddTriangle").unwrap();
        let n = m.normal(m.faces.get(flat).unwrap().normals[0]).unwrap();
        assert!(
            (n[1] - 0.8944).abs() < 1e-3 && (n[2] + 0.4472).abs() < 1e-3,
            "{n:?}"
        );
    }
}
