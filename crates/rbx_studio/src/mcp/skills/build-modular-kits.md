---
name: build-modular-kits
description: Kenney-style modular building kits in Roblox: picking one grid and tile size in studs, consistent corner pivots, piece families (floor, wall, corner, door, stairs, roof), connectors, variants, a template folder, and laying out a level by cloning kit pieces on the grid with Luau. Pull it before building anything made of repeated pieces (dungeons, cities, platformer courses, interiors).
---

# Modular kits

A kit is a small set of pieces that all share one grid, one pivot rule and one palette, so any piece snaps to any other. Kenney's kits are a good model of the structure: one kit per theme (City Kit Roads, Commercial and Industrial, Modular Dungeon Kit, Modular Space Kit, Platformer Kit, Nature Kit), each a family of named pieces with a few variants (https://kenney.nl/assets/category:3D). Learn the structure there; build your own pieces.

## Pick the grid first

| Kit | Base tile | Wall height | Why |
| --- | --- | --- | --- |
| Platformer, obby | 4 x 4 studs | 1, 2 or 4 (blocks) | matches the jump metrics in `build-level-design` |
| Dungeon, interior | 8 x 8 studs | 12 studs | doors 6 to 8 wide fit, the camera has room |
| City block | 16 x 16 studs road tile | 12 studs per storey | two 8-stud lanes per road tile |
| Nature scatter | none, free placement | n/a | snap only the paths and the ground tiles |

- Every piece's footprint is a whole number of base tiles, and its largest dimension divides by the smallest one. Roblox's own environment-art tutorial does the same with 5-stud increments: its tallest and widest piece is 15 x 5 (https://create.roblox.com/docs/tutorials/curriculums/environmental-art/greybox-your-environment).
- Wall thickness is a fixed fraction of the tile (1 stud on an 8 grid) and walls sit centred on the tile edge, so two rooms share one wall without z-fighting.
- Write the grid at the top of every build script as constants. Never type a raw position.

## Pivot rule

Pick one rule and keep it for every piece in the kit:

- Floor and wall pieces: pivot at the floor-level corner, on the tile grid. Rotating by 90 degrees about that corner keeps the piece on the grid. Roblox's tutorial uses the forward-most lower corner the same way.
- Props that sit on tiles (crates, lamps): pivot at bottom centre.
- Set the pivot with `Model.WorldPivot` (or `PivotOffset` on a single part), then only ever place with `PivotTo`.

## Piece families

A usable kit for a dungeon or interior, all on the 8-stud grid:

| Family | Pieces | Variants worth making |
| --- | --- | --- |
| Floor | `Floor_1x1`, `Floor_2x2` | plain, cracked, with drain |
| Wall | `Wall_Straight`, `Wall_Half` | plain, window, banner |
| Corner | `Wall_CornerOuter`, `Wall_CornerInner` | none |
| Opening | `Wall_Door`, `Wall_Arch` | open, barred |
| Vertical | `Stairs_1x2`, `Ramp_1x2`, `Pillar` | none |
| Roof / ceiling | `Roof_Flat`, `Roof_Slope`, `Roof_Ridge`, `Roof_Corner` | none |
| Trim | `Trim_Base`, `Trim_Top` | covers seams between pieces |

- Make the plain piece first, then a variant by cloning it and swapping one sub-model. Variants break repetition; 1 variant in 4 to 6 copies is plenty.
- Connectors (pillars, trims, door frames) hide seams where pieces meet, exactly where the eye catches gaps.
- Keep each piece to the budgets in `build-modeling`.

## Template folder

Keep the kit outside the level, in `ServerStorage.Kit` (templates are not shown and not replicated), and clone from there. Build one piece per `execute_luau` call (`datamodel_type: "Edit"`):

```lua
local TILE = 8
local WALL = 12
local THICK = 1

local storage = game:GetService("ServerStorage")
local kit = storage:FindFirstChild("Kit") or Instance.new("Folder")
kit.Name = "Kit"
kit.Parent = storage

local function block(name, size, offset, color, parent)
	local p = Instance.new("Part")
	p.Name = name
	p.Anchored = true
	p.Size = size
	p.CFrame = CFrame.new(offset)
	p.Color = Color3.fromHex(color)
	p.Material = Enum.Material.SmoothPlastic
	p.TopSurface = Enum.SurfaceType.Smooth
	p.BottomSurface = Enum.SurfaceType.Smooth
	p.Parent = parent
	return p
end

-- Floor tile: pivot at its -X -Z corner on the floor plane.
local floor = Instance.new("Model")
floor.Name = "Floor_1x1"
floor.PrimaryPart = block("Slab", Vector3.new(TILE, 1, TILE), Vector3.new(TILE / 2, -0.5, TILE / 2), "#b9a98f", floor)
floor.WorldPivot = CFrame.new(0, 0, 0)
floor.Parent = kit

-- Straight wall along +X on the tile edge, centred on it.
local wall = Instance.new("Model")
wall.Name = "Wall_Straight"
wall.PrimaryPart = block("Wall", Vector3.new(TILE, WALL, THICK), Vector3.new(TILE / 2, WALL / 2, 0), "#8f8577", wall)
block("Trim_Base", Vector3.new(TILE, 1, THICK + 0.4), Vector3.new(TILE / 2, 0.5, 0), "#5e564d", wall)
wall.WorldPivot = CFrame.new(0, 0, 0)
wall.Parent = kit

-- Door wall: a framed opening. Cheaper than a SubtractAsync union for a piece the level repeats.
local door = Instance.new("Model")
door.Name = "Wall_Door"
local side = (TILE - 6) / 2
door.PrimaryPart = block("Left", Vector3.new(side, WALL, THICK), Vector3.new(side / 2, WALL / 2, 0), "#8f8577", door)
block("Right", Vector3.new(side, WALL, THICK), Vector3.new(TILE - side / 2, WALL / 2, 0), "#8f8577", door)
block("Lintel", Vector3.new(6, WALL - 9, THICK), Vector3.new(TILE / 2, 9 + (WALL - 9) / 2, 0), "#8f8577", door)
door.WorldPivot = CFrame.new(0, 0, 0)
door.Parent = kit

print("kit pieces:", #kit:GetChildren())
```

The door opening is 6 wide and 9 tall, comfortable for a 5-stud character (`build-level-design`).

## Laying out a level from the kit

Describe the level as data (a grid of characters or a list of placements) and let one script place it. Rerunning the script rebuilds the level, so iteration is cheap:

```lua
local TILE = 8
local kit = game:GetService("ServerStorage"):FindFirstChild("Kit")
if not kit then
	print("build the kit first")
	return
end

local plan = {
	"#####",
	"#...#",
	"#....",
	"#####",
}

local level = workspace:FindFirstChild("Level")
if level then
	level:Destroy()
end
level = Instance.new("Model")
level.Name = "Level"
level.Parent = workspace

local function place(name, x, z, turns)
	local template = kit:FindFirstChild(name)
	if not template then
		return
	end
	local copy = template:Clone()
	copy:PivotTo(CFrame.new(x * TILE, 0, z * TILE) * CFrame.Angles(0, math.rad(90 * turns), 0))
	copy.Parent = level
end

for z, row in ipairs(plan) do
	for x = 1, #row do
		if row:sub(x, x) ~= "#" then
			place("Floor_1x1", x, z, 0)
			local function open(dx, dz)
				local r = plan[z + dz]
				return r ~= nil and r:sub(x + dx, x + dx) ~= "" and r:sub(x + dx, x + dx) ~= "#"
			end
			if not open(0, -1) then
				place("Wall_Straight", x, z, 0)
			end
			if not open(-1, 0) then
				place("Wall_Straight", x, z + 1, 1)
			end
		end
	end
end
print("placed", #level:GetChildren(), "pieces")
```

Rotation by `turns` quarter turns about the corner pivot keeps every piece on the grid. Add the east and south walls the same way, then doors where the plan says so.

After placing:

1. `screen_capture` from high above the level centre (`camera_position: [x, 80, z + 40]`, `look_at_position: [x, 0, z]`) to check the plan reads.
2. Walk it: `start_stop_play` with `is_start: true`, then `character_navigation` to a far tile, `screen_capture`, and `start_stop_play` with `is_start: false`.
3. Fix seams with trims, swap 1 in 5 pieces to a variant, add props off-grid for life.

## Don'ts

- Two grids in one level (a 5-stud kit next to an 8-stud kit). Pick one and convert.
- Pivots in the middle of some pieces and at the corner of others.
- Walls placed by hand with raw numbers. Always go through the grid constants.
- A level that is only kit pieces in straight lines. Break it with height changes, landmarks and props (`build-level-design`).
- Shipping someone else's kit pieces in the repo or the place without checking the licence (`build-importing-assets`).

## Sources used

- https://kenney.nl/assets/category:3D
- https://kenney.nl/assets/modular-dungeon-kit
- https://kenney.nl/assets/city-kit-roads
- https://kenney.nl/assets/platformer-kit
- https://create.roblox.com/docs/tutorials/curriculums/environmental-art/greybox-your-environment
- https://create.roblox.com/docs/reference/engine/classes/PVInstance
