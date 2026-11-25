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

-- A value as a golden field: numbers rounded, booleans and strings kept,
-- anything else as its type and text.
function Harness.show(v)
	if type(v) == "number" then
		return Harness.round(v, 4)
	elseif type(v) == "boolean" or type(v) == "string" then
		return v
	elseif v == nil then
		return "nil"
	elseif typeof(v) == "Vector3" then
		return "Vector3:" .. Harness.vec(v, 3)
	end
	return typeof(v) .. ":" .. tostring(v)
end

-- `f(...)` shown, or its error message without the script position.
function Harness.try(f, ...)
	local ok, value = pcall(f, ...)
	if ok then
		return Harness.show(value)
	end
	return "err: " .. (tostring(value):gsub("^.-:%d+: ", ""))
end

-- Each property in `props` of `instance` into `result` as `prefix.prop`.
function Harness.props(result, prefix, instance, props)
	for _, prop in props do
		result[prefix .. "." .. prop] = Harness.try(function()
			return instance[prop]
		end)
	end
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

-- An anchored floor whose top is at `origin`.
function Harness.floor(origin, size)
	local s = size or Vector3.new(120, 2, 120)
	return Harness.part({
		Name = "Floor",
		Anchored = true,
		Size = s,
		Position = origin - Vector3.new(0, s.Y / 2, 0),
	})
end

-- R15 parts (name, size, centre) and joints (name, part0, part1, pivot),
-- relative to the root's centre, which sits 3 studs above the feet. Our
-- own proportions: 2 stud legs under a 2x2x1 root, so HipHeight is 2.
local R15 = {
	parts = {
		{ "LowerTorso", Vector3.new(2, 0.4, 1), Vector3.new(0, -0.8, 0) },
		{ "UpperTorso", Vector3.new(2, 1.6, 1), Vector3.new(0, 0.2, 0) },
		{ "Head", Vector3.new(1.2, 1.2, 1.2), Vector3.new(0, 1.6, 0) },
		{ "LeftUpperArm", Vector3.new(1, 1, 1), Vector3.new(-1.5, 0.5, 0) },
		{ "LeftLowerArm", Vector3.new(1, 0.8, 1), Vector3.new(-1.5, -0.4, 0) },
		{ "LeftHand", Vector3.new(1, 0.2, 1), Vector3.new(-1.5, -0.9, 0) },
		{ "RightUpperArm", Vector3.new(1, 1, 1), Vector3.new(1.5, 0.5, 0) },
		{ "RightLowerArm", Vector3.new(1, 0.8, 1), Vector3.new(1.5, -0.4, 0) },
		{ "RightHand", Vector3.new(1, 0.2, 1), Vector3.new(1.5, -0.9, 0) },
		{ "LeftUpperLeg", Vector3.new(1, 0.9, 1), Vector3.new(-0.5, -1.45, 0) },
		{ "LeftLowerLeg", Vector3.new(1, 0.9, 1), Vector3.new(-0.5, -2.35, 0) },
		{ "LeftFoot", Vector3.new(1, 0.2, 1), Vector3.new(-0.5, -2.9, 0) },
		{ "RightUpperLeg", Vector3.new(1, 0.9, 1), Vector3.new(0.5, -1.45, 0) },
		{ "RightLowerLeg", Vector3.new(1, 0.9, 1), Vector3.new(0.5, -2.35, 0) },
		{ "RightFoot", Vector3.new(1, 0.2, 1), Vector3.new(0.5, -2.9, 0) },
	},
	joints = {
		{ "Root", "HumanoidRootPart", "LowerTorso", Vector3.new(0, -0.6, 0) },
		{ "Waist", "LowerTorso", "UpperTorso", Vector3.new(0, -0.6, 0) },
		{ "Neck", "UpperTorso", "Head", Vector3.new(0, 1, 0) },
		{ "LeftShoulder", "UpperTorso", "LeftUpperArm", Vector3.new(-1.5, 1, 0) },
		{ "LeftElbow", "LeftUpperArm", "LeftLowerArm", Vector3.new(-1.5, 0, 0) },
		{ "LeftWrist", "LeftLowerArm", "LeftHand", Vector3.new(-1.5, -0.8, 0) },
		{ "RightShoulder", "UpperTorso", "RightUpperArm", Vector3.new(1.5, 1, 0) },
		{ "RightElbow", "RightUpperArm", "RightLowerArm", Vector3.new(1.5, 0, 0) },
		{ "RightWrist", "RightLowerArm", "RightHand", Vector3.new(1.5, -0.8, 0) },
		{ "LeftHip", "LowerTorso", "LeftUpperLeg", Vector3.new(-0.5, -1, 0) },
		{ "LeftKnee", "LeftUpperLeg", "LeftLowerLeg", Vector3.new(-0.5, -1.9, 0) },
		{ "LeftAnkle", "LeftLowerLeg", "LeftFoot", Vector3.new(-0.5, -2.8, 0) },
		{ "RightHip", "LowerTorso", "RightUpperLeg", Vector3.new(0.5, -1, 0) },
		{ "RightKnee", "RightUpperLeg", "RightLowerLeg", Vector3.new(0.5, -1.9, 0) },
		{ "RightAnkle", "RightLowerLeg", "RightFoot", Vector3.new(0.5, -2.8, 0) },
	},
}

-- R6: joints in the torso (RootJoint in the root), 2 stud legs.
local R6 = {
	parts = {
		{ "Torso", Vector3.new(2, 2, 1), Vector3.new(0, 0, 0) },
		{ "Head", Vector3.new(2, 1, 1), Vector3.new(0, 1.5, 0) },
		{ "Left Arm", Vector3.new(1, 2, 1), Vector3.new(-1.5, 0, 0) },
		{ "Right Arm", Vector3.new(1, 2, 1), Vector3.new(1.5, 0, 0) },
		{ "Left Leg", Vector3.new(1, 2, 1), Vector3.new(-0.5, -2, 0) },
		{ "Right Leg", Vector3.new(1, 2, 1), Vector3.new(0.5, -2, 0) },
	},
	joints = {
		{ "RootJoint", "HumanoidRootPart", "Torso", Vector3.new(0, 0, 0), "HumanoidRootPart" },
		{ "Neck", "Torso", "Head", Vector3.new(0, 1, 0), "Torso" },
		{ "Left Shoulder", "Torso", "Left Arm", Vector3.new(-1.5, 0.5, 0), "Torso" },
		{ "Right Shoulder", "Torso", "Right Arm", Vector3.new(1.5, 0.5, 0), "Torso" },
		{ "Left Hip", "Torso", "Left Leg", Vector3.new(-0.5, -1, 0), "Torso" },
		{ "Right Hip", "Torso", "Right Leg", Vector3.new(0.5, -1, 0), "Torso" },
	},
}

-- A character made of plain Parts (no assets), so real Roblox and rbx_sim
-- start from the same model: a Humanoid (`props` set on it), a
-- HumanoidRootPart and Motor6Ds, the bottoms of its feet at `feet`. In
-- workspace with the server owning it, unless `keep` (then unparented).
function Harness.character(feet, rigType, props, keep)
	local layout = if rigType == "R6" then R6 else R15
	local model = Instance.new("Model")
	model.Name = "Rig"
	local centre = CFrame.new(feet + Vector3.new(0, 3, 0))
	local parts = {}
	local function add(name, size, offset)
		local part = Instance.new("Part")
		part.Name = name
		part.Size = size
		part.CFrame = centre + offset
		part.TopSurface = Enum.SurfaceType.Smooth
		part.BottomSurface = Enum.SurfaceType.Smooth
		part.Parent = model
		parts[name] = part
		return part
	end
	local root = add("HumanoidRootPart", Vector3.new(2, 2, 1), Vector3.zero)
	root.Transparency = 1
	for _, p in layout.parts do
		add(p[1], p[2], p[3])
	end
	for _, j in layout.joints do
		local motor = Instance.new("Motor6D")
		motor.Name = j[1]
		motor.Part0 = parts[j[2]]
		motor.Part1 = parts[j[3]]
		local pivot = centre + j[4]
		motor.C0 = motor.Part0.CFrame:ToObjectSpace(pivot)
		motor.C1 = motor.Part1.CFrame:ToObjectSpace(pivot)
		motor.Parent = parts[j[5] or j[3]]
	end
	local humanoid = Instance.new("Humanoid")
	humanoid.RigType = if rigType == "R6" then Enum.HumanoidRigType.R6 else Enum.HumanoidRigType.R15
	humanoid.HipHeight = if rigType == "R6" then 0 else 2
	for key, value in props or {} do
		humanoid[key] = value
	end
	humanoid.Parent = model
	model.PrimaryPart = root
	if not keep then
		model.Parent = workspace
		root:SetNetworkOwner(nil)
	end
	return model, humanoid, root
end

-- Logs `humanoid`'s state changes as "Old>New@beat", the beat read from
-- `clock`.
function Harness.states(humanoid, clock)
	local log = {}
	humanoid.StateChanged:Connect(function(old, new)
		table.insert(log, string.format("%s>%s@%d", old.Name, new.Name, clock.now()))
	end)
	return log
end

-- A heartbeat counter: `tick()` waits a beat and counts it, `now()` reads
-- the count.
function Harness.clock()
	local beat = 0
	return {
		tick = function()
			RunService.Heartbeat:Wait()
			beat += 1
			return beat
		end,
		now = function()
			return beat
		end,
	}
end

-- Facing in degrees: 0 along -Z (a fresh part's LookVector), 90 along +X.
function Harness.yaw(cframe)
	local look = cframe.LookVector
	return Harness.round(math.deg(math.atan2(look.X, -look.Z)), 2)
end

-- The one player in the game.
function Harness.player()
	local Players = game:GetService("Players")
	return Players:GetPlayers()[1] or Players.PlayerAdded:Wait()
end

-- The client half of a test: the module `name` in
-- ReplicatedStorage.ConformanceClient, called on the player's client with
-- `...`; returns what it returns.
local clientReady = false
local clientFolder = game:GetService("ReplicatedStorage"):FindFirstChild("ConformanceClient")
if clientFolder then
	clientFolder:WaitForChild("Ready").OnServerEvent:Connect(function()
		clientReady = true
	end)
end

function Harness.client(name, ...)
	local player = Harness.player()
	while not clientReady do
		RunService.Heartbeat:Wait()
	end
	return clientFolder.Call:InvokeClient(player, name, ...)
end

return Harness
