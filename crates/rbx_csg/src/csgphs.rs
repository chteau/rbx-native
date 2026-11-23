//! `PhysicalConfigData`, the collision geometry Studio saves on a union (see
//! `docs/research/csg-solidmeshholder.md`; the layout was read off blobs
//! Studio saved). `CollisionFidelity` itself is never saved: Studio reads
//! it back from this blob, and a union without one loads as `Box`.
//!
//! `CSGPHS`, then a `u32`: 0 for `Box`, whose blob ends with `BLOCK`, and
//! 8 otherwise. A fidelity byte and a zero byte follow (1 `Hull`,
//! 3 `PreciseConvexDecomposition`, 6 `Default`), then a zstd frame holding
//! convex hulls in the union's own space at `InitialSize`:
//!
//! - nine `u32`s: hull count, total vertices, total triangles, the most
//!   vertices and the most triangles one hull has, 0, the connectivity's
//!   length in bits and in bytes, and the vertices' length in bytes;
//! - the bounds of every vertex, min then max, six `f32`s;
//! - each hull's triangles, Edgebreaker-coded one after another (see
//!   [`encode_hull`]);
//! - every hull's vertices, `f32` xyz, in the order the coding visits them.

use std::io::Read;

use glam::DVec3;

use crate::decompose::decompose;
use crate::hull::Hull;
use crate::Solid;

const MAGIC: &[u8; 6] = b"CSGPHS";
const HEADER: usize = 12;
const COUNTS: usize = 9 * 4;
const BOUNDS: usize = 6 * 4;

/// `Enum.CollisionFidelity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fidelity {
    Default,
    Hull,
    Box,
    PreciseConvexDecomposition,
}

impl Fidelity {
    /// The enum's value, as a property holds it.
    pub fn from_value(value: u32) -> Fidelity {
        match value {
            1 => Fidelity::Hull,
            2 => Fidelity::Box,
            3 => Fidelity::PreciseConvexDecomposition,
            _ => Fidelity::Default,
        }
    }

    pub fn value(self) -> u32 {
        match self {
            Fidelity::Default => 0,
            Fidelity::Hull => 1,
            Fidelity::Box => 2,
            Fidelity::PreciseConvexDecomposition => 3,
        }
    }

    /// The byte after the header's `u32`.
    fn code(self) -> u8 {
        match self {
            Fidelity::Default => 6,
            Fidelity::Hull => 1,
            Fidelity::Box => 0,
            Fidelity::PreciseConvexDecomposition => 3,
        }
    }
}

/// One hull as the blob holds it: vertices and outward triangles.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// What [`decode`] reads back.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub fidelity: Fidelity,
    /// Empty for `Box`.
    pub hulls: Vec<Shape>,
}

/// The `PhysicalConfigData` Studio would save for `solid` (in the union's
/// own space) at `fidelity`: its hull, or its convex pieces.
pub fn encode(solid: &Solid, fidelity: Fidelity) -> Vec<u8> {
    let hulls = match fidelity {
        Fidelity::Box => Vec::new(),
        Fidelity::Hull => Hull::new(&points(solid)).into_iter().collect(),
        Fidelity::Default => decompose(solid, 0.02, 16),
        Fidelity::PreciseConvexDecomposition => decompose(solid, 0.002, 32),
    };
    if hulls.is_empty() {
        return [&MAGIC[..], &[0; 4], b"BLOCK"].concat();
    }
    let shapes: Vec<Shape> = hulls
        .iter()
        .map(|h| Shape {
            vertices: h.points.iter().map(|p| p.as_vec3().to_array()).collect(),
            triangles: h.triangles.clone(),
        })
        .collect();
    encode_shapes(&shapes, fidelity)
}

/// Every corner of `solid`'s polygons.
pub(crate) fn points(solid: &Solid) -> Vec<DVec3> {
    solid
        .polygons
        .iter()
        .flat_map(|p| p.vertices.iter().copied())
        .collect()
}

/// The blob for hulls already made, each coded from its first triangle.
pub fn encode_shapes(shapes: &[Shape], fidelity: Fidelity) -> Vec<u8> {
    let mut bits = BitWriter::default();
    let mut vertices = Vec::new();
    let (mut most_vertices, mut most_triangles, mut triangles) = (0, 0, 0);
    for shape in shapes {
        let order = encode_hull(&shape.triangles, &mut bits);
        vertices.extend(order.iter().map(|&i| shape.vertices[i as usize]));
        most_vertices = most_vertices.max(order.len());
        most_triangles = most_triangles.max(shape.triangles.len());
        triangles += shape.triangles.len();
    }
    let (count, words) = bits.finish();
    let (lo, hi) = vertices.iter().fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(lo, hi), v| {
            (
                [lo[0].min(v[0]), lo[1].min(v[1]), lo[2].min(v[2])],
                [hi[0].max(v[0]), hi[1].max(v[1]), hi[2].max(v[2])],
            )
        },
    );
    let mut raw = Vec::new();
    for value in [
        shapes.len(),
        vertices.len(),
        triangles,
        most_vertices,
        most_triangles,
        0,
        count,
        words.len() * 4,
        vertices.len() * 12,
    ] {
        raw.extend((value as u32).to_le_bytes());
    }
    for value in lo.iter().chain(&hi) {
        raw.extend(value.to_le_bytes());
    }
    for word in words {
        raw.extend(word.to_le_bytes());
    }
    for value in vertices.iter().flatten() {
        raw.extend(value.to_le_bytes());
    }
    let mut out = Vec::from(&MAGIC[..]);
    out.extend(8u32.to_le_bytes());
    out.extend([fidelity.code(), 0]);
    out.extend(zstd_frame(&raw));
    out
}

/// `raw` as one zstd frame of stored blocks that declares its size up
/// front, as Studio's own frames do: a frame without the size (what a
/// streaming compressor writes) makes Studio drop the blob and collide the
/// union as a box.
fn zstd_frame(raw: &[u8]) -> Vec<u8> {
    const MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
    const SINGLE_SEGMENT: u8 = 0x20;
    const BLOCK: usize = 128 * 1024;
    let mut out = MAGIC.to_vec();
    let size = raw.len() as u64;
    // The frame header: single segment, content size in 1, 2 or 4 bytes.
    match size {
        0..256 => out.extend([SINGLE_SEGMENT, size as u8]),
        256..65792 => {
            out.push(0x40 | SINGLE_SEGMENT);
            out.extend(((size - 256) as u16).to_le_bytes());
        }
        _ => {
            out.push(0x80 | SINGLE_SEGMENT);
            out.extend((size as u32).to_le_bytes());
        }
    }
    let mut blocks = raw.chunks(BLOCK).peekable();
    if blocks.peek().is_none() {
        out.extend([1, 0, 0]);
    }
    while let Some(block) = blocks.next() {
        // A raw block: `last | type 0 << 1 | size << 3`, 3 bytes.
        let header = u32::from(blocks.peek().is_none()) | (block.len() as u32) << 3;
        out.extend(&header.to_le_bytes()[..3]);
        out.extend(block);
    }
    out
}

/// Only the fidelity a blob was made at, without unpacking it.
pub fn fidelity(bytes: &[u8]) -> Option<Fidelity> {
    if !bytes.starts_with(MAGIC) || bytes.len() < HEADER - 1 {
        return None;
    }
    if bytes[6..10] == [0; 4] {
        return (&bytes[10..] == b"BLOCK").then_some(Fidelity::Box);
    }
    match bytes.get(10)? {
        1 => Some(Fidelity::Hull),
        3 => Some(Fidelity::PreciseConvexDecomposition),
        6 => Some(Fidelity::Default),
        _ => None,
    }
}

/// Reads a `PhysicalConfigData` blob back.
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    let fidelity = fidelity(bytes)?;
    if fidelity == Fidelity::Box {
        return Some(Decoded {
            fidelity,
            hulls: Vec::new(),
        });
    }
    let mut raw = Vec::new();
    ruzstd::decoding::StreamingDecoder::new(bytes.get(HEADER..)?)
        .ok()?
        .read_to_end(&mut raw)
        .ok()?;
    let word = |i: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            raw.get(i * 4..i * 4 + 4)?.try_into().ok()?,
        ))
    };
    let (count, total_vertices, bits, bit_bytes) = (word(0)?, word(1)?, word(6)?, word(7)?);
    let words: Vec<u32> = (0..bit_bytes as usize / 4)
        .map(|i| word((COUNTS + BOUNDS) / 4 + i))
        .collect::<Option<_>>()?;
    let floats = COUNTS + BOUNDS + bit_bytes as usize;
    let mut reader = BitReader::new(&words, bits as usize);
    let mut hulls = Vec::new();
    let mut first = 0usize;
    for _ in 0..count {
        let symbols = reader.hull()?;
        let triangles = decode_hull(&symbols)?;
        let n = triangles
            .iter()
            .flatten()
            .max()
            .map_or(0, |&m| m as usize + 1);
        let vertices = (first..first + n)
            .map(|v| {
                let at = floats + v * 12;
                let f = |k: usize| -> Option<f32> {
                    Some(f32::from_le_bytes(
                        raw.get(at + k * 4..at + k * 4 + 4)?.try_into().ok()?,
                    ))
                };
                Some([f(0)?, f(1)?, f(2)?])
            })
            .collect::<Option<_>>()?;
        first += n;
        hulls.push(Shape {
            vertices,
            triangles,
        });
    }
    (first == total_vertices as usize).then_some(Decoded { fidelity, hulls })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Symbol {
    C,
    L,
    E,
    R,
    S,
}

impl Symbol {
    /// The code and its length: `0` for C, then 3 bits each.
    fn code(self) -> (u32, u32) {
        match self {
            Symbol::C => (0b0, 1),
            Symbol::S => (0b100, 3),
            Symbol::R => (0b101, 3),
            Symbol::L => (0b110, 3),
            Symbol::E => (0b111, 3),
        }
    }
}

/// Bits pushed most significant first into `u32` words; the last, partial
/// word keeps its bits at the bottom.
#[derive(Default)]
struct BitWriter {
    words: Vec<u32>,
    current: u32,
    fill: u32,
    count: usize,
}

impl BitWriter {
    fn push(&mut self, symbol: Symbol) {
        let (code, len) = symbol.code();
        for k in (0..len).rev() {
            self.current = (self.current << 1) | ((code >> k) & 1);
            self.fill += 1;
            self.count += 1;
            if self.fill == 32 {
                self.words.push(self.current);
                (self.current, self.fill) = (0, 0);
            }
        }
    }

    fn finish(mut self) -> (usize, Vec<u32>) {
        if self.fill > 0 {
            self.words.push(self.current);
        }
        (self.count, self.words)
    }
}

struct BitReader<'a> {
    words: &'a [u32],
    total: usize,
    at: usize,
}

impl<'a> BitReader<'a> {
    fn new(words: &'a [u32], total: usize) -> Self {
        BitReader {
            words,
            total,
            at: 0,
        }
    }

    fn bit(&mut self) -> Option<u32> {
        if self.at >= self.total {
            return None;
        }
        let word = self.at / 32;
        // Bits in the last, partial word sit at its bottom.
        let width = if word + 1 == self.words.len() && !self.total.is_multiple_of(32) {
            self.total % 32
        } else {
            32
        };
        let bit = (self.words.get(word)? >> (width - 1 - self.at % 32)) & 1;
        self.at += 1;
        Some(bit)
    }

    fn symbol(&mut self) -> Option<Symbol> {
        if self.bit()? == 0 {
            return Some(Symbol::C);
        }
        Some(match (self.bit()?, self.bit()?) {
            (0, 0) => Symbol::S,
            (0, 1) => Symbol::R,
            (1, 0) => Symbol::L,
            _ => Symbol::E,
        })
    }

    /// One hull's symbols: up to the E that closes its last open split.
    fn hull(&mut self) -> Option<Vec<Symbol>> {
        let mut symbols = Vec::new();
        let mut open = 1;
        while open > 0 {
            let symbol = self.symbol()?;
            match symbol {
                Symbol::S => open += 1,
                Symbol::E => open -= 1,
                _ => {}
            }
            symbols.push(symbol);
        }
        Some(symbols)
    }
}

/// Edgebreaker over a closed, outward hull (Rossignac's CLERS, the way
/// Studio's blobs run): the first triangle's corners are vertices 0, 1, 2,
/// and the walk starts on the loop 0, 2, 1 at the gate 0 to 2. At each gate
/// `a -> b` the triangle beyond it, `(a, b, x)`, is C when `x` is new (the
/// gate moves to `x -> b`), R when `x` follows `b` on the loop (`b` leaves,
/// gate `a -> x`), L when `x` precedes `a` (`a` leaves, gate `x -> b`), E
/// when it closes a 3-vertex loop, and S otherwise: the loop splits at `x`,
/// the `b .. x` side walked first from `x -> b`, then the `x .. a` side
/// from `a -> x`. Returns the vertices in the order the walk numbers them.
fn encode_hull(triangles: &[[u32; 3]], bits: &mut BitWriter) -> Vec<u32> {
    let tip: std::collections::HashMap<(u32, u32), u32> = triangles
        .iter()
        .flat_map(|&[a, b, c]| [((a, b), c), ((b, c), a), ((c, a), b)])
        .collect();
    let [v0, v1, v2] = triangles[0];
    let mut order = vec![v0, v1, v2];
    let mut seen: std::collections::HashSet<u32> = order.iter().copied().collect();
    let mut stack = vec![(vec![v0, v2, v1], 0usize)];
    while let Some((mut ring, mut gate)) = stack.pop() {
        loop {
            let n = ring.len();
            let (a, b) = (ring[gate % n], ring[(gate + 1) % n]);
            let Some(&x) = tip.get(&(a, b)) else {
                return order;
            };
            if seen.insert(x) {
                bits.push(Symbol::C);
                order.push(x);
                ring.insert(gate % n + 1, x);
                gate = gate % n + 1;
            } else if n == 3 {
                bits.push(Symbol::E);
                break;
            } else if x == ring[(gate + 2) % n] {
                bits.push(Symbol::R);
                ring.remove((gate + 1) % n);
                gate = ring.iter().position(|&v| v == a).unwrap_or(0);
            } else if x == ring[(gate + n - 1) % n] {
                bits.push(Symbol::L);
                ring.remove(gate % n);
                gate = ring.iter().position(|&v| v == x).unwrap_or(0);
            } else {
                bits.push(Symbol::S);
                ring.rotate_left(gate % n);
                let k = ring.iter().position(|&v| v == x).unwrap_or(2);
                let right = ring[1..=k].to_vec();
                let mut left = ring[k..].to_vec();
                left.push(a);
                let (r, l) = (right.len() - 1, left.len() - 1);
                // The right side is walked first: it goes on the stack last.
                stack.push((left, l));
                ring = right;
                gate = r;
            }
        }
    }
    order
}

/// The triangles [`encode_hull`]'s symbols stand for, over the vertex
/// numbers it handed out.
fn decode_hull(symbols: &[Symbol]) -> Option<Vec<[u32; 3]>> {
    let mut triangles = vec![[0, 1, 2]];
    let mut next = 3u32;
    let mut stack = vec![(vec![0u32, 2, 1], 0usize)];
    let mut at = 0;
    while let Some((mut ring, mut gate)) = stack.pop() {
        loop {
            let n = ring.len();
            let (a, b) = (ring[gate % n], ring[(gate + 1) % n]);
            match *symbols.get(at)? {
                Symbol::C => {
                    triangles.push([a, b, next]);
                    ring.insert(gate % n + 1, next);
                    gate = gate % n + 1;
                    next += 1;
                }
                Symbol::E => {
                    if n != 3 {
                        return None;
                    }
                    triangles.push([a, b, ring[(gate + 2) % n]]);
                    at += 1;
                    break;
                }
                Symbol::R if n > 3 => {
                    let x = ring[(gate + 2) % n];
                    triangles.push([a, b, x]);
                    ring.remove((gate + 1) % n);
                    gate = ring.iter().position(|&v| v == a)?;
                }
                Symbol::L if n > 3 => {
                    let x = ring[(gate + n - 1) % n];
                    triangles.push([a, b, x]);
                    ring.remove(gate % n);
                    gate = ring.iter().position(|&v| v == x)?;
                }
                Symbol::S => {
                    // The right side is a disk whose boundary has
                    // `triangles - 2 * new vertices + 2` corners: count its
                    // symbols, up to the E that closes it.
                    let (mut open, mut len, mut new) = (1, 0i64, 0i64);
                    for &s in &symbols[at + 1..] {
                        len += 1;
                        match s {
                            Symbol::C => new += 1,
                            Symbol::S => open += 1,
                            Symbol::E => open -= 1,
                            _ => {}
                        }
                        if open == 0 {
                            break;
                        }
                    }
                    let k = usize::try_from(len - 2 * new + 2).ok()?;
                    if open != 0 || k < 3 || k > n - 2 {
                        return None;
                    }
                    ring.rotate_left(gate % n);
                    let x = ring[k];
                    triangles.push([a, b, x]);
                    let right = ring[1..=k].to_vec();
                    let mut left = ring[k..].to_vec();
                    left.push(a);
                    let l = left.len() - 1;
                    stack.push((left, l));
                    gate = right.len() - 1;
                    ring = right;
                }
                _ => return None,
            }
            at += 1;
        }
    }
    (at == symbols.len()).then_some(triangles)
}

#[cfg(test)]
mod tests;
