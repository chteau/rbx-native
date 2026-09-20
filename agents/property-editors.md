# Property editors — what exists, what's missing

Every `Variant` the DOM can hold, what the Properties panel does with it,
and what a purpose-built editor would be.

**Status: A1, A4, A5, B1, B2, B3, B4, B5, B6, B7 and B8 have shipped.** The rest
is still the triage list it started as.

Two facts that shape the whole list:

- A type with no `edit_text` arm is **read-only in the panel** — it renders
  a value and offers no editor at all. That is most of section B.
- `Properties::is_read_only` also blocks anything the reflection dump marks
  unsaveable (`BasePart.Size`, for instance), independently of type.

---

## The one you named: `CFrame`

`CFrameData` is `{ position: Vector3, rotation: [f32; 9] }` — a point and a
3×3 rotation matrix.

**Today:** the row shows *position only*, as three fields. The rotation is
invisible and unreachable. It is not lost — a commit keeps the previous
matrix — but there is no way to see or set it.

Oddly, the *parser* is already ahead of the UI: `parse_cframe` accepts 3
numbers (position), 9 (rotation), or 12 (both). Only the row builder is
stuck at three fields.

### Three ways to fix it

**1. Two labelled sub-rows — `Position` X/Y/Z, `Orientation` X/Y/Z.**
What Roblox Studio itself shows on a `BasePart`, so muscle memory carries
over, and degrees are what people actually type. Needs a matrix ↔ Euler
conversion in Roblox's own `YXZ` order.

**2. One six-field row**, position and rotation side by side under two
captions. Cheapest; a six-field row is wide and reads as one undifferentiated
strip at a 300px dock width.

**3. Position fields plus a raw 3×3 matrix grid.** Exact and lossless.
Nobody thinks in matrices.

**Recommendation: 1.** With one correctness rule worth stating up front,
because it is the part that bites: a 3×3 matrix carries more than three
Euler angles can, so converting *out* and back *in* is not guaranteed to
reproduce the original nine floats. So the rotation should be written **only
when the angle fields are actually edited** — editing position alone must
leave the stored matrix byte-identical rather than round-tripping it through
degrees. Same rule for a part that happens to sit at a gimbal-lock
orientation.

---

## A. Wrong or misleading today

| # | Type | Today | Would be |
|---|---|---|---|
| ~~A1~~ | **`CFrame`** | ✅ **done** — Position and Orientation, two captioned lines | |
| A2 | **`BrickColor`** | raw palette index, typed. Displays as `BrickColor(194)` | a named swatch picker. Needs the ~64-entry palette table bundled — it isn't, today |
| A3 | **`Font`** | three typed text fields: Family / Weight / Style | family dropdown + weight dropdown (9 members) + italic toggle. Weight and style are closed sets being typed by hand |
| ~~A4~~ | **`NumberRange`** | ✅ **done** — Min / Max fields | |
| ~~A5~~ | **`UDim`** | ✅ **done** — Scale / Offset, matching `UDim2` | |

## B. Read-only today — no editor at all

| # | Type | Where it shows up | Would be |
|---|---|---|---|
| ~~B1~~ | **`Vector3int16`** | ✅ **done** — three integer fields | |
| ~~B2~~ | **`Ray`** | ✅ **done** — Origin and Direction, two captioned lines | |
| ~~B3~~ | **`Faces`** | ✅ **done** — six named checkboxes | |
| ~~B4~~ | **`Axes`** | ✅ **done** — three named checkboxes | |
| ~~B5~~ | **`OptionalCFrame`** | ✅ **done** — a present/absent checkbox above the `CFrame` editor, which draws only while there is a value | |
| ~~B6~~ | **`PhysicalProperties`** | ✅ **done** — a Custom checkbox over the five numbers | |
| ~~B7~~ | **`NumberSequence`** | ✅ **done** — a draggable curve with an envelope band, in a graph panel the row opens | |
| ~~B8~~ | **`ColorSequence`** | ✅ **done** — a gradient ramp with draggable stops and a colour picker, in the same panel | |
| B9 | **`Ref`** | `ObjectValue.Value`, `Weld.Part0` | an instance picker (Explorer target, or "pick in viewport"). Shows the target's name today |
| B10 | **`Content`** | `Decal.Texture`, `MeshPart.MeshId` | an asset URI field; the `Content::Object` case is a `Ref` picker |

## C. Fine as they are

`String`, `Bool`, `Int32`, `Int64`, `Float32`, `Float64` — plain fields.
`Enum` — dropdown, from the reflection dump.
`Color3`, `Color3uint8` — colour picker.
`Vector2`, `Vector3`, `Rect` — labelled numeric fields.

## D. Deliberately read-only, and should stay that way

`SharedString`, `UniqueId`, `SecurityCapabilities`, `Unknown` — identity and
opaque payloads. Editing them by hand corrupts a file rather than editing it.

---

## What is left, and why it was left

- **A2 `BrickColor`** — needs the ~64-entry palette table bundled. That is
  an asset decision, not a UI one, and belongs with whoever decides where
  that table comes from.
- **A3 `Font`** — wants a family list, which lives in the viewer crate, not
  here. The weight and style halves are easy; the family is the whole job.
- **B9 `Ref`, B10 `Content`** — each is its own editor with its own
  interaction model. Neither fits in a property row. (B7/B8 were the same
  case and got exactly that: a panel of their own, which the row opens.)

## Rough sizing for what remains

- **Medium** (a new control, self-contained): A3's weight/style halves
- **Large** (a new editor with its own interaction): A2, B9, B10
