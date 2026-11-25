---
name: build-lighting-and-atmosphere
description: Lighting presets for Roblox builds with exact property values: Lighting (LightingStyle, ClockTime, Brightness, ambients, exposure, shadows), Atmosphere, Sky, Bloom, ColorCorrection, SunRays and DepthOfField for stylized day, golden sunset, night and interior moods, plus local lights and a capture check. Pull it before setting or changing the look of a scene.
---

# Lighting and atmosphere

Lighting does more for a build than any part. Pick one mood, apply its preset in one `execute_luau` call (`datamodel_type: "Edit"`), then check it with `screen_capture` from the player's eye height and from above. Colours were chosen under one light; re-check them after a change (`build-color-and-materials`).

## Baseline

Studio's own templates load with these values (measured in real Studio; this editor's Home templates use the same): `LightingStyle` Soft, `PrioritizeLightingQuality` true, `Ambient` and `OutdoorAmbient` (70, 70, 70), `Brightness` 3, `TimeOfDay` 14:30:00, `ShadowSoftness` 0.2, `EnvironmentDiffuseScale` and `EnvironmentSpecularScale` 1, `GlobalShadows` true; an `Atmosphere` with `Density` 0.3, `Offset` 0.25, `Color` (199, 199, 199), `Decay` (106, 112, 125), `Glare` 0, `Haze` 0; `Bloom` 1 / 24 / 2 (Intensity, Size, Threshold); `SunRays` 0.01 / 0.1.

- `LightingStyle` replaces the deprecated `Technology`: Soft is the stylized look, Realistic the naturalistic one (https://create.roblox.com/docs/reference/engine/classes/Lighting). Low-poly builds want Soft.
- `ClockTime` is the hour as a number (14.5 is 14:30). `GeographicLatitude` tilts the sun's path; around 40 gives long, angled shadows all day.

The presets below are starting points written for these skills, not Roblox numbers. Tune by eye after a capture.

## Preset helper

Every preset runs through the same helper, which replaces the previous mood's effects so presets never stack:

```lua
local Lighting = game:GetService("Lighting")

local function rgb(r, g, b)
	return Color3.fromRGB(r, g, b)
end

local function effect(class, name, props)
	local item = Lighting:FindFirstChild(name)
	if item and item.ClassName ~= class then
		item:Destroy()
		item = nil
	end
	item = item or Instance.new(class)
	item.Name = name
	for key, value in pairs(props) do
		item[key] = value
	end
	item.Parent = Lighting
	return item
end

local function apply(preset)
	for key, value in pairs(preset.lighting) do
		Lighting[key] = value
	end
	effect("Atmosphere", "Atmosphere", preset.atmosphere)
	effect("Sky", "Sky", preset.sky)
	effect("BloomEffect", "Bloom", preset.bloom)
	effect("ColorCorrectionEffect", "ColorCorrection", preset.color)
	effect("SunRaysEffect", "SunRays", preset.rays)
	effect("DepthOfFieldEffect", "DepthOfField", preset.depth)
end

-- Stylized day: warm, bright, soft shadows, clean colours.
apply({
	lighting = {
		LightingStyle = Enum.LightingStyle.Soft,
		PrioritizeLightingQuality = true,
		ClockTime = 14.5,
		GeographicLatitude = 35,
		Brightness = 3,
		Ambient = rgb(70, 70, 80),
		OutdoorAmbient = rgb(128, 128, 140),
		ExposureCompensation = 0,
		EnvironmentDiffuseScale = 1,
		EnvironmentSpecularScale = 0.5,
		GlobalShadows = true,
		ShadowSoftness = 0.3,
		FogEnd = 100000,
	},
	atmosphere = { Density = 0.25, Offset = 0.2, Color = rgb(199, 214, 230), Decay = rgb(106, 140, 180), Glare = 0, Haze = 0.5 },
	sky = { CelestialBodiesShown = true, StarCount = 3000, SunAngularSize = 14, MoonAngularSize = 11 },
	bloom = { Enabled = true, Intensity = 0.6, Size = 24, Threshold = 1.6 },
	color = { Enabled = true, Brightness = 0.02, Contrast = 0.08, Saturation = 0.15, TintColor = rgb(255, 250, 242) },
	rays = { Enabled = true, Intensity = 0.04, Spread = 0.6 },
	depth = { Enabled = false },
})
print("applied day preset", Lighting.ClockTime)
```

For the other moods, keep the helper and swap the table passed to `apply`.

## Golden sunset

```lua
local Lighting = game:GetService("Lighting")
local function rgb(r, g, b)
	return Color3.fromRGB(r, g, b)
end
local settings = {
	ClockTime = 17.6,
	GeographicLatitude = 40,
	Brightness = 2.2,
	Ambient = rgb(80, 62, 70),
	OutdoorAmbient = rgb(150, 110, 100),
	ExposureCompensation = 0.1,
	ShadowSoftness = 0.25,
}
for key, value in pairs(settings) do
	Lighting[key] = value
end
local atmosphere = Lighting:FindFirstChildOfClass("Atmosphere") or Instance.new("Atmosphere")
atmosphere.Density = 0.35
atmosphere.Offset = 0.3
atmosphere.Color = rgb(255, 196, 150)
atmosphere.Decay = rgb(140, 90, 110)
atmosphere.Glare = 0.6
atmosphere.Haze = 1.6
atmosphere.Parent = Lighting
local grade = Lighting:FindFirstChildOfClass("ColorCorrectionEffect") or Instance.new("ColorCorrectionEffect")
grade.Contrast = 0.1
grade.Saturation = 0.2
grade.TintColor = rgb(255, 236, 214)
grade.Parent = Lighting
print("sunset", Lighting.ClockTime)
```

Pair with `SunRays` Intensity 0.08, Spread 0.8 and `Bloom` Intensity 0.8, Threshold 1.4. Turn the main view so the sun sits low behind the landmark.

## Night

Values for the helper's `apply`:

| Object | Properties |
| --- | --- |
| Lighting | `ClockTime` 0, `Brightness` 1, `Ambient` (36, 40, 62), `OutdoorAmbient` (62, 72, 110), `ExposureCompensation` 0.3, `ShadowSoftness` 0.4, `EnvironmentSpecularScale` 0.3 |
| Atmosphere | `Density` 0.4, `Offset` 0.1, `Color` (90, 104, 150), `Decay` (34, 40, 66), `Glare` 0, `Haze` 0.4 |
| Sky | `StarCount` 3000, `MoonAngularSize` 14, `CelestialBodiesShown` true |
| Bloom | `Intensity` 1, `Size` 28, `Threshold` 1.2 (lamps glow) |
| ColorCorrection | `Contrast` 0.12, `Saturation` -0.05, `TintColor` (220, 230, 255) |
| SunRays | `Enabled` false |

Night is only good with local lights: warm `PointLight`s in lamps and windows against the cool ambient. Never make night a dark screen; the player must still read the path.

## Interior

| Object | Properties |
| --- | --- |
| Lighting | `ClockTime` 12, `Brightness` 2, `Ambient` (92, 86, 80), `OutdoorAmbient` (110, 110, 120), `ExposureCompensation` 0.2, `EnvironmentDiffuseScale` 1, `EnvironmentSpecularScale` 0.4, `ShadowSoftness` 0.2 |
| Atmosphere | `Density` 0.15, `Offset` 0, `Haze` 0, `Glare` 0 |
| Bloom | `Intensity` 0.5, `Size` 20, `Threshold` 1.8 |
| ColorCorrection | `Contrast` 0.06, `Saturation` 0.08, `TintColor` (255, 246, 236) |
| DepthOfField | `Enabled` false (it blurs small rooms) |

`Ambient` lights what the sky cannot reach, so it decides how dark a closed room is; raise it before adding more lamps.

## Local lights

```lua
local lamp = Instance.new("Part")
lamp.Name = "CeilingLamp"
lamp.Anchored = true
lamp.Size = Vector3.new(1.5, 0.5, 1.5)
lamp.CFrame = CFrame.new(0, 11.5, 0)
lamp.Material = Enum.Material.Neon
lamp.Color = Color3.fromRGB(255, 226, 180)
lamp.CanCollide = false
lamp.Parent = workspace

local light = Instance.new("PointLight")
light.Brightness = 1.5
light.Range = 18
light.Color = Color3.fromRGB(255, 214, 170)
light.Shadows = true
light.Parent = lamp
print(lamp.Name, light.Range)
```

- `PointLight` for lamps and torches (Range 12 to 24, Brightness 1 to 2). `SpotLight` (Angle 45 to 90, Face pointing down) for street lamps and stage lights. `SurfaceLight` for screens and windows.
- Warm lights (around 2700 K, `#ffd6aa`) against a cool ambient read as cosy; cool lights against warm ambient read as sci-fi.
- Shadows on a few key lights only. Every shadowed light costs, more so on phones (`performance`).

## Check it

1. `screen_capture` with `camera_position: [0, 6, 30]`, `look_at_position: [0, 4, 0]` (eye height) and one from high above.
2. Is the path the brightest readable thing? Are the darkest areas still not black? Does the accent colour still pop?
3. Change one value at a time and capture again.

## Don'ts

- Stacking a new `Atmosphere`, `Sky` or effect next to an old one. One of each, replaced in place.
- `ExposureCompensation` above 1 or `Bloom` Threshold below 1: everything glows and colour washes out.
- Heavy `DepthOfField` during gameplay. Keep it for menus and cutscenes.
- `Brightness` 0 for night. Lower it, cool the ambients, add warm local lights instead.
- Changing lighting without re-checking the palette.

## Sources used

- https://create.roblox.com/docs/environment/lighting
- https://create.roblox.com/docs/environment/atmosphere
- https://create.roblox.com/docs/environment/post-processing-effects
- https://create.roblox.com/docs/effects/light-sources
- https://create.roblox.com/docs/reference/engine/classes/Lighting
- https://create.roblox.com/docs/tutorials/curriculums/environmental-art/construct-your-world
