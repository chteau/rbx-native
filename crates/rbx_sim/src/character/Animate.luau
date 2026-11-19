--!strict
-- Animate: the character animation script of the rigs the Rig Builder makes.
--
-- An original implementation, written only from the public documentation of
-- the Humanoid state events, Animator:LoadAnimation, AnimationTrack and
-- BindableFunction. It serves R6 and R15 alike: what a pose plays is whatever
-- Animation children its StringValue folder holds (weighted by an optional
-- `Weight` NumberValue, default 1), so overriding an animation id, or adding
-- a second idle, needs no change here.
--
-- Everything is event driven: nothing polls and nothing steps every frame.

local Players = game:GetService("Players")

local character = script.Parent :: Model
local humanoid = character:WaitForChild("Humanoid") :: Humanoid
local animator = humanoid:WaitForChild("Animator") :: Animator

-- Studs per second at which each pose's animation plays at its authored speed.
local REFERENCE_SPEED: { [string]: number } = {
	walk = 12,
	run = 18,
	climb = 5,
	swim = 10,
}
-- From this speed on, the `run` folder is used instead of `walk`.
local RUN_FROM = 14
-- At or below this speed a running humanoid counts as standing.
local STANDING = 0.5
local FADE = 0.2
local JUMP_FALLBACK_LENGTH = 0.3
local EMOTES = { wave = true, point = true, dance = true, dance2 = true, dance3 = true, laugh = true, cheer = true }
local INTERRUPTED = {
	[Enum.HumanoidStateType.FallingDown] = true,
	[Enum.HumanoidStateType.Ragdoll] = true,
	[Enum.HumanoidStateType.PlatformStanding] = true,
	[Enum.HumanoidStateType.GettingUp] = true,
}

local connections: { RBXScriptConnection } = {}
local tracks: { [Animation]: AnimationTrack } = {}

local pose = ""
local main: AnimationTrack? = nil
local idleLoop: RBXScriptConnection? = nil
local emote: AnimationTrack? = nil
local toolTrack: AnimationTrack? = nil
local toolWatch: RBXScriptConnection? = nil
local moveSpeed = 0
-- Bumped by every pose change, so a delayed follow-up knows it is stale.
local epoch = 0

local function keep(connection: RBXScriptConnection)
	table.insert(connections, connection)
end

local function load(animation: Animation): AnimationTrack
	local track = tracks[animation]
	if not track then
		track = animator:LoadAnimation(animation)
		tracks[animation] = track
	end
	return track
end

-- A weighted random Animation of the folder `name`, read when asked so edits
-- to the folder apply without any bookkeeping.
local function pick(name: string): Animation?
	local folder = script:FindFirstChild(name)
	if not folder then
		return nil
	end
	local candidates: { Animation } = {}
	local weights: { number } = {}
	local total = 0
	for _, child in folder:GetChildren() do
		if child:IsA("Animation") then
			local value = child:FindFirstChild("Weight")
			local weight = if value and value:IsA("NumberValue") then math.max(value.Value, 0) else 1
			table.insert(candidates, child)
			table.insert(weights, weight)
			total += weight
		end
	end
	local roll = math.random() * total
	for index, animation in candidates do
		roll -= weights[index]
		if roll <= 0 and weights[index] > 0 then
			return animation
		end
	end
	return candidates[#candidates]
end

-- How much the body's height scale stretches a stride, damped by
-- ScaleDampeningPercent. Rigs without scale values (R6) are unscaled.
local function strideScale(): number
	local height = humanoid:FindFirstChild("BodyHeightScale")
	local damping = script:FindFirstChild("ScaleDampeningPercent")
	if not (height and height:IsA("NumberValue")) then
		return 1
	end
	local percent = if damping and damping:IsA("NumberValue") then damping.Value else 1
	return math.max(1 + (height.Value - 1) * percent, 0.1)
end

local function stopEmote()
	if emote then
		emote:Stop(FADE)
		emote = nil
	end
end

-- Plays the pose `name`. The pose already playing is left alone unless
-- `reroll`, which draws its animation again (only idle has a reason to).
local function enter(name: string, fade: number?, reroll: boolean?): AnimationTrack?
	if name == pose and main and not reroll then
		return main
	end
	local animation = pick(name)
	if not animation then
		return nil
	end
	local track = load(animation)
	epoch += 1
	pose = name
	if name ~= "idle" then
		stopEmote()
	end
	if track == main then
		return track
	end
	local previous = main
	main = track
	track.Priority = Enum.AnimationPriority.Core
	track:Play(fade or FADE)
	if previous then
		previous:Stop(fade or FADE)
	end
	if idleLoop then
		idleLoop:Disconnect()
		idleLoop = nil
	end
	if name == "idle" then
		-- A pose that loops is the only time a new idle can be told from a stall.
		idleLoop = track.DidLoop:Connect(function()
			if pose == "idle" then
				enter("idle", 0.4, true)
			end
		end)
	end
	return track
end

local function stopMain(fade: number)
	epoch += 1
	pose = ""
	if main then
		main:Stop(fade)
		main = nil
	end
	if idleLoop then
		idleLoop:Disconnect()
		idleLoop = nil
	end
end

local function locomote(speed: number)
	moveSpeed = speed
	if speed <= STANDING then
		enter("idle")
		return
	end
	local name = if speed >= RUN_FROM and pick("run") then "run" else "walk"
	local track = enter(name)
	if track then
		track:AdjustSpeed(speed / (REFERENCE_SPEED[name] * strideScale()))
	end
end

-- Tool animation: a Tool in the character's hands holds the `toolnone` pose;
-- the "toolanim" StringValue the tool is handed asks for a one-off swing.

local function playTool(name: string)
	local animation = pick(name)
	if not animation then
		return
	end
	local track = load(animation)
	if toolTrack and toolTrack ~= track then
		toolTrack:Stop(0.1)
	end
	toolTrack = track
	track.Priority = Enum.AnimationPriority.Action
	track:Play(0.1)
	if name ~= "toolnone" then
		task.delay(math.max(track.Length, 0.2), function()
			if toolTrack == track then
				playTool("toolnone")
			end
		end)
	end
end

local function stopTool()
	if toolTrack then
		toolTrack:Stop(0.1)
		toolTrack = nil
	end
	if toolWatch then
		toolWatch:Disconnect()
		toolWatch = nil
	end
end

local function equip(tool: Tool)
	playTool("toolnone")
	if toolWatch then
		toolWatch:Disconnect()
	end
	toolWatch = tool.ChildAdded:Connect(function(child: Instance)
		if child.Name == "toolanim" and child:IsA("StringValue") then
			local request = string.lower(child.Value)
			if request == "slash" then
				playTool("toolslash")
			elseif request == "lunge" then
				playTool("toollunge")
			else
				playTool("toolnone")
			end
			child:Destroy()
		end
	end)
end

-- Emotes: standing still only; any other pose ends one.

local function playEmote(request: any): (boolean, AnimationTrack?)
	if pose ~= "idle" then
		return false, nil
	end
	local animation: Animation? = nil
	if typeof(request) == "string" then
		local name = string.lower(request)
		if EMOTES[name] then
			animation = pick(name)
		end
	elseif typeof(request) == "Instance" and request:IsA("Animation") then
		animation = request
	end
	if not animation then
		return false, nil
	end
	stopEmote()
	local track = load(animation)
	track.Priority = Enum.AnimationPriority.Action
	track:Play(FADE)
	emote = track
	return true, track
end

local function cleanup()
	for _, connection in connections do
		connection:Disconnect()
	end
	table.clear(connections)
	stopMain(0)
	stopEmote()
	stopTool()
	for _, track in tracks do
		track:Stop(0)
		track:Destroy()
	end
	table.clear(tracks)
end

keep(humanoid.Running:Connect(function(speed: number)
	locomote(speed)
end))

keep(humanoid.Jumping:Connect(function(active: boolean)
	if not active then
		return
	end
	local track = enter("jump", 0.1)
	if not track then
		return
	end
	local token = epoch
	task.delay(if track.Length > 0 then track.Length else JUMP_FALLBACK_LENGTH, function()
		if token == epoch then
			enter("fall", 0.2)
		end
	end)
end))

keep(humanoid.FreeFalling:Connect(function(active: boolean)
	if active and pose ~= "jump" then
		enter("fall", 0.3)
	end
end))

keep(humanoid.Climbing:Connect(function(speed: number)
	local track = enter("climb")
	if track then
		track:AdjustSpeed(speed / REFERENCE_SPEED.climb)
	end
end))

keep(humanoid.Seated:Connect(function(active: boolean)
	if active then
		enter("sit")
	end
end))

keep(humanoid.Swimming:Connect(function(speed: number)
	local track = if speed > 1 then enter("swim") else (enter("swimidle") or enter("swim"))
	if track and pose == "swim" then
		track:AdjustSpeed(math.max(speed, 1) / REFERENCE_SPEED.swim)
	end
end))

keep(humanoid.Died:Connect(function()
	stopMain(0.1)
	stopEmote()
	stopTool()
end))

keep(humanoid.StateChanged:Connect(function(_: Enum.HumanoidStateType, new: Enum.HumanoidStateType)
	if INTERRUPTED[new] then
		stopMain(0.1)
	end
end))

keep(character.ChildAdded:Connect(function(child: Instance)
	if child:IsA("Tool") then
		equip(child)
	end
end))

keep(character.ChildRemoved:Connect(function(child: Instance)
	if child:IsA("Tool") then
		stopTool()
	end
end))

-- A new height scale or damping changes how fast the current stride plays.
for _, name in { "BodyHeightScale", "ScaleDampeningPercent" } do
	local value = humanoid:FindFirstChild(name) or script:FindFirstChild(name)
	if value and value:IsA("NumberValue") then
		keep(value.Changed:Connect(function()
			if pose == "walk" or pose == "run" then
				locomote(moveSpeed)
			end
		end))
	end
end

local bindable = script:FindFirstChild("PlayEmote")
if bindable and bindable:IsA("BindableFunction") then
	bindable.OnInvoke = playEmote
end

local player = Players:GetPlayerFromCharacter(character)
if player and player == Players.LocalPlayer then
	keep(player.Chatted:Connect(function(message: string)
		local name = string.match(message, "^/e%s+(%S+)") or string.match(message, "^/emote%s+(%S+)")
		if name then
			playEmote(name)
		end
	end))
end

keep(character.AncestryChanged:Connect(function(_: Instance, parent: Instance?)
	if parent == nil then
		cleanup()
	end
end))
keep(script.Destroying:Connect(cleanup))

-- The `mood` folder, when a rig has one, is a face animation that never stops.
local mood = pick("mood")
if mood then
	local track = load(mood)
	track.Priority = Enum.AnimationPriority.Idle
	track.Looped = true
	track:Play(0)
end

locomote(0)
for _, child in character:GetChildren() do
	if child:IsA("Tool") then
		equip(child)
	end
end
