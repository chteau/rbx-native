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
-- Connected gamepads (by UserInputType) to their InputObjects by KeyCode
-- name; the gamepads set to navigate GUIs.
local gamepads = {}
local navigation = {}
-- The motion sensors' last readings, by the signal they fire.
local motion = {}
-- What VirtualInput objects sent, for the engine to take each frame.
local virtual = {}

-- What a standard gamepad has, in the order GetGamepadState lists it.
local GAMEPAD_CODES = {
	"ButtonX",
	"ButtonY",
	"ButtonA",
	"ButtonB",
	"ButtonR1",
	"ButtonL1",
	"ButtonR2",
	"ButtonL2",
	"ButtonR3",
	"ButtonL3",
	"ButtonStart",
	"ButtonSelect",
	"DPadLeft",
	"DPadRight",
	"DPadUp",
	"DPadDown",
	"Thumbstick1",
	"Thumbstick2",
}

-- The engine's own prompt images for gamepad buttons (rbxasset paths the
-- client ships, not images of ours); anything else has none.
local KEY_IMAGES = {
	ButtonA = "xboxA",
	ButtonB = "xboxB",
	ButtonX = "xboxX",
	ButtonY = "xboxY",
	ButtonL1 = "xboxLB",
	ButtonR1 = "xboxRB",
	ButtonL2 = "xboxLT",
	ButtonR2 = "xboxRT",
	ButtonL3 = "xboxLS",
	ButtonR3 = "xboxRS",
	ButtonStart = "xboxmenu",
	ButtonSelect = "xboxView",
	DPadUp = "dpadUp",
	DPadDown = "dpadDown",
	DPadLeft = "dpadLeft",
	DPadRight = "dpadRight",
	Thumbstick1 = "xboxLSDirectional",
	Thumbstick2 = "xboxRSDirectional",
}

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
	elseif gamepads[kind] then
		gamepads[kind][input.KeyCode.Name] = input
	end
	if kind == Enum.UserInputType.MouseMovement or string.sub(kind.Name, 1, 5) == "Mouse" then
		-- A mouse InputObject counts from below the top bar; the location
		-- doesn't.
		location = Vector2.new(input.Position.X, input.Position.Y + TOPBAR_INSET)
	end
	local changed = kind ~= lastType
	lastType = kind
	if sunk then
		return true, changed
	end
	for _, binding in ordered() do
		if matches(binding, input) and handle(binding, input) ~= Enum.ContextActionResult.Pass then
			return true, changed
		end
	end
	return false, changed
end

-- The arguments of the signal a touch or a sensor fires after its
-- InputBegan/InputChanged/InputEnded; a sensor's reading is kept for the
-- getters.
function M.also(name, input, processed)
	if string.sub(name, 1, 5) == "Touch" then
		return input, processed
	end
	motion[name] = input
	if name == "DeviceRotationChanged" then
		local p = input.Position
		return input, CFrame.fromEulerAnglesXYZ(p.X, p.Y, p.Z)
	end
	return input
end

-- A gamepad plugged in or out; whether that changed anything.
function M.gamepad(pad, connected)
	local kind = Enum.UserInputType["Gamepad" .. pad]
	if (gamepads[kind] ~= nil) == connected then
		return false
	end
	gamepads[kind] = if connected then {} else nil
	if not connected then
		navigation[kind] = nil
	end
	return true
end

function M.takeVirtual()
	local sent = virtual
	virtual = {}
	return sent
end

local function connected(kind)
	return typeof(kind) == "EnumItem" and gamepads[kind] ~= nil
end

local function supports(kind, code)
	return connected(kind) and typeof(code) == "EnumItem" and table.find(GAMEPAD_CODES, code.Name) ~= nil
end

local function sensor(name, kindName)
	return motion[name] or M.newInput(kindName, "Unknown")
end

-- A VirtualInput: what it sends reaches the game on the next frame, as
-- the player's own input would.
local function virtualInput(target)
	local object = {}
	local function send(...)
		table.insert(virtual, table.pack(...))
	end
	function object.SendKey(_, pressed, keyCode)
		assert(typeof(keyCode) == "EnumItem", "Argument 2 missing or nil")
		send("key", keyCode.Name, pressed == true)
	end
	function object.SendMouseButton(_, position, button, down)
		assert(typeof(position) == "Vector2", "Argument 1 missing or nil")
		assert(typeof(button) == "EnumItem", "Argument 2 missing or nil")
		local n = tonumber(string.match(button.Name, "^MouseButton(%d)$"))
		send("move", position.X, position.Y + TOPBAR_INSET, 0, 0)
		if n then
			send("button", n, down == true)
		end
	end
	function object.SendMouseDelta(_, d)
		assert(typeof(d) == "Vector2", "Argument 1 missing or nil")
		send("move", location.X + d.X, location.Y + d.Y, d.X, d.Y)
	end
	function object.SendMousePosition(_, position)
		assert(typeof(position) == "Vector2", "Argument 1 missing or nil")
		send("move", position.X, position.Y + TOPBAR_INSET, position.X - location.X, position.Y + TOPBAR_INSET - location.Y)
	end
	function object.SendPointerAction(_, position, action)
		assert(typeof(position) == "Vector2", "Argument 1 missing or nil")
		send("move", position.X, position.Y + TOPBAR_INSET, 0, 0)
		local wheel = type(action) == "table" and tonumber(action.Wheel)
		if wheel and wheel ~= 0 then
			send("wheel", wheel)
		end
	end
	function object.SendTextInput(_, text)
		send("text", tostring(text))
	end
	return setmetatable(object, {
		__tostring = function()
			return "VirtualInput"
		end,
		__index = function(_, key)
			if key == "ClassName" then
				return "VirtualInput"
			end
			if key == "Target" then
				return target
			end
			error(`{key} is not a valid member of VirtualInput`, 2)
		end,
	})
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

local MODIFIERS = {
	Shift = { "LeftShift", "RightShift" },
	Ctrl = { "LeftControl", "RightControl" },
	Alt = { "LeftAlt", "RightAlt" },
	Meta = { "LeftMeta", "RightMeta", "LeftSuper", "RightSuper" },
}

local INPUT_OBJECT = {
	IsModifierKeyDown = function(_, modifier)
		if typeof(modifier) ~= "EnumItem" or modifier.EnumType ~= Enum.ModifierKey then
			error("Argument 1 missing or nil", 2)
		end
		for _, name in MODIFIERS[modifier.Name] do
			if keys[name] then
				return true
			end
		end
		return false
	end,
}

local function currentToolIcon()
	local player = game:GetService("Players").LocalPlayer
	local tool = player and player.Character and player.Character:FindFirstChildOfClass("Tool")
	return if tool then tool.TextureId else ""
end

local CAS = {
	GetCurrentLocalToolIcon = currentToolIcon,
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
		local list = {}
		for kind in gamepads do
			table.insert(list, kind)
		end
		table.sort(list, function(a, b)
			return a.Value < b.Value
		end)
		return list
	end,
	GetNavigationGamepads = function()
		local list = {}
		for kind in navigation do
			table.insert(list, kind)
		end
		table.sort(list, function(a, b)
			return a.Value < b.Value
		end)
		return list
	end,
	GetSupportedGamepadKeyCodes = function(_, kind)
		local list = {}
		if connected(kind) then
			for _, name in GAMEPAD_CODES do
				table.insert(list, Enum.KeyCode[name])
			end
		end
		return list
	end,
	GetGamepadState = function(_, kind)
		local list = {}
		if connected(kind) then
			local pad = gamepads[kind]
			for _, name in GAMEPAD_CODES do
				pad[name] = pad[name] or M.newInput(kind.Name, name)
				table.insert(list, pad[name])
			end
		end
		return list
	end,
	GetGamepadConnected = function(_, kind)
		return connected(kind)
	end,
	GamepadSupports = function(_, kind, code)
		return supports(kind, code)
	end,
	IsGamepadButtonDown = function(_, kind, code)
		local input = supports(kind, code) and gamepads[kind][code.Name]
		return input ~= nil and input ~= false and input.UserInputState == Enum.UserInputState.Begin
	end,
	-- With none set, every gamepad navigates.
	IsNavigationGamepad = function(_, kind)
		if next(navigation) == nil then
			return connected(kind)
		end
		return navigation[kind] == true
	end,
	SetNavigationGamepad = function(_, kind, enabled)
		if typeof(kind) ~= "EnumItem" then
			error("Argument 1 missing or nil", 2)
		end
		navigation[kind] = if enabled then true else nil
	end,
	GetImageForKeyCode = function(_, keyCode)
		if typeof(keyCode) ~= "EnumItem" then
			error("Argument 1 missing or nil", 2)
		end
		local image = KEY_IMAGES[keyCode.Name]
		return if image then `rbxasset://textures/ui/Controls/{image}.png` else ""
	end,
	GetDeviceGravity = function()
		return sensor("DeviceGravityChanged", "Accelerometer")
	end,
	GetDeviceAcceleration = function()
		return sensor("DeviceAccelerationChanged", "Accelerometer")
	end,
	GetDeviceRotation = function()
		local input = sensor("DeviceRotationChanged", "Gyro")
		local p = input.Position
		return input, CFrame.fromEulerAnglesXYZ(p.X, p.Y, p.Z)
	end,
	CreateVirtualInput = function(_, target)
		if typeof(target) ~= "Instance" then
			error("Argument 1 missing or nil", 2)
		end
		return virtualInput(target)
	end,
	RecenterUserHeadCFrame = nothing,
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
		elseif service == "InputObject" then INPUT_OBJECT
		else RUN
	local member = members[key]
	if member == nil then
		return nil, false
	end
	return member, true
end

return M
