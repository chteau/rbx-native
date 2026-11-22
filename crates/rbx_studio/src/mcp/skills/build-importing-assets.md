---
name: build-importing-assets
description: Bringing free 3D kits such as Kenney's (CC0) into a place in this editor: which file to pick (GLB, glTF, OBJ, FBX), Model > Import 3D Model and drag-and-drop, what the importer does (Open Cloud upload, 20,000-triangle decimation, no rescale), scaling to Roblox metrics, textures, turning imports into a kit, Creator Store search and insert through MCP tools, and the licence note. Pull it before using any outside model.
---

# Importing assets

Kenney publishes hundreds of 3D kits for free under CC0 at https://kenney.nl/assets/category:3D (city, nature, platformer, dungeon, space, cars and more). They are a fast way to a clean, consistent look. Each kit ships every piece as its own model file; this skill gets them into a place and makes them behave like a native kit.

## Licence

- Kenney's assets are public domain (CC0): free for any project, commercial included. Credit is not required but appreciated, for example "3D models by Kenney (kenney.nl)" in the game description or credits screen. Do not use the Kenney logo on its own (https://kenney.nl/support).
- Other sources: check each asset's licence before use. CC-BY needs credit; "no redistribution" or unclear licences are a no.
- Never commit downloaded model files to a code repository unless the user asks; the place and Roblox's asset store hold them once imported.
- Kenney offers no help importing into specific engines, so the steps below are this editor's, not theirs.

## What the user does, what you do

MCP has no tool that reads a local model file, so the import itself is a menu action for the user. Tell them exactly what to click, then take over with tools.

1. The user downloads the kit from its page (for example https://kenney.nl/assets/platformer-kit) and unzips it.
2. Pick the file: GLB first (one self-contained file with its texture embedded), then glTF, then FBX, then OBJ. This editor uploads `.fbx`, `.gltf` and `.glb` as they are, inlines a `.gltf`'s side files, and rewrites `.obj` as glTF.
3. The user runs Model > Import 3D Model... (or drops the file on the editor window) for each piece needed. Model > Insert Mesh... places only the MeshParts, without a Model around a single one.
4. You find what arrived with `search_game_tree` (`path: "Workspace"`, `instance_type: "Model"`, `keywords` set to the file's name) and read it with `inspect_instance`.

What the importer does:

- Uploads the file through Open Cloud with the editor's API key, as Studio's own importer does, so the result is a real asset usable in-game. Without a key the import fails; the user adds one from Home > Manage key.
- A mesh over Roblox's 20,000-triangle budget is decimated to it first. Kenney pieces are far below it.
- Places the result in `Workspace` as a `Model` of `MeshPart`s named after the file, and the Output names the new asset id.
- Keeps the file's axes and units: 1 unit in the file becomes 1 stud. No rescale happens, so most kits arrive small.

## Scale to Roblox metrics

Measure first, then pick one scale for the whole kit. Kenney kits are authored on a small unit grid, so a scale of 4 to 8 is typical; derive yours from a door, a character or a floor tile so it matches `build-level-design` (doors 7 to 9 studs tall, tiles 4, 8 or 16 studs).

```lua
local NAME = "Imported"
local TARGET_HEIGHT = 8

local model = workspace:FindFirstChild(NAME)
if not model or not model:IsA("Model") then
	print("set NAME to the imported model's name")
	return
end

local _, size = model:GetBoundingBox()
local scale = TARGET_HEIGHT / size.Y
local pivot = model:GetPivot()
for _, part in ipairs(model:GetDescendants()) do
	if part:IsA("BasePart") then
		local offset = pivot:ToObjectSpace(part.CFrame)
		part.Size *= scale
		part.CFrame = pivot * (CFrame.new(offset.Position * scale) * offset.Rotation)
	end
end
print(string.format("%s scaled by %.2f to %.1f studs tall", NAME, scale, TARGET_HEIGHT))
```

Use the scale this prints for every other piece of the same kit (set `scale` directly instead of computing it), so pieces stay on one grid.

## Textures

Kenney's recent kits colour every model from one small shared palette texture; in a GLB it is embedded. Check a MeshPart with `inspect_instance`:

- `TextureID` set: done.
- `TextureID` empty and the part is plain grey: the user imports the kit's texture PNG with Model > Import Image..., the Output names its asset id, and you apply it to every MeshPart of the kit:

```lua
local TEXTURE = "rbxassetid://0"
local model = workspace:FindFirstChild("Imported")
if not model then
	print("no model named Imported")
	return
end
local count = 0
for _, part in ipairs(model:GetDescendants()) do
	if part:IsA("MeshPart") and part.TextureID == "" then
		part.TextureID = TEXTURE
		count += 1
	end
end
print("textured", count, "mesh parts")
```

- Prefer flat palette textures over detailed ones; they fit the materials rules in `build-color-and-materials`. A MeshPart's `Color` tints its texture, so leave it white when textured.

## Turn imports into a kit

Imported pieces behave like any Model. Make them a real modular kit (`build-modular-kits`):

1. Scale each piece by the kit's one scale.
2. Set the pivot rule: `model.WorldPivot` at the floor corner for tiles and walls, bottom centre for props.
3. Set `PrimaryPart`, anchor every part, and for decoration set `CanCollide`, `CanQuery` and `CanTouch` to false and `CollisionFidelity` to Box.
4. Rename to the kit's naming (`Floor_1x1`, `Tree_Pine_Tall`) and move it into `ServerStorage.Kit`.
5. Lay out the level by cloning from there.

```lua
local kit = game:GetService("ServerStorage"):FindFirstChild("Kit") or Instance.new("Folder")
kit.Name = "Kit"
kit.Parent = game:GetService("ServerStorage")

local model = workspace:FindFirstChild("Imported")
if not model or not model:IsA("Model") then
	print("no model named Imported")
	return
end
local cframe, size = model:GetBoundingBox()
model.WorldPivot = CFrame.new(cframe.Position - Vector3.new(size.X / 2, size.Y / 2, size.Z / 2))
for _, part in ipairs(model:GetDescendants()) do
	if part:IsA("BasePart") then
		part.Anchored = true
		model.PrimaryPart = model.PrimaryPart or part
	end
end
model.Name = "Floor_1x1"
model.Parent = kit
print("added", model.Name, "to the kit, footprint", size.X, "x", size.Z)
```

## Assets already on Roblox

- An asset id the user gives you: `insert_asset` with `assetId`, `parentPath` (for example `ServerStorage.Kit`) and optionally `assetName`.
- Searching the Creator Store: `search_asset` with `query` (for example "low poly tree"), `assetType` `Model` or `MeshPart`, and `maxResults`; prefer results from verified creators (`verifiedCreatorsOnly: true`) and check what you insert with `inspect_instance`, since free models can carry scripts. Delete any script you did not expect before saving.
- A model uploaded earlier from this editor (its id is in the Output) inserts the same way, without a second upload.

Generation tools listed for Studio compatibility (`generate_mesh`, `generate_procedural_model`, `generate_material`) answer that they cannot run here; build with parts (`build-modeling`) or import instead.

## Don'ts

- Mixing kits from different artists or at different scales in one area without unifying palette and grid.
- Importing a whole kit "just in case". Each import is an upload; take the pieces the level needs.
- Leaving imports in `Workspace` at their imported, tiny size.
- Free models with unknown scripts.
- Claiming CC0 for assets whose page says otherwise.

## Sources used

- https://kenney.nl/assets/category:3D
- https://kenney.nl/support
- https://kenney.nl/assets/platformer-kit
- https://kenney.nl/assets/nature-kit
- https://create.roblox.com/docs/art/modeling/specifications
- https://create.roblox.com/docs/cloud/guides/usage-assets
- https://create.roblox.com/docs/reference/engine/classes/MeshPart
