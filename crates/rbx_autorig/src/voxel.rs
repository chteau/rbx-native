//! A solid voxel model of the mesh: what landmark detection measures and
//! what geodesic distances run through.
//!
//! The surface is rasterised, the outside is flood-filled from the border,
//! and everything else is solid. Working on a volume rather than on the
//! triangles makes the rest robust to what real files do: several
//! intersecting meshes, unwelded seams, slivers and pinholes smaller than a
//! voxel. Holes are deliberately *not* closed morphologically: closing glues
//! limbs that nearly touch (legs at the crotch, an I-pose arm at the ribs),
//! which is exactly what the landmarks need kept apart.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use glam::Vec3;

/// Padding voxels on every side; at least one keeps neighbour lookups of
/// solid voxels in range without bounds checks.
const PAD: usize = 2;

pub(crate) struct Grid {
    /// Corner of voxel (0, 0, 0).
    pub origin: Vec3,
    pub cell: f32,
    pub dims: [usize; 3],
    pub solid: Vec<bool>,
}

/// The 26 neighbour offsets with their lengths in voxels.
fn neighbours(dims: [usize; 3]) -> Vec<(isize, f32)> {
    let (nx, nxy) = (dims[0] as isize, (dims[0] * dims[1]) as isize);
    let mut out = Vec::with_capacity(26);
    for dz in -1isize..=1 {
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }
                let len = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
                out.push((dx + dy * nx + dz * nxy, len));
            }
        }
    }
    out
}

impl Grid {
    /// Voxelises `triangles` with `resolution` cells (16 to 512) along the
    /// longest axis of their bounds.
    pub fn build(positions: &[Vec3], triangles: &[[u32; 3]], resolution: usize) -> Grid {
        // Bounds of what the triangles use: stray unreferenced vertices
        // must not stretch the grid.
        let (lo, hi) =
            crate::mesh::bounds(triangles.iter().flatten().map(|&i| positions[i as usize]));
        let extent = (hi - lo).max(Vec3::splat(1e-6));
        // 512³ voxels is already over 100 MB of working state.
        let cell = extent.max_element() / resolution.clamp(16, 512) as f32;
        let dims = [0, 1, 2].map(|k| (extent[k] / cell).ceil() as usize + 1 + 2 * PAD);
        let origin = lo - Vec3::splat(PAD as f32 * cell);
        let mut grid = Grid {
            origin,
            cell,
            dims,
            solid: vec![false; dims[0] * dims[1] * dims[2]],
        };

        // Surface: sample each triangle at half-voxel spacing. Consecutive
        // samples then never skip a voxel, so the marked shell is
        // 26-connected and a 6-connected flood cannot leak through it.
        let mut surface = vec![false; grid.solid.len()];
        let step = 0.5 * cell;
        for t in triangles {
            let [a, b, c] = t.map(|i| positions[i as usize]);
            let longest = (b - a).length().max((c - a).length()).max((c - b).length());
            let n = ((longest / step).ceil() as usize).max(1);
            for i in 0..=n {
                for j in 0..=n - i {
                    let p = a + (b - a) * (i as f32 / n as f32) + (c - a) * (j as f32 / n as f32);
                    if let Some(v) = grid.voxel(p) {
                        surface[v] = true;
                    }
                }
            }
        }

        // Exterior: 6-connected flood from the (empty) border.
        let mut outside = vec![false; grid.solid.len()];
        let mut stack = Vec::new();
        let [nx, ny, nz] = dims;
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    if x == 0 || y == 0 || z == 0 || x == nx - 1 || y == ny - 1 || z == nz - 1 {
                        let i = grid.index(x, y, z);
                        outside[i] = true;
                        stack.push(i);
                    }
                }
            }
        }
        let steps = [
            1isize,
            -1,
            nx as isize,
            -(nx as isize),
            (nx * ny) as isize,
            -((nx * ny) as isize),
        ];
        while let Some(i) = stack.pop() {
            for s in steps {
                let n = i as isize + s;
                if n < 0 || n as usize >= outside.len() {
                    continue;
                }
                let n = n as usize;
                if !outside[n] && !surface[n] {
                    outside[n] = true;
                    stack.push(n);
                }
            }
        }
        for (s, o) in grid.solid.iter_mut().zip(&outside) {
            *s = !o;
        }
        grid
    }

    pub fn index(&self, x: usize, y: usize, z: usize) -> usize {
        x + self.dims[0] * (y + self.dims[1] * z)
    }

    pub fn coords(&self, i: usize) -> [usize; 3] {
        let x = i % self.dims[0];
        let y = (i / self.dims[0]) % self.dims[1];
        let z = i / (self.dims[0] * self.dims[1]);
        [x, y, z]
    }

    pub fn centre(&self, i: usize) -> Vec3 {
        let [x, y, z] = self.coords(i);
        self.origin + (Vec3::new(x as f32, y as f32, z as f32) + 0.5) * self.cell
    }

    /// The voxel holding `p`, if inside the grid.
    pub fn voxel(&self, p: Vec3) -> Option<usize> {
        let q = (p - self.origin) / self.cell;
        if q.x < 0. || q.y < 0. || q.z < 0. {
            return None;
        }
        let (x, y, z) = (q.x as usize, q.y as usize, q.z as usize);
        (x < self.dims[0] && y < self.dims[1] && z < self.dims[2]).then(|| self.index(x, y, z))
    }

    pub fn solid_at(&self, p: Vec3) -> bool {
        self.voxel(p).is_some_and(|v| self.solid[v])
    }

    /// Exact Euclidean distance (in voxels) from each solid voxel to the
    /// nearest empty one: Felzenszwalb & Huttenlocher's separable lower
    /// envelope of parabolas, one pass per axis.
    pub fn distance(&self) -> Vec<f32> {
        const FAR: f64 = 1e12;
        let mut f: Vec<f64> = self
            .solid
            .iter()
            .map(|&s| if s { FAR } else { 0. })
            .collect();
        let [nx, ny, nz] = self.dims;
        let n = nx.max(ny).max(nz);
        let (mut line, mut out) = (vec![0.; n], vec![0.; n]);
        let (mut v, mut z) = (vec![0usize; n], vec![0f64; n + 1]);
        let strides = [1, nx, nx * ny];
        for axis in 0..3 {
            let len = self.dims[axis];
            let stride = strides[axis];
            let (o1, o2) = ((axis + 1) % 3, (axis + 2) % 3);
            for a in 0..self.dims[o1] {
                for b in 0..self.dims[o2] {
                    let base = a * strides[o1] + b * strides[o2];
                    for (i, l) in line.iter_mut().enumerate().take(len) {
                        *l = f[base + i * stride];
                    }
                    envelope(&line[..len], &mut out[..len], &mut v, &mut z);
                    for (i, o) in out.iter().enumerate().take(len) {
                        f[base + i * stride] = *o;
                    }
                }
            }
        }
        f.into_iter().map(|d| (d as f32).sqrt()).collect()
    }

    /// Labels of the 26-connected solid pieces (`u32::MAX` outside) and
    /// each piece's voxel count.
    fn pieces(&self) -> (Vec<u32>, Vec<usize>) {
        let offsets = neighbours(self.dims);
        let mut label = vec![u32::MAX; self.solid.len()];
        let mut sizes = Vec::new();
        let mut stack = Vec::new();
        for seed in 0..self.solid.len() {
            if !self.solid[seed] || label[seed] != u32::MAX {
                continue;
            }
            let id = sizes.len() as u32;
            let mut size = 0;
            label[seed] = id;
            stack.push(seed);
            while let Some(v) = stack.pop() {
                size += 1;
                for &(off, _) in &offsets {
                    let w = (v as isize + off) as usize;
                    if self.solid[w] && label[w] == u32::MAX {
                        label[w] = id;
                        stack.push(w);
                    }
                }
            }
            sizes.push(size);
        }
        (label, sizes)
    }

    /// Joins pieces of the solid that float apart from the body (a hand
    /// modelled separately from its forearm, a gap at the wrist) to it with
    /// a one-voxel-thick bridge along the shortest way across, so geodesics
    /// reach them and the bridge reads as the narrowest section there: the
    /// joint. Only pieces of at least `min_fraction` of the body's volume and
    /// at most `max_gap` (world units) away are joined; specks stay apart.
    /// Returns how many pieces were joined.
    pub fn bridge(&mut self, max_gap: f32, min_fraction: f32) -> usize {
        let (mut label, sizes) = self.pieces();
        let Some(body) = (0..sizes.len()).max_by_key(|&i| sizes[i]) else {
            return 0;
        };
        let body = body as u32;
        let least = min_fraction * sizes[body as usize] as f32;
        let wanted = |id: u32| id != body && sizes[id as usize] as f32 >= least;
        let limit = max_gap / self.cell;
        let offsets = neighbours(self.dims);
        let n = self.solid.len();
        let mut joined = 0;
        loop {
            // Dijkstra out of the body through empty voxels to the nearest
            // piece worth joining.
            let mut dist = vec![f32::INFINITY; n];
            let mut prev = vec![u32::MAX; n];
            let mut heap = BinaryHeap::new();
            for v in 0..n {
                if label[v] == body {
                    dist[v] = 0.;
                    heap.push(Reverse((0u32, v as u32)));
                }
            }
            let mut reached = None;
            while let Some(Reverse((bits, v))) = heap.pop() {
                let v = v as usize;
                let d = f32::from_bits(bits);
                if d > dist[v] {
                    continue;
                }
                if self.solid[v] && label[v] != body {
                    reached = Some(v);
                    break;
                }
                // Offsets from the grid's outer layer would wrap round to
                // the far side: the path stops there (it is padding anyway).
                let [x, y, z] = self.coords(v);
                if [x, y, z].contains(&0)
                    || x + 1 == self.dims[0]
                    || y + 1 == self.dims[1]
                    || z + 1 == self.dims[2]
                {
                    continue;
                }
                for &(off, len) in &offsets {
                    let w = (v as isize + off) as usize;
                    let nd = d + len;
                    let open = !self.solid[w] || wanted(label[w]);
                    if open && nd <= limit && nd < dist[w] {
                        dist[w] = nd;
                        prev[w] = v as u32;
                        heap.push(Reverse((nd.to_bits(), w as u32)));
                    }
                }
            }
            let Some(hit) = reached else {
                return joined;
            };
            let piece = label[hit];
            let mut v = prev[hit] as usize;
            while label[v] != body {
                self.solid[v] = true;
                label[v] = body;
                v = prev[v] as usize;
            }
            for l in label.iter_mut().filter(|l| **l == piece) {
                *l = body;
            }
            joined += 1;
        }
    }

    /// Multi-source Dijkstra through solid voxels (26-connected, edge
    /// lengths 1, √2, √3 voxels). Returns each voxel's distance in voxels
    /// (infinite when unreached), the label of its nearest source and its
    /// predecessor on the shortest path (`u32::MAX` at a source).
    pub fn geodesic(&self, sources: &[(usize, u16)]) -> Geodesic {
        let n = self.solid.len();
        let mut dist = vec![f32::INFINITY; n];
        let mut label = vec![u16::MAX; n];
        let mut prev = vec![u32::MAX; n];
        let mut heap = BinaryHeap::new();
        for &(v, l) in sources {
            if self.solid[v] && dist[v] > 0. {
                dist[v] = 0.;
                label[v] = l;
                heap.push(Reverse((0u32, v as u32)));
            }
        }
        let offsets = neighbours(self.dims);
        while let Some(Reverse((bits, v))) = heap.pop() {
            let v = v as usize;
            let d = f32::from_bits(bits);
            if d > dist[v] {
                continue;
            }
            for &(off, len) in &offsets {
                let w = (v as isize + off) as usize;
                if !self.solid[w] {
                    continue;
                }
                let nd = d + len;
                if nd < dist[w] {
                    dist[w] = nd;
                    label[w] = label[v];
                    prev[w] = v as u32;
                    // Non-negative floats order like their bit patterns.
                    heap.push(Reverse((nd.to_bits(), w as u32)));
                }
            }
        }
        Geodesic { dist, label, prev }
    }

    /// Whether solid voxel `v` has no solid neighbour farther than itself in
    /// `dist` (a tip of the geodesic field).
    pub fn is_local_max(&self, v: usize, dist: &[f32]) -> bool {
        neighbours(self.dims).iter().all(|&(off, _)| {
            let w = (v as isize + off) as usize;
            !self.solid[w] || dist[w] <= dist[v]
        })
    }

    /// A planar cross-section of the solid through `centre`, perpendicular
    /// to `normal`, keeping only the connected piece around `centre` and
    /// looking no farther than `limit`.
    pub fn section(&self, centre: Vec3, normal: Vec3, limit: f32) -> Section {
        let normal = normal.try_normalize().unwrap_or(Vec3::Y);
        let u = normal.any_orthonormal_vector();
        let w = normal.cross(u);
        let h = 0.75 * self.cell;
        let m = (limit / h).ceil().max(1.) as i64;
        let side = (2 * m + 1) as usize;
        let at = |i: i64, j: i64| centre + u * (i as f32 * h) + w * (j as f32 * h);
        let solid = |i: i64, j: i64| self.solid_at(at(i, j));
        // Start at the centre, or the nearest solid sample within 3 voxels.
        let reach = (3. * self.cell / h).ceil() as i64;
        let mut start = None;
        'ring: for r in 0..=reach.min(m) {
            for i in -r..=r {
                for j in -r..=r {
                    if (i.abs() == r || j.abs() == r) && solid(i, j) {
                        start = Some((i, j));
                        break 'ring;
                    }
                }
            }
        }
        let Some(start) = start else {
            return Section::default();
        };
        let mut seen = vec![false; side * side];
        let key = |i: i64, j: i64| ((i + m) as usize) * side + (j + m) as usize;
        seen[key(start.0, start.1)] = true;
        let mut stack = vec![start];
        let (mut count, mut sum, mut far, mut truncated) = (0usize, Vec3::ZERO, 0f32, false);
        while let Some((i, j)) = stack.pop() {
            count += 1;
            let p = at(i, j);
            sum += p;
            far = far.max((p - centre).length());
            if i.abs() == m || j.abs() == m {
                truncated = true;
            }
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (a, b) = (i + di, j + dj);
                if a.abs() > m || b.abs() > m || seen[key(a, b)] {
                    continue;
                }
                seen[key(a, b)] = true;
                if solid(a, b) {
                    stack.push((a, b));
                }
            }
        }
        Section {
            found: true,
            area: count as f32 * h * h,
            centroid: sum / count as f32,
            reach: far + 0.5 * h,
            truncated,
        }
    }
}

pub(crate) struct Geodesic {
    pub dist: Vec<f32>,
    pub label: Vec<u16>,
    pub prev: Vec<u32>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Section {
    pub found: bool,
    pub area: f32,
    pub centroid: Vec3,
    /// Distance from the section's centre to its farthest solid sample.
    pub reach: f32,
    /// The piece ran into `limit`: it is not a limb's cross-section any more
    /// but the torso's.
    pub truncated: bool,
}

/// One-dimensional squared distance transform of sampled function `f`.
fn envelope(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let fq = f[q] + (q * q) as f64;
        let mut s;
        loop {
            let p = v[k];
            s = (fq - (f[p] + (p * p) as f64)) / (2. * q as f64 - 2. * p as f64);
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        // z[0] is −∞, so the loop always stops with s > z[k].
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        *out = dq * dq + f[p];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_is_the_exact_squared_distance() {
        let f = [0., 1e12, 1e12, 1e12, 0., 1e12];
        let mut d = [0.; 6];
        let (mut v, mut z) = ([0usize; 6], [0f64; 7]);
        envelope(&f, &mut d, &mut v, &mut z);
        assert_eq!(d, [0., 1., 4., 1., 0., 1.]);
    }
}
