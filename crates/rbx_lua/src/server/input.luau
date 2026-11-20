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

-- What a US keyboard types for the keys whose name isn't the character.
local KEY_STRINGS = {
	Zero = "0",
	One = "1",
	Two = "2",
	Three = "3",
	Four = "4",
	Five = "5",
	Six = "6",
	Seven = "7",
	Eight = "8",
	Nine = "9",
	Space = " ",
	Minus = "-",
	Equals = "=",
	LeftBracket = "[",
	RightBracket = "]",
	BackSlash = "\\",
	Semicolon = ";",
	Quote = "'",
	Backquote = "`",
	Comma = ",",
	Period = ".",
	Slash = "/",
}
-- GuiService:GetGuiInset()'s top (rbx_sim's GuiRuntime golden).
local TOPBAR_INSET = 58

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
-- action sank it, or the GUI already had (`sunk`: a button or a focused
-- TextBox took it, and no action hears it).
function M.dispatch(input, sunk)
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
		-- A mouse InputObject counts from below the top bar; the location
		-- doesn't.
		location = Vector2.new(input.Position.X, input.Position.Y + TOPBAR_INSET)
	end
	lastType = kind
	if sunk then
		return true
	end
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

-- On a touch device a binding that asks for one gets an on-screen button:
-- an ImageButton with an `ActionTitle` label in the player's
-- `ContextActionGui`, our own look (round, dark, up the right side above
-- the jump button), which a script styles with SetTitle/SetImage/
-- SetPosition or reaches through GetButton.
local SLOTS = {
	UDim2.new(1, -95, 1, -170),
	UDim2.new(1, -170, 1, -95),
	UDim2.new(1, -170, 1, -170),
	UDim2.new(1, -95, 1, -245),
	UDim2.new(1, -245, 1, -95),
	UDim2.new(1, -170, 1, -245),
	UDim2.new(1, -245, 1, -170),
}

local function buttonFrame()
	local player = game:GetService("Players").LocalPlayer
	local playerGui = player and player:FindFirstChildOfClass("PlayerGui")
	if not playerGui then
		return nil
	end
	local screen = playerGui:FindFirstChild("ContextActionGui")
	if not screen then
		screen = Instance.new("ScreenGui")
		screen.Name = "ContextActionGui"
		screen.ResetOnSpawn = false
		local frame = Instance.new("Frame")
		frame.Name = "ContextButtonFrame"
		frame.BackgroundTransparency = 1
		frame.Size = UDim2.fromScale(1, 1)
		frame.Parent = screen
		screen.Parent = playerGui
	end
	return screen:FindFirstChild("ContextButtonFrame")
end

local function freeSlot()
	local taken = {}
	for _, binding in bindings do
		if binding.slot then
			taken[binding.slot] = true
		end
	end
	for i = 1, #SLOTS do
		if not taken[i] then
			return i
		end
	end
	return #SLOTS
end

local function makeButton(binding)
	local frame = buttonFrame()
	if not frame then
		return
	end
	binding.slot = freeSlot()
	local button = Instance.new("ImageButton")
	button.Name = "ContextActionButton"
	button.Size = UDim2.fromOffset(70, 70)
	button.Position = SLOTS[binding.slot]
	button.BackgroundColor3 = Color3.new(0, 0, 0)
	button.BackgroundTransparency = 0.5
	button.AutoButtonColor = false
	local corner = Instance.new("UICorner")
	corner.CornerRadius = UDim.new(0.5, 0)
	corner.Parent = button
	local title = Instance.new("TextLabel")
	title.Name = "ActionTitle"
	title.BackgroundTransparency = 1
	title.Size = UDim2.fromScale(1, 1)
	title.Text = ""
	title.TextColor3 = Color3.new(1, 1, 1)
	title.TextSize = 18
	title.Font = Enum.Font.BuilderSansBold
	title.Parent = button
	local function press(input)
		local kind = input.UserInputType
		if kind == Enum.UserInputType.Touch or kind == Enum.UserInputType.MouseButton1 then
			handle(binding, input)
		end
	end
	button.InputBegan:Connect(press)
	button.InputEnded:Connect(press)
	button.Parent = frame
	binding.button = button
end

local function dropButton(binding)
	if binding and binding.button then
		binding.button:Destroy()
		binding.button = nil
	end
end

local function bind(name, callback, touch, priority, ...)
	if type(name) ~= "string" then
		error("Argument 1 missing or nil", 3)
	end
	if type(callback) ~= "function" then
		error("Argument 2 missing or nil", 3)
	end
	dropButton(bindings[name])
	bound += 1
	local binding = {
		name = name,
		callback = callback,
		touch = touch == true,
		priority = priority,
		order = bound,
		inputs = { ... },
	}
	bindings[name] = binding
	if binding.touch and M.touch then
		makeButton(binding)
	end
end

-- `f(button, value)` on `name`'s button, when it has one.
local function onButton(f)
	return function(_, name, value)
		local binding = bindings[name]
		if binding and binding.button then
			f(binding.button, value)
		end
	end
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
		dropButton(bindings[name])
		bindings[name] = nil
	end,
	UnbindAllActions = function()
		for _, binding in bindings do
			dropButton(binding)
		end
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
	GetButton = function(_, name)
		local binding = bindings[name]
		return binding and binding.button
	end,
	SetTitle = onButton(function(button, title)
		button.ActionTitle.Text = tostring(title)
	end),
	SetImage = onButton(function(button, image)
		button.Image = tostring(image)
	end),
	SetDescription = onButton(function(button, text)
		button:SetAttribute("Description", tostring(text))
	end),
	SetPosition = onButton(function(button, position)
		if typeof(position) == "UDim2" then
			button.Position = position
		end
	end),
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
		if typeof(keyCode) ~= "EnumItem" then
			error("Argument 1 missing or nil", 2)
		end
		local name = keyCode.Name
		return KEY_STRINGS[name] or (if #name == 1 then name else "")
	end,
	GetPlatform = function()
		return Enum.Platform.None
	end,
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
