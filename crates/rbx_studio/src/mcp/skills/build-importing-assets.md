---
name: build-importing-assets
description: Bringing free CC0 3D kits (Poly Haven, Kenney) and local model files into a place in this editor with the import_model tool: which file to pick (GLB, glTF, OBJ, FBX), what the import does (Open Cloud upload, 20,000-triangle decimation, scaling to studs, base-centre pivot, credit attributes), Toolbox > Free kits, textures, turning imports into a kit, Creator Store search and insert through MCP tools, and the licence note. Pull it before using any outside model.
---

# Importing assets

Kenney publishes hundreds of 3D kits for free under CC0 at https://kenney.nl/assets/category:3D (city, nature, platformer, dungeon, space, cars and more). They are a fast way to a clean, consistent look. Each kit ships every piece as its own model file; this skill gets them into a place and makes them behave like a native kit.

## Licence

- Kenney's assets are public domain (CC0): free for any project, commercial included. Credit is not required but appreciated, for example "3D models by Kenney (kenney.nl)" in the game description or credits screen. Do not use the Kenney logo on its own (https://kenney.nl/support).
- Other sources: check each asset's licence before use. CC-BY needs credit; "no redistribution" or unclear licences are a no.
- Never commit downloaded model files to a code repository unless the user asks; the place and Roblox's asset store hold them once imported.
- Kenney offers no help importing into specific engines, so the steps below are this editor's, not theirs.

## Importing with `import_model`

`import_model` imports a model and makes it a kit piece in one call: it uploads the file through Open Cloud with the editor's API key (as Model > Import 3D Model does, so the result is a real asset usable in-game), then gathers it into one `Model` named after the asset, anchors every part, scales it to studs, puts its pivot on its base centre at `position`, and tags it `Assistant:<guid>`. The answer names the new Model's `fullPath`.

- A free kit model: `provider` and `id`.
  - `provider: "polyhaven"` with a Poly Haven model slug, the end of its page's address (https://polyhaven.com/a/ArmChair_01 is `id: "ArmChair_01"`). Every Poly Haven asset is CC0. A wrong id answers with close matches.
  - `provider: "local"` with `<pack>/<model>` for a pack the user added in Toolbox > Free kits (for example `kenney_platformer-kit/flag`).
- A file on disk: `path` to a `.glb`, `.gltf`, `.obj` or `.fbx`. Prefer GLB (one file), then glTF (its side files are read from beside it), then FBX, then OBJ (rewritten as glTF).
- `position: [x, y, z]` is where the base centre lands; `parentPath` (for example `ServerStorage.Kit`) is where it goes, Workspace by default.
- `scale` replaces the default studs per file unit. Defaults: Poly Haven and glTF files are in metres and become studs at 1 stud = 28 cm (x3.57); a Kenney kit is 4.5 studs per unit (its characters stand 1.1 units, so they come out a Roblox character's 5 studs), and 24 for Kenney's city kits (one tile, a road or a small house, becomes 24 studs); OBJ and FBX stay one unit to the stud.
- A kit model carries `Source` (its page), `License` (CC0) and `Author` attributes; a file from disk carries only `Source`, its file name, because its licence is unknown.
- Each call is one upload. Without a key it fails, and the user adds one from Home > Manage key.

Kenney's own site can't be downloaded from automatically, so for a Kenney kit the user downloads the zip from its page (for example https://kenney.nl/assets/platformer-kit) and adds it with Toolbox > Free kits > Add pack, or unzips it and you import files with `path`.

After importing, check the result with `inspect_instance` on the returned path, and keep one scale for every piece of the same kit.

## Rescaling

To match a size the default didn't hit, measure, then use `Model:ScaleTo`, which scales around the pivot relative to the imported size, so calling it again replaces the scale instead of compounding it:

```lua
local NAME = "Imported"
local TARGET_HEIGHT = 8

local model = workspace:FindFirstChild(NAME)
if not model or not model:IsA("Model") then
	print("set NAME to the imported model's name")
	return
end

local _, size = model:GetBoundingBox()
local scale = model:GetScale() * TARGET_HEIGHT / size.Y
model:ScaleTo(scale)
print(string.format("%s scaled to %.2f, %.1f studs tall", NAME, scale, TARGET_HEIGHT))
```

Use the same scale for every other piece of the kit (`model:ScaleTo(scale)`), so pieces stay on one grid that matches `build-level-design` (doors 7 to 9 studs tall, tiles 4, 8 or 16 studs).

## Textures

Kenney's recent kits colour every model from one small shared palette texture beside the GLBs; `import_model` and the Toolbox embed it in the upload. Check a MeshPart with `inspect_instance`:

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

1. Keep the kit's one scale on every piece.
2. Set the pivot rule: imports already pivot at the bottom centre, right for props; move `model.WorldPivot` to the floor corner for tiles and walls.
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
- Mixing scales: an import scaled by hand next to one left at the kit default.
- Free models with unknown scripts.
- Claiming CC0 for assets whose page says otherwise.

## Sources used

- https://kenney.nl/assets/category:3D
- https://polyhaven.com/license
- https://kenney.nl/support
- https://kenney.nl/assets/platformer-kit
- https://kenney.nl/assets/nature-kit
- https://create.roblox.com/docs/art/modeling/specifications
- https://create.roblox.com/docs/cloud/guides/usage-assets
- https://create.roblox.com/docs/reference/engine/classes/MeshPart
