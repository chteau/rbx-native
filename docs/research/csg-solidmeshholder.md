# Union geometry in current Studio saves: `SolidMeshHolder`

Observed on 2026-11-22 against real Roblox Studio (Vinegar), using unions
built from primitive parts in an empty place (`BasePart:UnionAsync` /
`SubtractAsync` from the command bar), saved to `.rbxl`, `.rbxlx` and
exported to `.rbxm`. These notes only describe saved files. No Studio code
was read.

## Where the geometry lives

| Property | Binary type | XML tag | Content in a fresh Studio save |
|---|---|---|---|
| `SolidMeshHolder` | SharedString (0x1C) | `<NetAssetRef>` | the union's mesh, 0.6-15 KB here |
| `PhysicalConfigData` | SharedString | `<SharedString>` | `CSGPHS` + zstd frame (collision hull) |
| `ChildData2` | SharedString | `<SharedString>` | the source parts (what Separate uses) |
| `MeshData2` | SharedString | `<SharedString>` | **empty** |
| `MeshData`, `ChildData`, `PhysicsData` | String | `<BinaryString>` | empty |

Studio's own error text gives the property type away: an old place whose
unions carry `SolidMeshHolder` as an empty **String** column (our
`marked.rbxl` fixture) doesn't open at all in current Studio:

> Unexpected format 1 (expected 28) << NetworkAssetRef << property values,
> name=SolidMeshHolder, type=SolidMeshHolder << chunk#456[PROP] ...

So it's a `NetworkAssetRef` that only loads from a SharedString column
(type 28 = 0x1C). In XML the same value is a `<NetAssetRef>` element that
points at the `<SharedStrings>` table. A `<SharedString name="SolidMeshHolder">`
element is refused too ("type conversion from SharedString ...").

Studio draws a union from `SolidMeshHolder`. With it removed and the other
properties untouched, the union still loads (its `TriangleCount` too) but
nothing is drawn.

## What this meant for our writers (fixed on `fix/union-solidmeshholder`)

- `rbx_binary` used to drop every `SolidMeshHolder` column (it was added
  for `marked.rbxl`'s legacy String column). Opening a Studio-made place and
  saving it from rbx-native stripped the geometry from every union: they
  were invisible in Studio afterwards. The writer now drops only a column with
  no SharedString value (the legacy shape) and writes the geometry back
  byte for byte.
- `rbx_xml` wrote it as `<SharedString>`, so Studio refused to open the
  re-saved `.rbxlx`. It now writes `<NetAssetRef>` and leaves out the
  legacy String shape.

## The `SolidMeshHolder` blob

Every field below is little-endian. The layout comes from comparing blobs of
known shapes: two touching 2-stud cubes (`TwoCubes`, 679 bytes), the same
two cubes apart (667 bytes), a block ∪ ball, and two block − cylinder/ball.

```
0x00  u8   3                 (version?)
0x01  u8   1
0x02  u32  N = total length - 14
0x06  u32  0                 (compressed size? 0 = stored raw, like a binary chunk header)
0x0A  u32  N                 (uncompressed size)
0x0E  "SolidMesh"            9 ASCII bytes, no terminator
0x17  u32  0
then a run of arrays, each:  u32 count, u32 0 (count high half?), payload
  positions  count × vec3 f32      TwoCubes: the 8 corners of the 4×2×2 box, part-local
  normals    count × vec3 f32      TwoCubes: 18
  colors     count × vec3 f32      TwoCubes: 14 × (0.639, 0.635, 0.647) = Medium stone grey
  index streams, count bytes each, 4 streams (TwoCubes: 36, 36, 0, 36)
```

The blob ends exactly after the fourth index stream. Each stream holds one
byte per triangle corner (36 = 12 triangles × 3, 72 for the two separate
cubes). Each byte is a 7-bit signed delta from the previous index
(0x7F = -1, 0x7E = -2, ...). For the first stream, the running sum stays
within 0..7, the eight positions. The second and fourth streams index into
the normal and color arrays the same way. The empty third stream is probably
UVs, since the union has no textures. Multi-byte deltas (larger meshes) were
not decoded.

Studio produced byte-identical blobs for the same union in two separate
saves (`.rbxl` and `.rbxlx`), so the encoding is deterministic.

`PhysicalConfigData` starts with `CSGPHS`, a small header
(`08 00 00 00 06 00` here), then a zstd frame (`28 B5 2F FD`). Its contents
weren't decoded.

## Can we write it for unions we make?

Probably yes for the drawing side, but untested. The container is simple.
Positions, normals and colors are plain f32 arrays, and the index streams are
byte deltas. We already compute all of that in `crates/rbx_viewer/src/scene/union/csg.rs`.
Still unknown:

- the meaning of the two leading bytes and the zero words (versions or
  flags), and whether Studio checks them;
- how normals and colors get split (18 normals and 14 colors for a box with
  8 corners, so they're deduplicated per face or per corner by some rule we
  haven't pinned down);
- the multi-byte form of a delta, needed past ±63;
- whether Studio also needs a matching `PhysicalConfigData`, or rebuilds the
  collision hull when that property is missing (CollisionFidelity Box is what
  we see today).

Next step: write a `TwoCubes` blob by hand from our CSG output, drop it into
the fixture in place of Studio's, and check whether Studio draws it.

Fixtures: `assets/tests/csg/TwoCubesUnion.rbxm` (Studio export) and
`TwoCubesUnion.rbxmx` (trimmed from a Studio-saved `.rbxlx`). Round-trip
tests: `crates/rbx_xml/tests/union_geometry_round_trip.rs`.

## `PhysicalConfigData` (observed 2026-11-23)

Studio does not save `CollisionFidelity`. It reads it back from this blob,
so the blob is the only thing that decides how a union collides.

- `Box`: `CSGPHS`, four zero bytes, then `BLOCK`. Nothing else.
- Every other fidelity: `CSGPHS`, a u32 code (`Hull` 1,
  `PreciseConvexDecomposition` 3, `Default` 6), then one zstd frame. Inside
  the frame is a list of convex hulls in the union's own space. Each hull is
  a vertex count, f32 positions, and its triangles in Edgebreaker CLERS
  order. The bits are packed MSB first, and the last word is right-aligned.
  `crates/rbx_csg/src/csgphs.rs` re-encodes every Studio hull we saved back
  to the same bytes.
- Studio ignores a frame that does not declare its content size: the union
  then collides as a box. Our writer emits single-segment raw blocks.
- Studio's drilled 8x4x8 block with a 3-stud hole: `Hull` is 1 hull, which
  fills the hole. `Default` is 4 hulls and `Precise` 24, and both leave the
  hole open.

What we write: `Hull` is a quickhull of the result. `Default` and `Precise`
use our own approximate convex decomposition (`rbx_csg::decompose`): it
cuts along axis planes until every piece is within 2% (`Default`) or 0.2%
(`Precise`) of its own hull. Setting `CollisionFidelity` in the editor
rewrites the blob.

Checked in Studio:
- the Properties panel shows the fidelity we wrote;
- raycasts down the hole miss for `Default` and `Precise` and hit for `Hull`
  and `Box`;
- in Run mode, balls drop through our `Default` and `Precise` holes, as
  they do through Studio's own, and rest on our `Box` and `Hull` unions.

## Colours

`SolidMeshHolder` stores a colour for every triangle. When `UsePartColor`
is off, Studio draws each face in the colour of the part it came from,
including the walls of a hole, which take the negated part's colour. The
viewer now draws those vertex colours. It uses the carved mesh's polygon
colours and the decoded per-corner colours of a baked mesh.

## Negate

The Negate button does not wrap the part. It tags the part `rbxNegated` and
sets its Transparency to 0.5, and pressing Negate again undoes both. The
union then wraps the part as a `NegateOperation` named `NegativePart`:
- `Color3uint8` is white, `Material` is 256, and `Transparency` is 0.1;
- it has an `InitialSize`;
- it has no `SolidMeshHolder` or `PhysicalConfigData`;
- its `ChildData2` holds the part with the tag removed.

`plugin:Negate` still returns a top-level `NegateOperation`.
