---
name: build-color-and-materials
description: Colour and material choices for good-looking Roblox builds in a clean low-poly style: small fixed palettes with exact Color3 hex values (meadow, town, dungeon, sci-fi, candy), value and saturation rules, SmoothPlastic versus textured materials, Neon and Glass use, MaterialVariant setup with StudsPerTile, and a Luau pass that snaps stray colours back to the palette. Pull it before colouring any build.
---

# Colour and materials

Kenney's 3D kits look clean because every model draws from one small palette of flat colours, with no texture noise (https://kenney.nl/assets/category:3D). Do the same: pick a palette before the first part, write it as constants, and never type a colour that is not in it.

## Rules

- 5 to 8 colours for a scene, plus 1 accent. Each colour gets a light and a dark step for trims and shading, never more.
- Big surfaces (ground, walls, roofs) take the calm, less saturated colours. Saturation goes on small things the player should notice: pickups, doors, the path, the goal.
- Value contrast carries readability: walkable surfaces lighter than walls, or the reverse, but consistently. Check by imagining the capture in greyscale.
- No pure black (#000000) or pure white (#ffffff) on surfaces; they clip under lighting. Darkest about #2b2d38, lightest about #f4f1e8.
- Lighting tints everything. Pick colours under the lighting preset you will ship (`build-lighting-and-atmosphere`), then re-check with `screen_capture`.

## Palettes

Original palettes for this editor's skills, tuned for Roblox's default Soft lighting. Copy one, then shift its hue to give the game its own identity.

| Role | Meadow | Town | Dungeon | Sci-fi | Candy |
| --- | --- | --- | --- | --- | --- |
| Ground | `#7cbf5a` | `#9aa3a8` | `#5a5450` | `#3b4252` | `#ffd6e8` |
| Ground dark | `#5e9a45` | `#7b8388` | `#433e3b` | `#2c3240` | `#f5b3d0` |
| Path | `#e3c98f` | `#d9d2c3` | `#8a7f72` | `#5f6b80` | `#fff3b0` |
| Wall | `#f2e6cf` | `#e8dccb` | `#8f8577` | `#d6dbe4` | `#b8e3ff` |
| Wall dark / trim | `#c9b394` | `#b5a68f` | `#5e564d` | `#9aa3b3` | `#8cc8f0` |
| Roof / feature | `#d9624a` | `#4f6d8f` | `#6b4f3a` | `#4fb3bf` | `#c58cf0` |
| Wood / metal | `#a06a42` | `#8a5a3c` | `#7a5a3a` | `#7d8796` | `#ff9fb2` |
| Foliage / detail | `#4e9a50` | `#5e9e5a` | `#3f6b4a` | `#2f3a4a` | `#7ed9a6` |
| Accent | `#ffcf3f` | `#ff7a45` | `#ff9b3d` | `#ff4fa0` | `#ff6b6b` |

Apply as constants in every build script:

```lua
local PALETTE = {
	ground = Color3.fromHex("#7cbf5a"),
	groundDark = Color3.fromHex("#5e9a45"),
	path = Color3.fromHex("#e3c98f"),
	wall = Color3.fromHex("#f2e6cf"),
	trim = Color3.fromHex("#c9b394"),
	roof = Color3.fromHex("#d9624a"),
	wood = Color3.fromHex("#a06a42"),
	foliage = Color3.fromHex("#4e9a50"),
	accent = Color3.fromHex("#ffcf3f"),
}

local sign = Instance.new("Part")
sign.Name = "Sign"
sign.Anchored = true
sign.Size = Vector3.new(4, 2, 0.4)
sign.CFrame = CFrame.new(0, 4, 0)
sign.Color = PALETTE.accent
sign.Material = Enum.Material.SmoothPlastic
sign.Parent = workspace
print(sign.Name, sign.Color)
```

## Materials

| Look | Material | Notes |
| --- | --- | --- |
| Clean low-poly (default) | `SmoothPlastic` | flat colour, no pattern; the Kenney look |
| Slightly soft toy | `Plastic` | faint grain, set both surfaces to Smooth to drop the studs |
| Natural ground at scale | `Grass`, `Sand`, `Ground`, `Rock`, `Snow` | only on large surfaces; colour still from the palette |
| Wood structures | `Wood`, `WoodPlanks` | grain direction follows the part; keep planks on few large parts |
| Stone / brick walls | `Slate`, `Cobblestone`, `Brick`, `Concrete` | pick one per building |
| Metal props | `Metal`, `DiamondPlate`, `CorrodedMetal` | small parts only, they get noisy fast |
| Light sources, signs | `Neon` | glows with bloom; 1 to 3 % of the screen at most |
| Windows, water surface | `Glass` with `Transparency` 0.3 to 0.6 | expensive in bulk; use few large panes |

- Stick to one family per scene: all SmoothPlastic with one or two textured ground materials is the safest stylized result. Mixing six textured materials is the fastest way to visual noise.
- A textured material tiles at a fixed size. On small parts its pattern looks huge; scale it with a MaterialVariant instead of tolerating it.
- `Neon` ignores lighting, so it reads as a light. Do not use it as a "bright colour".

## MaterialVariant

A MaterialVariant in `MaterialService` changes how a base material looks wherever a part names it. Use one to calm a pattern down or to use your own texture:

```lua
local service = game:GetService("MaterialService")
local variant = service:FindFirstChild("SoftPlanks") or Instance.new("MaterialVariant")
variant.Name = "SoftPlanks"
variant.BaseMaterial = Enum.Material.WoodPlanks
variant.StudsPerTile = 8
variant.MaterialPattern = Enum.MaterialPattern.Regular
variant.Parent = service

local deck = Instance.new("Part")
deck.Name = "Deck"
deck.Anchored = true
deck.Size = Vector3.new(16, 1, 16)
deck.CFrame = CFrame.new(0, 0.5, 0)
deck.Material = Enum.Material.WoodPlanks
deck.MaterialVariant = "SoftPlanks"
deck.Color = Color3.fromHex("#a06a42")
deck.Parent = workspace
print(deck.MaterialVariant, variant.StudsPerTile)
```

- `StudsPerTile` sets how many studs one repeat of the texture covers. Bigger values give a calmer surface.
- `MaterialPattern = Organic` breaks visible tiling on ground; `Regular` keeps planks and bricks aligned.
- Own textures go in `ColorMap`, `NormalMap`, `RoughnessMap` and `MetalnessMap` as `rbxassetid://<id>`. Upload a local PNG with Model > Import Image... (the Output names the asset id) or a web image with `upload_image` (`imagePaths`: its URL). Keep maps flat and low-contrast for a stylized look.

## Snap stray colours to the palette

After a big build, find colours that drifted outside the palette and snap them to the nearest entry:

```lua
local PALETTE = {
	"#7cbf5a", "#5e9a45", "#e3c98f", "#f2e6cf", "#c9b394",
	"#d9624a", "#a06a42", "#4e9a50", "#ffcf3f",
}
local colors = {}
for _, hex in ipairs(PALETTE) do
	table.insert(colors, Color3.fromHex(hex))
end

local function distance(a, b)
	return (a.R - b.R) ^ 2 + (a.G - b.G) ^ 2 + (a.B - b.B) ^ 2
end

local changed = 0
for _, item in ipairs(workspace:GetDescendants()) do
	if item:IsA("BasePart") and item.Name ~= "Terrain" and item.Material ~= Enum.Material.Neon then
		local best, bestDistance = item.Color, math.huge
		for _, color in ipairs(colors) do
			local d = distance(item.Color, color)
			if d < bestDistance then
				best, bestDistance = color, d
			end
		end
		if bestDistance > 0.0001 then
			item.Color = best
			changed += 1
		end
	end
end
print("snapped", changed, "parts to the palette")
```

Run it on a sub-model instead of `workspace` when only one area should change.

## Don'ts

- Rainbow builds: every part its own colour.
- BrickColor names picked at random; use `Color3.fromHex` from the palette.
- Default medium stone grey everywhere. It reads as unfinished.
- Textured material on every part, or a strong normal map on a toy-like build.
- Neon used as a colour, or more than a few glowing parts in view.

## Sources used

- https://kenney.nl/assets/category:3D
- https://create.roblox.com/docs/parts/materials
- https://create.roblox.com/docs/reference/engine/classes/MaterialVariant
- https://create.roblox.com/docs/reference/engine/classes/MaterialService
- https://create.roblox.com/docs/reference/engine/enums/Material
