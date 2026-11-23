---
name: build-modeling
description: Low-poly modeling in Roblox with Parts, WedgeParts, CornerWedgeParts, cylinders and MeshParts in the clean Kenney-like style: proportions, chunky silhouettes, fake bevels and chamfers, when to cut with unions (SubtractAsync, UnionAsync, IntersectAsync) and when to frame with parts, part budgets, naming, and grouping into Models with a PrimaryPart and a sensible pivot. Pull it before building any prop, building or vehicle out of parts.
---

# Build modeling

Goal: props that read at a glance from 30 studs away, built from few, chunky pieces, every one named and grouped. The reference look is Kenney's free CC0 3D kits (https://kenney.nl/assets/category:3D): simple exaggerated forms, flat colours, soft chamfers, no noise. Use them to calibrate taste, not to copy a model. Give each build its own idea.

Pull `build-color-and-materials` for colours and `build-modular-kits` when the piece belongs to a kit.

## How to build here

- Build with `execute_luau`, `datamodel_type: "Edit"`. One call per object keeps each one a single undo step and a short script you can rerun.
- Look at it with `screen_capture` (`camera_position` and `look_at_position` in studs, both or neither), from at least two angles: a 3/4 view from above and a player's eye view about 5 studs up.
- Find what exists with `search_game_tree` (`path: "Workspace"`, `instance_type: "Model"`) and check one with `inspect_instance` before editing it.
- Save with `save_place` when the user is happy.

## Shape language

- Start from the silhouette. Block the whole object in 3 to 6 boxes, capture it, and only add detail once the outline reads. If it is unclear in silhouette, no colour will save it.
- Exaggerate: thicker legs, bigger heads on lamps, deeper roofs, chunky trims. Toy-like proportions read better on a phone than realistic ones.
- Vary sizes in steps. Pair big, medium and small pieces (roughly 1 : 0.5 : 0.2); avoid many parts of almost the same size.
- Keep thickness: nothing thinner than 0.2 studs unless it is a decal-like trim. Paper-thin walls flicker and vanish at a distance.
- Snap sizes and positions to a grid (0.25, 0.5 or 1 stud). Odd values like 3.137 make later pieces impossible to line up.

## Fake bevels and chamfers

Roblox parts have hard edges. Kenney's look comes from chamfered edges and soft corners, so fake them where the eye goes:

- Chamfer a top edge with a thin `WedgePart` along it, or by placing the box 0.2 lower and capping it with a slightly smaller box (a stepped edge reads as a bevel from afar).
- A darker trim strip (0.2 thick) along the base of a wall or the rim of a table does more than a real bevel.
- Round things are cylinders: a `Part` with `Shape = Enum.PartType.Cylinder` has its axis along its own X, so turn it with `CFrame.Angles(0, 0, math.rad(90))` to stand it up.
- Spend wedges only on hero pieces and roof lines; a bevel on every box multiplies the part count.

## Cutting and joining: unions

`execute_luau` has Roblox's boolean methods, and they behave as in Studio: the result is a new `UnionOperation` (an `IntersectOperation` for `IntersectAsync`), unparented, and the parts you passed stay where they were. Parent the result and destroy the parts yourself.

- `base:SubtractAsync({cutter, ...})` carves the cutters out of `base`: doorways, windows, arches, a bowl.
- `base:UnionAsync({part, ...})` merges parts into one solid that moves, colours and collides as one.
- `base:IntersectAsync({part, ...})` keeps only the overlap: a rounded tile from a block and a cylinder.

```lua
local wall = Instance.new("Part")
wall.Name = "Wall"
wall.Anchored = true
wall.Size = Vector3.new(12, 10, 1)
wall.CFrame = CFrame.new(0, 5, 0)
wall.Color = Color3.fromHex("#8f8577")
wall.Parent = workspace

local hole = Instance.new("Part")
hole.Size = Vector3.new(4, 7, 3) -- deeper than the wall so the cut goes all the way through
hole.CFrame = CFrame.new(0, 3.5, 0)
hole.Parent = workspace

local cut = wall:SubtractAsync({ hole })
cut.Name = "Wall_Door"
cut.Anchored = true
cut.Parent = workspace
wall:Destroy()
hole:Destroy()
print(cut.ClassName, cut.Size)
```

When to use them:

- A union is one draw call, but it has many triangles and its own collision mesh. Use one for a hero piece or a shape parts can't fake (an arch, a round window, a cut-away corner), not for every box.
- A wall that repeats down a whole street is cheaper framed: left, right and lintel parts, like `build-modular-kits` does.
- Make a cutter thicker than the part it cuts. A cutter that only touches a face leaves a paper-thin skin.
- Collision follows `CollisionFidelity`. Set `cut.CollisionFidelity = Enum.CollisionFidelity.PreciseConvexDecomposition` when players must walk through the opening; the default hulls can block a doorway.
- `UnionAsync` takes the base part's properties (colour, material) for the whole result. Faces keep their own part's colour only when `UsePartColor` is off.
- People do the same by hand from the Model tab: Union (Ctrl+Shift+G), Negate (Ctrl+Shift+N), Intersect (Ctrl+Shift+I) and Separate (Ctrl+Shift+U).

For anything organic or smoothly curved, model it in Blender and bring it in as a MeshPart (see `build-importing-assets`).

## Recipe: a low-poly lamp post

```lua
local function part(class, name, size, cframe, color, parent)
	local p = Instance.new(class)
	p.Name = name
	p.Anchored = true
	p.Size = size
	p.CFrame = cframe
	p.Color = Color3.fromHex(color)
	p.Material = Enum.Material.SmoothPlastic
	p.TopSurface = Enum.SurfaceType.Smooth
	p.BottomSurface = Enum.SurfaceType.Smooth
	p.Parent = parent
	return p
end

local origin = CFrame.new(0, 0, 0)
local model = Instance.new("Model")
model.Name = "LampPost"

local base = part("Part", "Base", Vector3.new(2, 0.6, 2), origin * CFrame.new(0, 0.3, 0), "#3d4451", model)
part("Part", "BaseTrim", Vector3.new(1.6, 0.3, 1.6), origin * CFrame.new(0, 0.75, 0), "#525c6b", model)
local pole = part("Part", "Pole", Vector3.new(9, 0.6, 0.6), origin * CFrame.new(0, 5.4, 0) * CFrame.Angles(0, 0, math.rad(90)), "#3d4451", model)
pole.Shape = Enum.PartType.Cylinder
part("Part", "Head", Vector3.new(1.8, 1.2, 1.8), origin * CFrame.new(0, 10.5, 0), "#3d4451", model)
local glow = part("Part", "Lamp", Vector3.new(1.4, 0.8, 1.4), origin * CFrame.new(0, 9.7, 0), "#ffe6a8", model)
glow.Material = Enum.Material.Neon
part("WedgePart", "CapFront", Vector3.new(1.8, 0.5, 0.9), origin * CFrame.new(0, 11.35, -0.45), "#525c6b", model)
part("WedgePart", "CapBack", Vector3.new(1.8, 0.5, 0.9), origin * CFrame.new(0, 11.35, 0.45) * CFrame.Angles(0, math.rad(180), 0), "#525c6b", model)

local light = Instance.new("PointLight")
light.Range = 16
light.Brightness = 1.5
light.Color = Color3.fromHex("#ffd9a0")
light.Parent = glow

model.PrimaryPart = base
model.WorldPivot = origin
model.Parent = workspace
print(model.Name, #model:GetChildren(), "parts")
```

A `WedgePart` slopes down towards its -Z face; turn it 180 degrees about Y to face the other way.

## Grouping, pivots and names

- Every object is a `Model`. Set `PrimaryPart` to its base (the part that touches the ground) and `WorldPivot` to the point you want to place it by: bottom centre for props, the kit corner for modular pieces.
- Place and move it with `model:PivotTo(cframe)`, never by moving parts one by one.
- Names in PascalCase, describing role, not shape: `LampPost`, `Pole`, `Head`, not `Part`, `Part2`. Variants get a suffix: `Crate_A`, `Crate_B`, `Tree_Pine_Tall`.
- Nest sub-assemblies (`Door`, `Window_A`) as Models inside the building Model so they can be swapped as a unit.
- Anchor everything static. Loose parts fall the moment Play starts.

Check a model's real size and its pivot before you place copies:

```lua
local model = workspace:FindFirstChildWhichIsA("Model")
if not model then
	print("no model in Workspace")
	return
end
local cframe, size = model:GetBoundingBox()
print(model.Name, "size", size, "centre", cframe.Position, "pivot", model:GetPivot().Position)
```

## Part budgets

Roblox publishes no hard part limit; these are working targets that keep a scene light on phones:

| Object | Parts |
| --- | --- |
| Small prop (crate, sign, bench) | 3 to 15 |
| Medium prop (lamp, tree, cart) | 8 to 30 |
| Hero prop (fountain, statue, vehicle) | 30 to 120 |
| Small building, outside only | 40 to 200 |
| Whole small level | a few thousand |

- A shape you repeat dozens of times with many parts is a MeshPart candidate: one mesh draws cheaper than 40 parts.
- Decoration that nobody stands on: `CanCollide = false`, `CanQuery = false`, `CanTouch = false`. Tiny details (< 1 stud): `CastShadow = false`.
- MeshParts used as decoration: `CollisionFidelity = Enum.CollisionFidelity.Box` keeps physics cheap. One mesh may have at most 20,000 triangles (https://create.roblox.com/docs/art/modeling/specifications).
- Pull `performance` before a large scene.

## Don'ts

- Default grey 4x1x2 bricks with studs: set the material and both surfaces to Smooth.
- Dozens of tiny parts for texture (bricks, planks one by one). Use a material or a MaterialVariant.
- Floating or intersecting-by-accident parts. Overlap deliberately, by at least 0.1 stud, or touch exactly.
- Realistic proportions in a stylized world, or the reverse; match the scale rules in `build-level-design`.
- Copying a Kenney model piece for piece. Learn the language, then make your own.

## Sources used

- https://kenney.nl/assets/category:3D
- https://create.roblox.com/docs/parts
- https://create.roblox.com/docs/studio/pivot-tools
- https://create.roblox.com/docs/art/modeling/specifications
- https://create.roblox.com/docs/reference/engine/classes/Model
