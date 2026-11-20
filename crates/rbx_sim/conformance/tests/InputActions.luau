-- The Input Action System: defaults and Fire on the server, then on the
-- client contexts bound to keys, pressed for real (E, then F), with
-- priorities and sinking. Needs a player.
local CONTEXT = { "Enabled", "Priority", "Sink" }
local ACTION = { "BoolState", "Direction1DState", "Direction2DState", "Direction3DState", "DisplayName", "Enabled", "PreferredBinding", "Type", "ViewportPositionState" }
local BINDING = { "Backward", "ClampMagnitudeToOne", "Deadzone", "DisplayName", "Down", "Forward", "KeyCode", "Left", "PointerIndex", "PressedThreshold", "PrimaryModifier", "ReleasedThreshold", "ResponseCurve", "Right", "Scale", "SecondaryModifier", "Type", "UIButton", "Up", "Vector2Scale", "Vector3Scale" }

return function(h)
	local r = {}
	local context = Instance.new("InputContext")
	local action = Instance.new("InputAction")
	local binding = Instance.new("InputBinding")
	h.props(r, "context", context, CONTEXT)
	h.props(r, "action", action, ACTION)
	h.props(r, "binding", binding, BINDING)
	binding.Parent = action
	action.Parent = context
	context.Parent = workspace
	r.server_state = h.try(function()
		return action:GetState()
	end)
	local fired = {}
	action.Pressed:Connect(function()
		table.insert(fired, "pressed")
	end)
	action.StateChanged:Connect(function(state)
		table.insert(fired, "state " .. tostring(state))
	end)
	r.server_fire = h.try(function()
		action:Fire(true)
		return action:GetState()
	end)
	h.beats(2)
	r.server_fired = table.concat(fired, ",")
	r.bindings = h.try(function()
		return #action:GetInputBindings()
	end)
	r.actions = h.try(function()
		return #context:GetInputActions()
	end)
	r.fire_wrong_type = h.try(function()
		action:Fire(Vector2.new(1, 1))
	end)
	r.state_1d = h.try(function()
		action.Type = Enum.InputActionType.Direction1D
		return action:GetState()
	end)
	context:Destroy()

	for key, value in h.client("InputActions", "setup") do
		r["client_" .. key] = value
	end
	local function gesture(text)
		print("CONFORMANCE_GESTURE " .. text)
		h.beats(30)
	end
	gesture("key E")
	gesture("key F")
	for key, value in h.client("InputActions", "log") do
		r[key] = value
	end
	return r
end
