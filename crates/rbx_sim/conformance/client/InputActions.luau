-- InputActions on the client: "setup" makes the contexts, "log" says what
-- the key presses did and cleans up.
local Players = game:GetService("Players")

local log = {}
local made = {}
local actions = {}

local function context(name, priority, sink)
	local c = Instance.new("InputContext")
	c.Name = name
	c.Priority = priority
	c.Sink = sink
	c.Parent = Players.LocalPlayer:WaitForChild("PlayerGui")
	table.insert(made, c)
	return c
end

local function action(parent, name, key, kind)
	local a = Instance.new("InputAction")
	a.Name = name
	if kind then
		a.Type = kind
	end
	local b = Instance.new("InputBinding")
	b.KeyCode = key
	b.Parent = a
	a.Parent = parent
	local events = {}
	log[name] = events
	a.Pressed:Connect(function()
		table.insert(events, "pressed")
	end)
	a.Released:Connect(function()
		table.insert(events, "released")
	end)
	a.StateChanged:Connect(function(state)
		table.insert(events, "state " .. tostring(state))
	end)
	actions[name] = a
	return a
end

local function try(f)
	local ok, value = pcall(f)
	if ok then
		return value == nil and "ok" or value
	end
	return "err: " .. (tostring(value):gsub("^.-:%d+: ", ""))
end

return function(_, step)
	if step == "setup" then
		local main = context("ConfMain", 1000, false)
		action(main, "Use", Enum.KeyCode.E)
		local high = context("ConfHigh", 2000, true)
		action(high, "High", Enum.KeyCode.F)
		local low = context("ConfLow", 500, false)
		action(low, "Low", Enum.KeyCode.F)
		action(main, "Axis", Enum.KeyCode.E, Enum.InputActionType.Direction1D)
		local r = {}
		r.state = try(function()
			return actions.Use:GetState()
		end)
		r.fire = try(function()
			actions.Use:Fire(true)
		end)
		r.fired_state = try(function()
			return actions.Use:GetState()
		end)
		try(function()
			actions.Use:Fire(false)
		end)
		task.wait()
		r.fire_log = table.concat(log.Use, ",")
		table.clear(log.Use)
		return r
	end
	local r = {}
	for name, events in log do
		r["log_" .. name] = table.concat(events, ",")
	end
	r.use_state = try(function()
		return actions.Use:GetState()
	end)
	r.axis_state = try(function()
		return actions.Axis:GetState()
	end)
	for _, c in made do
		c:Destroy()
	end
	table.clear(made)
	return r
end
