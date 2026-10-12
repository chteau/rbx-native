--[[
GraphProbe: settles three Animation Graph behaviours in real Studio.

Press Play (F5) or Run (F8) and copy every Output line that starts with
"PROBE". Nothing here uploads anything.

How a graph plays without being published (the Animation Editor's own
"Create Animate script" template does this, Client Tracker
AnimationEditor/Resources/ScriptTemplates/Animate):
  AnimationClipProvider:RegisterAnimationClip(<AnimationGraphDefinition>)
returns a temporary content id, which goes on an Animation's AnimationId
and is played through the rig's Animator. The editor warns that this only
works while Workspace.AuthorityMode is Automatic; the place is saved that
way.

The clips are local KeyframeSequences, so each Clip node's AnimationId
(saved as the KeyframeSequence's name) is first swapped for the id that
registering that KeyframeSequence returns.

Fallback, only if registering a graph fails: publish the graph from the
Graph Editor, then give this script an attribute named
"<GraphName>_AssetId" (for example ProbeBlend2D_AssetId) holding the number.
That id is played instead.

Questions:
  Q2 random - the picks a RandomSequence makes with Seed 42 (twice) and 7.
  Q3 blend  - RightShoulder.Transform under Blend2D and Over at set inputs.
]]

local RunService = game:GetService("RunService")
local ServerStorage = game:GetService("ServerStorage")
local ClipProvider = game:GetService("AnimationClipProvider")
local SequenceProvider = game:GetService("KeyframeSequenceProvider")

local saves = ServerStorage:WaitForChild("RBX_ANIMSAVES"):WaitForChild("ProbeRig")
local rig = workspace:WaitForChild("ProbeRig")
local animator = rig:WaitForChild("Humanoid"):WaitForChild("Animator")
local shoulder = rig:WaitForChild("RightUpperArm"):WaitForChild("RightShoulder")

-- Copies of the graphs that this run edits, so the saved ones stay as made.
local scratch = Instance.new("Folder")
scratch.Name = "ProbeRuntime"
scratch.Parent = ServerStorage

local function out(...)
	print("PROBE", ...)
end

local function deg(r)
	return math.deg(r)
end

-- The whole Transform: position, ToOrientation (Y, X, Z order, degrees),
-- ToEulerAnglesXYZ (degrees) and the twelve GetComponents numbers.
local function describe(cf)
	local ox, oy, oz = cf:ToOrientation()
	local ex, ey, ez = cf:ToEulerAnglesXYZ()
	local parts = {}
	for i, v in { cf:GetComponents() } do
		parts[i] = string.format("%.4f", v)
	end
	return string.format(
		"pos=(%.4f,%.4f,%.4f) orient=(%.2f,%.2f,%.2f) eulerXYZ=(%.2f,%.2f,%.2f) m=[%s]",
		cf.X, cf.Y, cf.Z,
		deg(ox), deg(oy), deg(oz),
		deg(ex), deg(ey), deg(ez),
		table.concat(parts, ",")
	)
end

-- Lets the Animator run for `seconds`, then one more step so the
-- Transform read is this frame's.
local function settle(seconds)
	task.wait(seconds)
	RunService.Stepped:Wait()
	RunService.Heartbeat:Wait()
end

-- Registers a KeyframeSequence or a graph and returns its temporary id.
local function register(instance)
	local ok, id = pcall(function()
		return ClipProvider:RegisterAnimationClip(instance)
	end)
	if ok and id and tostring(id) ~= "" then
		return tostring(id), "AnimationClipProvider"
	end
	local why = tostring(id)
	if instance:IsA("KeyframeSequence") then
		local ok2, id2 = pcall(function()
			return SequenceProvider:RegisterKeyframeSequence(instance)
		end)
		if ok2 and id2 and tostring(id2) ~= "" then
			return tostring(id2), "KeyframeSequenceProvider"
		end
		why ..= " / " .. tostring(id2)
	end
	return nil, why
end

-- Every KeyframeSequence in the save folder, registered by name.
local clipIds = {}
for _, child in saves:GetChildren() do
	if child:IsA("KeyframeSequence") then
		local id, how = register(child)
		clipIds[child.Name] = id
		out("clip", child.Name, id and "registered" or "REGISTER-FAILED", id or how, id and how or "")
	end
end

-- A copy of graph `name` whose Clip nodes name registered ids.
local function prepared(name, keepNames)
	local source = saves:FindFirstChild(name)
	if not source then
		out(name, "MISSING from ServerStorage.RBX_ANIMSAVES.ProbeRig")
		return nil
	end
	local copy = source:Clone()
	copy.Parent = scratch
	if not keepNames then
		for _, node in copy:GetDescendants() do
			local clip = node:GetAttribute("AnimationId")
			if typeof(clip) == "string" and clipIds[clip] then
				node:SetAttribute("AnimationId", clipIds[clip])
			end
		end
	end
	return copy
end

local current = nil

local function stop()
	if current then
		current:Stop(0)
		current:Destroy()
		current = nil
	end
	settle(0.25)
end

-- Plays `definition` (a graph or a KeyframeSequence) and returns its track.
local function play(definition, label, params)
	stop()
	if not definition then
		return nil
	end
	local id, how
	local key = definition.Name .. "_AssetId"
	local override = script:GetAttribute(key)
	if override then
		id, how = "rbxassetid://" .. tostring(override), "attribute " .. key
	else
		id, how = register(definition)
	end
	if not id then
		out(label, "REGISTER-FAILED", how)
		return nil
	end
	local animation = Instance.new("Animation")
	animation.Name = label
	animation.AnimationId = id
	pcall(function()
		animation:AddTag("AnimationEditor_GraphAnimation")
	end)
	local ok, track = pcall(function()
		return animator:LoadAnimation(animation)
	end)
	if not ok then
		out(label, "LOAD-FAILED", track)
		return nil
	end
	track.Priority = Enum.AnimationPriority.Action4
	for key, value in params or {} do
		local set, err = pcall(function()
			track:SetParameter(key, value)
		end)
		if not set then
			out(label, "SETPARAMETER-FAILED", key, err)
		end
	end
	track:Play(0)
	current = track
	settle(0.1)
	local defaults = {}
	local got, list = pcall(function()
		return track:GetParameterDefaults()
	end)
	if got and type(list) == "table" then
		for k, v in list do
			table.insert(defaults, tostring(k) .. "=" .. tostring(v))
		end
		table.sort(defaults)
	end
	out(
		label, "playing", "via", how, "id", id,
		"length", string.format("%.3f", track.Length),
		"isPlaying", track.IsPlaying,
		"paramDefaults", got and "{" .. table.concat(defaults, ",") .. "}" or ("ERR " .. tostring(list))
	)
	return track
end

local function sample(label, what, track, params, wait)
	for key, value in params do
		local ok, err = pcall(function()
			track:SetParameter(key, value)
		end)
		if not ok then
			out(label, "SETPARAMETER-FAILED", key, err)
		end
	end
	settle(wait or 0.6)
	out(label, what, describe(shoulder.Transform))
end

-- Which clip the arm shows: ArmUp is +90 about X.
local function which()
	local x, _, z = shoulder.Transform:ToEulerAnglesXYZ()
	x, z = deg(x), deg(z)
	if math.abs(x - 90) < 5 and math.abs(z) < 5 then
		return "ArmUp"
	elseif math.abs(x) < 1 and math.abs(z) < 1 then
		return "rest"
	end
	return string.format("other(x=%.1f,z=%.1f)", x, z)
end

out("start", "AuthorityMode", tostring(workspace.AuthorityMode), "server", RunService:IsServer())

-- Baseline: a KeyframeSequence on its own, to show registering and reading
-- the Transform work at all. Expect eulerXYZ x=90.
do
	local ks = saves:FindFirstChild("ArmUp")
	if play(ks, "baseline-ArmUp") then
		settle(0.4)
		out("baseline-ArmUp", "transform", describe(shoulder.Transform))
	end
end

-- Sanity: one Clip node into the output (ArmUp), first with its
-- AnimationId left as the KeyframeSequence's name, then with the
-- registered id. Says how a graph's clips must be named.
do
	if play(prepared("ProbeSanity", true), "ProbeSanity-names") then
		settle(0.4)
		out("ProbeSanity-names", "shows", which(), describe(shoulder.Transform))
	end
	if play(prepared("ProbeSanity", false), "ProbeSanity") then
		settle(0.4)
		out("ProbeSanity", "shows", which(), describe(shoulder.Transform))
	end
end

-- Q2: four Once clips of 0.6 s, the arm at 0/30/60/90 degrees about X and
-- rising 0.5 studs along Y through each clip, so a repeat pick shows as Y
-- dropping back. Records the first 16 picks (1 = Pick0 ... 4 = Pick3),
-- starting 0.1 s into the first one.
local function picks(graph, label)
	local track = play(prepared(graph, false), label)
	if not track then
		return
	end
	local list, markers = {}, {}
	local lastY = math.huge
	local marker = track:GetMarkerReachedSignal("Pick"):Connect(function(value)
		table.insert(markers, tostring(value))
	end)
	local started = os.clock()
	local connection
	connection = RunService.Heartbeat:Connect(function()
		local t = shoulder.Transform
		local x = deg((t:ToEulerAnglesXYZ()))
		if t.Y < lastY - 0.2 or (#list == 0 and lastY == math.huge) then
			table.insert(list, math.floor(x / 30 + 0.5) + 1)
		end
		lastY = t.Y
	end)
	while #list < 16 and os.clock() - started < 15 do
		task.wait(0.1)
	end
	connection:Disconnect()
	marker:Disconnect()
	out(label, "picks", table.concat(list, ","))
	out(label, "markers", table.concat(markers, ","))
end
picks("ProbeRandom42", "ProbeRandom42-run1")
picks("ProbeRandom42", "ProbeRandom42-run2")
picks("ProbeRandom7", "ProbeRandom7")

-- Luau's own Random with the same seeds, for comparison with the picks.
for _, seed in { 42, 7 } do
	local r = Random.new(seed)
	local ints, units = {}, {}
	for i = 1, 16 do
		ints[i] = r:NextInteger(1, 4)
	end
	r = Random.new(seed)
	for i = 1, 6 do
		units[i] = string.format("%.6f", r:NextNumber())
	end
	out("luau-random", seed, "NextInteger(1,4)", table.concat(ints, ","), "NextNumber", table.concat(units, ","))
end

-- Q3a: Blend2D, inputs (1,0) ZPos90, (-1,0) ZNeg90, (0,1) XPos90,
-- (0,-1) XNeg90, X and Y bound to the parameters X and Y.
do
	local track = play(prepared("ProbeBlend2D", false), "ProbeBlend2D", { X = 0, Y = 0 })
	if track then
		for _, at in { { 1, 0 }, { 0, 1 }, { 0, 0 }, { 0.5, 0 }, { 0.3, 0.3 }, { 1, 1 }, { 0.7, -0.2 } } do
			sample("ProbeBlend2D", string.format("at(%.2f,%.2f)", at[1], at[2]), track, { X = at[1], Y = at[2] })
		end
	end
end

-- Q3b: Over, Base X120 (120 about X), Over Z120 (120 about Z), Weight
-- bound to the parameter W.
do
	local track = play(prepared("ProbeOver", false), "ProbeOver", { W = 0 })
	if track then
		for _, w in { 0, 0.25, 0.5, 0.75, 1 } do
			sample("ProbeOver", string.format("W=%.2f", w), track, { W = w })
		end
	end
end

stop()
out("done")
