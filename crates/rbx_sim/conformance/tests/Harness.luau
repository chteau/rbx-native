-- Shared by every conformance test, in real Roblox and in rbx_sim alike.
local RunService = game:GetService("RunService")

local Harness = {}

function Harness.beat()
	return RunService.Heartbeat:Wait()
end

function Harness.beats(n)
	for _ = 1, n do
		RunService.Heartbeat:Wait()
	end
end

-- A part in the test's own corner of the world. Unanchored ones stay with
-- the server, so a nearby player's client never simulates them.
function Harness.part(props, class)
	local part = Instance.new(class or "Part")
	for key, value in props do
		part[key] = value
	end
	part.Parent = workspace
	if not part.Anchored then
		part:SetNetworkOwner(nil)
	end
	return part
end

-- Any other instance: properties set, then parented (to workspace when
-- `parent` is nil).
function Harness.new(class, props, parent)
	local instance = Instance.new(class)
	for key, value in props do
		instance[key] = value
	end
	instance.Parent = parent or workspace
	return instance
end

function Harness.attach(part, cframe)
	return Harness.new("Attachment", { CFrame = cframe or CFrame.new() }, part)
end

-- An anchored 1x1x1 pivot at `position` and an unanchored body.
function Harness.rig(position, props)
	local pivot = Harness.part({
		Name = "Pivot",
		Anchored = true,
		Size = Vector3.new(1, 1, 1),
		Position = position,
	})
	return pivot, Harness.part(props)
end

function Harness.round(x, digits)
	local m = 10 ^ (digits or 4)
	return math.floor(x * m + 0.5) / m
end

function Harness.vec(v, digits)
	local d = digits or 2
	return string.format("%s,%s,%s", Harness.round(v.X, d), Harness.round(v.Y, d), Harness.round(v.Z, d))
end

-- The anchored scene the spatial query tests look at, around `origin`.
-- QueryMask doesn't collide with QueryGroup.
function Harness.queryScene(origin)
	local PhysicsService = game:GetService("PhysicsService")
	for _, name in { "QueryGroup", "QueryMask" } do
		if not PhysicsService:IsCollisionGroupRegistered(name) then
			PhysicsService:RegisterCollisionGroup(name)
		end
	end
	PhysicsService:CollisionGroupSetCollidable("QueryMask", "QueryGroup", false)
	local function make(props, class)
		props.Anchored = true
		props.Position = origin + props.Position
		return Harness.part(props, class)
	end
	local tilted = make({ Name = "Tilted", Size = Vector3.new(6, 0.5, 6), Position = Vector3.new(0, 10, 0) })
	tilted.CFrame = CFrame.new(origin + Vector3.new(0, 10, 0)) * CFrame.Angles(0, math.rad(45), 0)
	return {
		tilted,
		make({ Name = "Wall", Size = Vector3.new(10, 10, 1), Position = Vector3.new(0, 0, 10) }),
		make({ Name = "Ghost", CanCollide = false, Size = Vector3.new(4, 4, 0.5), Position = Vector3.new(0, 0, 5) }),
		make({
			Name = "Hidden",
			CanCollide = false,
			CanQuery = false,
			Size = Vector3.new(4, 4, 0.5),
			Position = Vector3.new(0, 0, 3),
		}),
		make({
			Name = "Grouped",
			CollisionGroup = "QueryGroup",
			Size = Vector3.new(4, 4, 0.5),
			Position = Vector3.new(0, 0, 7),
		}),
		make({ Name = "Ball", Shape = Enum.PartType.Ball, Size = Vector3.new(4, 4, 4), Position = Vector3.new(10, 0, 0) }),
		make({
			Name = "Cyl",
			Shape = Enum.PartType.Cylinder,
			Material = Enum.Material.Metal,
			Size = Vector3.new(6, 3, 3),
			Position = Vector3.new(-10, 0, 0),
		}),
		make({ Name = "Slope", Size = Vector3.new(4, 4, 4), Position = Vector3.new(0, 0, -10) }, "WedgePart"),
	}
end

function Harness.clear(parts)
	for _, part in parts do
		part:Destroy()
	end
end

-- A query hit as "Name d=distance n=normal", or "nil".
function Harness.hit(result)
	if not result then
		return "nil"
	end
	return string.format(
		"%s d=%s n=%s %s",
		result.Instance.Name,
		Harness.round(result.Distance, 2),
		Harness.vec(result.Normal),
		result.Material.Name
	)
end

-- Names of `parts`, sorted, comma separated.
function Harness.names(parts)
	local names = {}
	for _, part in parts do
		table.insert(names, part.Name)
	end
	table.sort(names)
	return table.concat(names, ",")
end

return Harness
