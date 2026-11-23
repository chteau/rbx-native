---
name: build-level-design
description: Level design for Roblox worlds: greyboxing first, scale against a 5-stud character, real movement numbers (WalkSpeed 16, JumpHeight 7.2, gravity 196.2) turned into comfortable jump gaps, ledge heights, stairs, ramps, doors and corridors, plus landmarks, paths, sightlines, readability and a playtest loop through MCP tools. Pull it before laying out any playable space.
---

# Level design

Layout first, art last. A level that plays well in grey blocks will look good once dressed; a beautiful level with bad metrics never plays well.

## Greybox first

1. Block the whole playable space in plain parts on a grid (Roblox's tutorials snap greybox geometry to 5-stud increments; see https://create.roblox.com/docs/tutorials/curriculums/environmental-art/greybox-your-environment). One flat colour per role: floor, wall, hazard, goal.
2. Put every greybox part in one Model, `Greybox`, so it can be hidden or replaced as a unit.
3. Playtest it (loop below) before any detail.
4. Replace blocks with kit pieces (`build-modular-kits`) and props (`build-modeling`), keeping the same footprint.

```lua
local ROLE = {
	floor = Color3.fromHex("#b8bcc4"),
	wall = Color3.fromHex("#8a8f99"),
	hazard = Color3.fromHex("#e0564a"),
	goal = Color3.fromHex("#ffcf3f"),
}

local greybox = workspace:FindFirstChild("Greybox") or Instance.new("Model")
greybox.Name = "Greybox"
greybox.Parent = workspace

local function block(name, role, size, position)
	local p = Instance.new("Part")
	p.Name = name
	p.Anchored = true
	p.Size = size
	p.Position = position
	p.Color = ROLE[role]
	p.Material = Enum.Material.SmoothPlastic
	p.TopSurface = Enum.SurfaceType.Smooth
	p.BottomSurface = Enum.SurfaceType.Smooth
	p.Parent = greybox
	return p
end

-- Start pad, three jumps with growing gaps, a goal platform.
block("Start", "floor", Vector3.new(20, 2, 20), Vector3.new(0, 1, 0))
local z = 10
for i, gap in ipairs({ 5, 6, 7 }) do
	z += gap + 3
	block("Hop" .. i, "floor", Vector3.new(6, 2, 6), Vector3.new(0, 1 + i, z))
	z += 3
end
block("Goal", "goal", Vector3.new(12, 2, 12), Vector3.new(0, 5, z + 5 + 6))
print("greybox parts:", #greybox:GetChildren())
```

## Scale and metrics

Character and movement defaults, from Roblox's docs:

| Fact | Value | Source |
| --- | --- | --- |
| Stud | about 28 cm | greybox-a-playable-area tutorial |
| Character height | about 5 studs (R6 is 5; R15 default proportions land close) | rig guides |
| Walk speed | 16 studs/s (`StarterPlayer.CharacterWalkSpeed`) | Humanoid.WalkSpeed |
| Jump height | 7.2 studs (`StarterPlayer.CharacterJumpHeight`) | Humanoid.JumpHeight |
| Jump power | 50 (`CharacterJumpPower`, when `UseJumpPower` is on) | Humanoid.JumpPower |
| Gravity | 196.2 studs/s² (`Workspace.Gravity`) | Workspace.Gravity |
| Max slope | 89 degrees by default (`Humanoid.MaxSlopeAngle`) | Humanoid.MaxSlopeAngle |

Derived from those defaults (rule of thumb, confirm in Play):

- Airtime on flat ground is about 2 x sqrt(2 x 7.2 / 196.2), roughly 0.54 s, so a running jump covers about 8.7 studs at most.
- Comfortable gaps: 4 to 6 studs for anyone, 7 to 8 only as a deliberate challenge. Never require the theoretical maximum.
- Ledges: up to 5 studs is a casual jump, 6 to 7 a precise one; above 7.2 needs a step, a ladder or a jump pad.
- Stairs: risers of about 1 stud with treads of 1.5 to 2 studs walk without jumping. A `WedgePart` ramp is smoother and cheaper than many step parts; keep walkable ramps under about 40 degrees so they read as walkable even though the engine allows steeper.
- Doors: 4 to 6 studs wide and 7 to 9 tall for one character; Roblox's sample keeps doorways and hallways at least 10 studs wide and walls at least 10 tall so two players pass and the camera fits (https://create.roblox.com/docs/tutorials/curriculums/core/building/greybox-a-playable-area).
- Corridors: 10 studs minimum, 14 to 20 for main routes. Rooms under 20 x 20 feel cramped with the default third-person camera.
- Ceilings: 12 studs minimum indoors, or the camera clips into them.
- Rails and low walls: 3 to 3.5 studs, just under the character's shoulder.

Check movement settings before tuning gaps, since a game may change them:

```lua
local StarterPlayer = game:GetService("StarterPlayer")
print("walk", StarterPlayer.CharacterWalkSpeed, "jump height", StarterPlayer.CharacterJumpHeight, "gravity", workspace.Gravity)
```

## Structure and readability

- One main path, readable from the start: lighter or more saturated than its surroundings, wider than side paths, lit better. The player should always know where to go next.
- Landmarks: one tall, unique silhouette per area (a tower, a giant tree, a lighthouse) visible from far away. Players navigate by them. Kenney's kits always include a few such hero pieces next to the repeatable ones.
- Frame the goal: put the landmark or the goal at the end of a sightline, between two walls or under an arch.
- Rhythm: tight space, then an open view; challenge, then a rest platform; repeat with rising difficulty.
- Height: change elevation every few areas. A flat level reads as a floor plan.
- Landing zones: make platforms at least 6 x 6 studs for casual jumps, bigger after long jumps and on mobile.
- Hazards look like hazards: one colour (red, orange) or material (Neon, CrackedLava) used only for them.
- Spawn: on a calm, open pad facing the main path, never facing a wall.
- Bounds: invisible walls only where a visible edge (fence, cliff, water) explains them.

## Playtest loop through MCP tools

1. `start_stop_play` with `is_start: true`.
2. `character_navigation` with `x`, `y`, `z` of a landing zone (or `instance_path` of a part, for example `Workspace.Greybox.Hop2`). The answer says whether it got there; a path that fails is a layout bug.
3. `screen_capture` from the player's view (no camera arguments during Play) to check what the player sees: is the next goal visible?
4. Jumps: `user_keyboard_input` with a Space press while walking, or `execute_luau` with `datamodel_type: "Server"` to read the character's position after a run.
5. `get_console_output` for errors, then `start_stop_play` with `is_start: false`.

Fix metrics in the greybox script and rerun it rather than nudging parts by hand.

## Don'ts

- Detailing before a playtest.
- Gaps and ledges at the engine's limits.
- Many look-alike corridors with no landmark.
- Narrow 4-stud corridors on main routes; the camera fights the walls.
- A path that is the same colour and brightness as everything around it.
- Spawning players facing a wall or above a hazard.

## Sources used

- https://create.roblox.com/docs/tutorials/curriculums/core/building/greybox-a-playable-area
- https://create.roblox.com/docs/tutorials/curriculums/environmental-art/greybox-your-environment
- https://create.roblox.com/docs/production/game-design/prototyping
- https://create.roblox.com/docs/reference/engine/classes/Humanoid
- https://create.roblox.com/docs/reference/engine/classes/StarterPlayer
- https://create.roblox.com/docs/reference/engine/classes/Workspace
- https://kenney.nl/assets/platformer-kit
