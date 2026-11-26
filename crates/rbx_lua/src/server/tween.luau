--!nonstrict
-- TweenService in Local Run, our own, written from the creator docs and
-- what the TweenCurves and TweenRuntime goldens measured on a real server:
--
-- * Tweens step once a frame at its very start, before PreAnimation (on a
--   client too: after the previous frame's render step), so a tween played
--   after Heartbeat first writes next frame, one frame's worth along.
-- * A play is DelayTime (the frame the delay runs out moves nothing on),
--   then Time forward, then Time back when Reverses; every repetition
--   (RepeatCount more, negative for ever) starts with the delay again.
-- * A tween started on a property another tween plays takes it over from
--   the next step; a tween left with no property is cancelled then.
-- * Cancel fires Completed(Cancelled) before it returns and leaves the
--   values; Play after Cancel or the end starts again from the current
--   values, after Pause it carries on.
-- * Every type lerps component-wise (CFrame slerps), a bool flips on the
--   first step; integer offsets round.
local host = ...

local M = {}

local tweens = {} -- id -> tween
local order = {} -- ids of tweens that are playing or delayed, oldest first
local claims = {} -- instance id -> property name -> id of the tween writing it

local PI = math.pi

-- The In curve of each style; Out and InOut are made from it.
local curves = {
	Linear = function(t)
		return t
	end,
	Sine = function(t)
		return 1 - math.cos(t * PI / 2)
	end,
	Quad = function(t)
		return t * t
	end,
	Cubic = function(t)
		return t * t * t
	end,
	Quart = function(t)
		return t * t * t * t
	end,
	Quint = function(t)
		return t * t * t * t * t
	end,
	Exponential = function(t)
		return t == 0 and 0 or 2 ^ (10 * t - 10)
	end,
	Circular = function(t)
		return 1 - math.sqrt(1 - t * t)
	end,
	Back = function(t)
		local s = 1.70158
		return t * t * ((s + 1) * t - s)
	end,
	-- Roblox's period is 4/13 (TweenCurves: Elastic_Out matches
	-- 1 + 2^(-10t) sin((t - p/4) 2π/p) to the sixth decimal, not
	-- the usual 0.3).
	Elastic = function(t)
		if t == 0 or t == 1 then
			return t
		end
		local u = 1 - t
		local p = 4 / 13
		return -(2 ^ (-10 * u)) * math.sin((u - p / 4) * 2 * PI / p)
	end,
	Bounce = function(t)
		local u = 1 - t
		local v
		if u < 1 / 2.75 then
			v = 7.5625 * u * u
		elseif u < 2 / 2.75 then
			u -= 1.5 / 2.75
			v = 7.5625 * u * u + 0.75
		elseif u < 2.5 / 2.75 then
			u -= 2.25 / 2.75
			v = 7.5625 * u * u + 0.9375
		else
			u -= 2.625 / 2.75
			v = 7.5625 * u * u + 0.984375
		end
		return 1 - v
	end,
}

local function ease(alpha, style, direction)
	if alpha <= 0 then
		return 0
	elseif alpha >= 1 then
		return 1
	end
	local t = alpha
	local f = curves[style] or curves.Linear
	if direction == "In" then
		return f(t)
	elseif direction == "Out" then
		return 1 - f(1 - t)
	end
	if t < 0.5 then
		return f(2 * t) / 2
	end
	return 1 - f(2 - 2 * t) / 2
end

local function lerpNumber(a, b, t)
	return a + (b - a) * t
end

local function lerpMethod(a, b, t)
	return a:Lerp(b, t)
end

local lerps = {
	number = lerpNumber,
	boolean = function(a, b, t)
		return t > 0 and b or a
	end,
	CFrame = lerpMethod,
	Vector3 = lerpMethod,
	Vector2 = lerpMethod,
	Color3 = lerpMethod,
	UDim2 = function(a, b, t)
		return UDim2.new(
			lerpNumber(a.X.Scale, b.X.Scale, t),
			math.round(lerpNumber(a.X.Offset, b.X.Offset, t)),
			lerpNumber(a.Y.Scale, b.Y.Scale, t),
			math.round(lerpNumber(a.Y.Offset, b.Y.Offset, t))
		)
	end,
	UDim = function(a, b, t)
		return UDim.new(lerpNumber(a.Scale, b.Scale, t), math.round(lerpNumber(a.Offset, b.Offset, t)))
	end,
	Rect = function(a, b, t)
		return Rect.new(a.Min:Lerp(b.Min, t), a.Max:Lerp(b.Max, t))
	end,
	Vector2int16 = function(a, b, t)
		return Vector2int16.new(math.round(lerpNumber(a.X, b.X, t)), math.round(lerpNumber(a.Y, b.Y, t)))
	end,
	Vector3int16 = function(a, b, t)
		return Vector3int16.new(
			math.round(lerpNumber(a.X, b.X, t)),
			math.round(lerpNumber(a.Y, b.Y, t)),
			math.round(lerpNumber(a.Z, b.Z, t))
		)
	end,
}

-- How Roblox names a script value's type in Create's errors.
local given = { number = "double", boolean = "bool", EnumItem = "token" }

local function setState(tween, name)
	tween.state = name
	host.setState(tween.id, Enum.PlaybackState[name].Value)
end

local function unlist(id)
	local i = table.find(order, id)
	if i then
		table.remove(order, i)
	end
end

local function release(tween)
	local mine = claims[tween.target]
	if not mine then
		return
	end
	for name, id in mine do
		if id == tween.id then
			mine[name] = nil
		end
	end
	if next(mine) == nil then
		claims[tween.target] = nil
	end
end

local function finish(tween, name)
	unlist(tween.id)
	release(tween)
	tween.phase = nil
	setState(tween, name)
	host.fire(tween.id, Enum.PlaybackState[name])
end

local function write(tween, alpha)
	local target = tween.instance
	local mine = claims[tween.target] or {}
	for _, goal in tween.goals do
		if mine[goal.name] == tween.id then
			-- The ends exactly (a slerped CFrame at 1 is a hair off).
			local value = goal.to
			if alpha ~= 1 then
				value = goal.lerp(goal.from, goal.to, alpha)
			end
			pcall(function()
				target[goal.name] = value
			end)
		end
	end
end

local function owns(tween)
	local mine = claims[tween.target] or {}
	for _, goal in tween.goals do
		if mine[goal.name] == tween.id then
			return true
		end
	end
	return false
end

local function start(tween)
	local target = tween.instance
	for _, goal in tween.goals do
		goal.from = target[goal.name]
	end
	local mine = claims[tween.target] or {}
	claims[tween.target] = mine
	for _, goal in tween.goals do
		mine[goal.name] = tween.id
	end
	tween.repetition = 0
	tween.timer = 0
	tween.phase = tween.info.DelayTime > 0 and "delay" or "play"
	table.insert(order, tween.id)
	setState(tween, tween.phase == "delay" and "Delayed" or "Playing")
end

-- One frame of one tween.
local function advance(tween, dt)
	if not owns(tween) then
		finish(tween, "Cancelled")
		return
	end
	local info = tween.info
	if tween.phase == "delay" then
		if tween.timer < info.DelayTime - 1e-6 then
			tween.timer += dt
			return
		end
		tween.phase = "play"
		tween.timer = 0
		setState(tween, "Playing")
		write(tween, ease(0, info.EasingStyle.Name, info.EasingDirection.Name))
		return
	end
	tween.timer += dt
	local span = info.Time
	local length = span * (info.Reverses and 2 or 1)
	if tween.timer >= length - 1e-6 then
		write(tween, info.Reverses and 0 or 1)
		tween.repetition += 1
		if info.RepeatCount >= 0 and tween.repetition > info.RepeatCount then
			finish(tween, "Completed")
		elseif info.DelayTime > 0 then
			tween.phase = "delay"
			tween.timer = 0
			setState(tween, "Delayed")
		else
			tween.timer = length > 0 and tween.timer % length or 0
		end
		return
	end
	local t = tween.timer < span and tween.timer / span or 2 - tween.timer / span
	write(tween, ease(t, info.EasingStyle.Name, info.EasingDirection.Name))
end

local function create(_, instance, info, goals)
	if typeof(instance) ~= "Instance" then
		error("TweenService:Create failed because Instance is null", 3)
	end
	if typeof(info) ~= "TweenInfo" then
		error("Unable to cast value to TweenInfo", 3)
	end
	if type(goals) ~= "table" then
		error("Unable to cast value to Dictionary", 3)
	end
	local list = {}
	for name, goal in goals do
		local kind = host.kind(instance, tostring(name))
		if not kind then
			error(string.format("TweenService:Create no property named '%s' for object '%s'", tostring(name), instance.Name), 3)
		end
		local current = instance[name]
		local lerp = lerps[typeof(current)]
		if not lerp and typeof(current) ~= "EnumItem" then
			error(
				string.format(
					"TweenService:Create property named '%s' on object '%s' is not a data type that can be tweened",
					name,
					instance.Name
				),
				3
			)
		end
		if not lerp or typeof(goal) ~= typeof(current) then
			error(
				string.format(
					"TweenService:Create property named '%s' cannot be tweened due to type mismatch (property is a '%s', but given type is '%s')",
					name,
					kind,
					given[typeof(goal)] or typeof(goal)
				),
				3
			)
		end
		table.insert(list, { name = name, to = goal, lerp = lerp })
	end
	local object, id = host.new(instance)
	tweens[id] = {
		id = id,
		object = object,
		instance = instance,
		target = host.id(instance),
		info = info,
		goals = list,
		state = "Begin",
	}
	return object
end

local function getValue(_, alpha, style, direction)
	if type(alpha) ~= "number" then
		error(string.format("Unable to cast %s to float", typeof(alpha)), 3)
	end
	if style == Enum.EasingStyle.Spring or style == Enum.EasingStyle.Bezier then
		error("TweenService:GetValue does not support Enum.EasingStyle.Spring or Enum.EasingStyle.Bezier", 3)
	end
	return ease(alpha, style.Name, direction.Name)
end

local service = {
	Create = create,
	GetValue = getValue,
}

-- `TweenService` members.
function M.service(key)
	local f = service[key]
	return f, f ~= nil
end

local methods = {
	Play = function(tween)
		if tween.state == "Playing" or tween.state == "Delayed" then
			return
		end
		if tween.state == "Paused" and tween.phase then
			table.insert(order, tween.id)
			setState(tween, tween.phase == "delay" and "Delayed" or "Playing")
			return
		end
		start(tween)
	end,
	Pause = function(tween)
		if tween.state == "Playing" or tween.state == "Delayed" then
			unlist(tween.id)
			setState(tween, "Paused")
		end
	end,
	Cancel = function(tween)
		if tween.state == "Playing" or tween.state == "Delayed" or tween.state == "Paused" then
			finish(tween, "Cancelled")
		end
	end,
}

-- A tween's members, by its id.
function M.tween(id, key)
	local tween = tweens[id]
	if not tween then
		return nil, false
	end
	if key == "TweenInfo" then
		return tween.info, true
	end
	local f = methods[key]
	if f then
		return function()
			f(tween)
		end, true
	end
	return nil, false
end

-- One frame: every playing tween, oldest first.
function M.step(dt)
	for _, id in table.clone(order) do
		local tween = tweens[id]
		if tween and table.find(order, id) then
			advance(tween, dt)
		end
	end
end

return M
