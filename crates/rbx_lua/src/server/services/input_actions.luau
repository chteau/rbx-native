--!nonstrict
-- The Input Action System: InputContext, InputAction and InputBinding. Our
-- own, from the creator docs and the InputActions golden: an action's
-- state starts at its type's zero, Fire sets it (a Bool's Pressed, or
-- Released, fires ahead of StateChanged; the others only fire
-- StateChanged), and changing Type starts it over. On a client a key or
-- mouse button goes to every enabled context, highest Priority first; a
-- context with Sink set that has a binding for it keeps it from the
-- lower ones.
local host = ...

local M = {}

local states = {}
local pressed = {}

local KIND = Enum.InputActionType
local NONE = Enum.KeyCode.None

local MOUSE_KEYS = {
	[Enum.UserInputType.MouseButton1] = Enum.KeyCode.MouseLeftButton,
	[Enum.UserInputType.MouseButton2] = Enum.KeyCode.MouseRightButton,
	[Enum.UserInputType.MouseButton3] = Enum.KeyCode.MouseMiddleButton,
}

local WANTS = {
	[KIND.Bool] = "bool",
	[KIND.Direction1D] = "number",
	[KIND.Direction2D] = "Vector2",
	[KIND.Direction3D] = "Vector3",
	[KIND.ViewportPosition] = "Vector2",
}

local function zero(kind)
	if kind == KIND.Bool then
		return false
	elseif kind == KIND.Direction1D then
		return 0
	elseif kind == KIND.Direction3D then
		return Vector3.zero
	end
	return Vector2.zero
end

local function getState(action)
	local state = states[host.id(action)]
	if state == nil then
		return zero(action.Type)
	end
	return state
end

local function setState(action, value)
	local id = host.id(action)
	if getState(action) == value then
		return
	end
	states[id] = value
	if action.Type == KIND.Bool then
		host.fire(id, if value then "Pressed" else "Released")
	end
	host.fire(id, "StateChanged", value)
end

local function fire(action, value)
	local kind = action.Type
	local wants = WANTS[kind]
	local given = if wants == "bool" then type(value) == "boolean" elseif wants == "number" then type(value) == "number" else typeof(value) == wants
	if not given then
		error(`Action:Fire() on InputActionType.{kind.Name} should be called with a {wants} state`, 0)
	end
	setState(action, value)
end

local function robloxOnly(name)
	return nil, false, `The current thread cannot call '{name}' (lacking capability RobloxScript)`
end

local SIGNALS = { Pressed = true, Released = true, StateChanged = true }

function M.member(class, id, key)
	if class == "InputAction" then
		if SIGNALS[key] then
			return host.signal(id, key), true
		elseif key == "GetState" then
			return getState, true
		elseif key == "Fire" then
			return fire, true
		elseif key == "GetInputBindings" or key == "GetPreferredBindingList" then
			return robloxOnly(key)
		end
	elseif class == "InputContext" then
		if key == "GetInputActions" then
			return robloxOnly(key)
		end
	elseif class == "InputBinding" and key == "Fire" then
		return function(self, value)
			local action = self.Parent
			if action and action:IsA("InputAction") then
				fire(action, value)
			end
		end, true
	end
	return nil, false
end

function M.set(_, id, key)
	if key == "Type" then
		states[id] = nil
	end
	return false
end

local function down(code)
	return code ~= NONE and pressed[code.Name] == true
end

local function modifiersHeld(binding)
	for _, modifier in { binding.PrimaryModifier, binding.SecondaryModifier } do
		if modifier ~= NONE and not down(modifier) then
			return false
		end
	end
	return true
end

local function axis(positive, negative)
	return (if down(positive) then 1 else 0) - (if down(negative) then 1 else 0)
end

-- What one binding says the action's state is.
local function bindingValue(binding, kind)
	if not modifiersHeld(binding) then
		return zero(kind)
	end
	if kind == KIND.Bool then
		return down(binding.KeyCode)
	elseif kind == KIND.Direction1D then
		local value = axis(binding.Up, binding.Down)
		if value == 0 and down(binding.KeyCode) then
			value = 1
		end
		return value * binding.Scale
	elseif kind == KIND.Direction2D then
		local v = Vector2.new(axis(binding.Right, binding.Left), axis(binding.Up, binding.Down))
		if binding.ClampMagnitudeToOne and v.Magnitude > 1 then
			v = v.Unit
		end
		return v * binding.Vector2Scale
	elseif kind == KIND.Direction3D then
		local v = Vector3.new(axis(binding.Right, binding.Left), axis(binding.Up, binding.Down), axis(binding.Backward, binding.Forward))
		if binding.ClampMagnitudeToOne and v.Magnitude > 1 then
			v = v.Unit
		end
		return v * binding.Vector3Scale
	end
	return zero(kind)
end

local function magnitude(value)
	if type(value) == "boolean" then
		return if value then 1 else 0
	elseif type(value) == "number" then
		return math.abs(value)
	end
	return value.Magnitude
end

local function uses(binding, code)
	if binding.Type ~= Enum.InputBindingType.Automatic then
		return false
	end
	for _, key in { binding.KeyCode, binding.Up, binding.Down, binding.Left, binding.Right, binding.Forward, binding.Backward, binding.PrimaryModifier, binding.SecondaryModifier } do
		if key == code then
			return true
		end
	end
	return false
end

-- The action's state from every binding it has: the strongest wins.
local function refresh(action)
	local kind = action.Type
	local best = zero(kind)
	for _, binding in action:GetChildren() do
		if binding:IsA("InputBinding") and binding.Type == Enum.InputBindingType.Automatic then
			local value = bindingValue(binding, kind)
			if magnitude(value) > magnitude(best) then
				best = value
			end
		end
	end
	setState(action, best)
end

function M.input(input, processed)
	if host.server() then
		return
	end
	local state = input.UserInputState
	if state ~= Enum.UserInputState.Begin and state ~= Enum.UserInputState.End then
		return
	end
	local code = MOUSE_KEYS[input.UserInputType] or input.KeyCode
	if code == Enum.KeyCode.Unknown or code == NONE then
		return
	end
	local isDown = state == Enum.UserInputState.Begin
	if isDown and processed then
		return
	end
	pressed[code.Name] = isDown or nil
	local contexts = {}
	for _, context in host.of_class("InputContext") do
		if context.Enabled then
			table.insert(contexts, context)
		end
	end
	table.sort(contexts, function(a, b)
		return a.Priority > b.Priority
	end)
	for _, context in contexts do
		local heard = false
		for _, action in context:GetChildren() do
			if action:IsA("InputAction") and action.Enabled then
				for _, binding in action:GetChildren() do
					if binding:IsA("InputBinding") and uses(binding, code) then
						heard = true
						refresh(action)
						break
					end
				end
			end
		end
		if heard and context.Sink then
			break
		end
	end
end

return M
