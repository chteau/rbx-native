--!nonstrict
-- The client half of input in Local Run: UserInputService's state and
-- methods, ContextActionService's bindings and RunService's render-step
-- bindings. Our own, written from the creator docs: an input goes to the
-- bound actions first, highest priority first and among equals the newest
-- binding first, until one sinks it (any result but Pass); the engine then
-- fires UserInputService's signal with whether an action took it.

local M = {}

local bindings = {}
local bound = 0
local steps = {}
local keys = {}
local buttons = {}
local delta = Vector2.zero
local location = Vector2.zero
local lastType = Enum.UserInputType.None

local DEFAULT_PRIORITY = Enum.ContextActionPriority.Medium.Value

local function matches(binding, input)
	for _, wanted in binding.inputs do
		if wanted == input.KeyCode or wanted == input.UserInputType then
			return true
		end
	end
	return false
end

local function values(map)
	local list = {}
	for _, value in map do
		table.insert(list, value)
	end
	return list
end

local function ordered()
	local list = values(bindings)
	table.sort(list, function(a, b)
		if a.priority ~= b.priority then
			return a.priority > b.priority
		end
		return a.order > b.order
	end)
	return list
end

-- A handler that yields has taken the input; one that errors is reported
-- and has too.
local function handle(binding, input)
	local thread = coroutine.create(binding.callback)
	local ok, result = coroutine.resume(thread, binding.name, input.UserInputState, input)
	if not ok then
		warn(debug.traceback(thread, tostring(result)))
		return Enum.ContextActionResult.Sink
	end
	if coroutine.status(thread) ~= "dead" then
		return Enum.ContextActionResult.Sink
	end
	return result
end

-- Frame start: the mouse delta counts this frame's motion only.
function M.frame()
	delta = Vector2.zero
end

-- One input, after the engine wrote its InputObject: true when a bound
-- action sank it.
function M.dispatch(input)
	local kind = input.UserInputType
	local began = input.UserInputState == Enum.UserInputState.Begin
	if kind == Enum.UserInputType.Keyboard then
		keys[input.KeyCode.Name] = if began then input else nil
	elseif string.sub(kind.Name, 1, 11) == "MouseButton" then
		buttons[kind.Name] = if began then input else nil
	elseif kind == Enum.UserInputType.MouseMovement then
		delta += Vector2.new(input.Delta.X, input.Delta.Y)
	end
	if kind == Enum.UserInputType.MouseMovement or string.sub(kind.Name, 1, 5) == "Mouse" then
		location = Vector2.new(input.Position.X, input.Position.Y)
	end
	lastType = kind
	for _, binding in ordered() do
		if matches(binding, input) and handle(binding, input) ~= Enum.ContextActionResult.Pass then
			return true
		end
	end
	return false
end

-- Runs the render-step bindings, lowest priority value first.
function M.render(dt)
	local list = values(steps)
	table.sort(list, function(a, b)
		if a.priority ~= b.priority then
			return a.priority < b.priority
		end
		return a.order < b.order
	end)
	for _, step in list do
		if steps[step.name] == step then
			local ok, err = pcall(step.callback, dt)
			if not ok then
				warn(err)
			end
		end
	end
end

local function bind(name, callback, touch, priority, ...)
	if type(name) ~= "string" then
		error("Argument 1 missing or nil", 3)
	end
	if type(callback) ~= "function" then
		error("Argument 2 missing or nil", 3)
	end
	bound += 1
	bindings[name] = {
		name = name,
		callback = callback,
		touch = touch == true,
		priority = priority,
		order = bound,
		inputs = { ... },
	}
end

local function info(binding)
	return {
		inputTypes = table.clone(binding.inputs),
		priorityLevel = binding.priority,
		stackOrder = binding.order,
		createTouchButton = binding.touch,
	}
end

local function nothing() end

local CAS = {
	BindAction = function(_, name, callback, touch, ...)
		bind(name, callback, touch, DEFAULT_PRIORITY, ...)
	end,
	BindActionAtPriority = function(_, name, callback, touch, priority, ...)
		bind(name, callback, touch, tonumber(priority) or DEFAULT_PRIORITY, ...)
	end,
	UnbindAction = function(_, name)
		bindings[name] = nil
	end,
	UnbindAllActions = function()
		table.clear(bindings)
	end,
	GetBoundActionInfo = function(_, name)
		local binding = bindings[name]
		return if binding then info(binding) else {}
	end,
	GetAllBoundActionInfo = function()
		local all = {}
		for name, binding in bindings do
			all[name] = info(binding)
		end
		return all
	end,
	GetButton = function()
		return nil
	end,
	SetTitle = nothing,
	SetImage = nothing,
	SetDescription = nothing,
	SetPosition = nothing,
	BindActivate = nothing,
	UnbindActivate = nothing,
}

local UIS = {
	IsKeyDown = function(_, keyCode)
		return keyCode ~= nil and keys[keyCode.Name] ~= nil
	end,
	GetKeysPressed = function()
		return values(keys)
	end,
	IsMouseButtonPressed = function(_, button)
		return button ~= nil and buttons[button.Name] ~= nil
	end,
	GetMouseButtonsPressed = function()
		return values(buttons)
	end,
	GetMouseDelta = function(service)
		return delta * service.MouseDeltaSensitivity
	end,
	GetMouseLocation = function()
		return location
	end,
	GetLastInputType = function()
		return lastType
	end,
	GetFocusedTextBox = function()
		return nil
	end,
	GetConnectedGamepads = function()
		return {}
	end,
	GetNavigationGamepads = function()
		return {}
	end,
	GetSupportedGamepadKeyCodes = function()
		return {}
	end,
	GetGamepadState = function()
		return {}
	end,
	GetGamepadConnected = function()
		return false
	end,
	GamepadSupports = function()
		return false
	end,
	IsGamepadButtonDown = function()
		return false
	end,
	IsNavigationGamepad = function()
		return false
	end,
	SetNavigationGamepad = nothing,
	GetStringForKeyCode = function(_, keyCode)
		return if #keyCode.Name == 1 then keyCode.Name else ""
	end,
	GetPlatform = function()
		return Enum.Platform.None
	end,
	KeyboardEnabled = true,
	MouseEnabled = true,
	TouchEnabled = false,
	GamepadEnabled = false,
	VREnabled = false,
	AccelerometerEnabled = false,
	GyroscopeEnabled = false,
}

local RUN = {
	BindToRenderStep = function(_, name, priority, callback)
		if type(name) ~= "string" then
			error("Argument 1 missing or nil", 2)
		end
		if type(callback) ~= "function" then
			error("Argument 3 missing or nil", 2)
		end
		bound += 1
		steps[name] = { name = name, priority = tonumber(priority) or 0, callback = callback, order = bound }
	end,
	UnbindFromRenderStep = function(_, name)
		steps[name] = nil
	end,
}

function M.member(service, key)
	local members = if service == "ContextActionService"
		then CAS
		elseif service == "UserInputService" then UIS
		else RUN
	local member = members[key]
	if member == nil then
		return nil, false
	end
	return member, true
end

return M
