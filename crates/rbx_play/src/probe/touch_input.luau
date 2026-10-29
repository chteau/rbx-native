-- Touch input for game scripts on a simulated touch device.
--
-- Sober hands every input to the game as a mouse, so a game's own touch
-- code never runs on a simulated phone. Game scripts are rewritten
-- (rbx_play::touch): their UserInputService goes through `wrap`, their
-- input events (`button.InputBegan`, `frame.TouchPan`, ...) through `event`
-- and their `typeof` through `typeof`. On a simulated touch device the
-- mouse becomes one finger:
--
-- - MouseButton1 down / move / up become a Touch InputObject's Begin /
--   Change / End, on InputBegan/InputChanged/InputEnded and
--   TouchStarted/TouchMoved/TouchEnded (the same object for the whole
--   touch, as Roblox does). Mouse movement with no button down, and the
--   other mouse buttons, are dropped: a phone has neither.
-- - The finger's gestures fire TouchTap, TouchLongPress, TouchPan and
--   TouchSwipe.
-- - The wheel is a second gesture with two fingers around the cursor: a
--   pinch (wheel up spreads them), or a rotation with Ctrl held (not
--   Shift: Shift toggles Roblox's Shift Lock, which pins the mouse to the
--   centre). It ends WHEEL_IDLE seconds after the last notch.
-- - A GuiObject's InputBegan/InputChanged/InputEnded and a GuiButton's
--   Activated hand over the same touch, and its gesture events fire for a
--   gesture that starts on it.
-- - A finger the game did not take turns the camera, and the camera
--   zooms on a pinch over the world, as on a phone; a rotation, or a pinch
--   over the game's active UI, does not move it.
-- - TouchEnabled reads true, MouseEnabled and KeyboardEnabled false,
--   PreferredInput Touch and GetLastInputType Touch.
-- - OnScreenKeyboardVisible, OnScreenKeyboardPosition and
--   OnScreenKeyboardSize report the editor's keyboard cover (`setKeyboard`),
--   and Changed and GetPropertyChangedSignal fire for them.
--
-- On a simulated console's client (IsTenFootInterface) only PreferredInput
-- changes: Gamepad, as on a console with its pad. Roblox's own CoreGui
-- reads the real service, so it still picks its UI by the real values.
--
-- A touch is a stand-in InputObject (a table): its properties, Changed,
-- GetPropertyChangedSignal and IsA work, and `typeof` reads "Instance".
-- Keyboard, gamepad and text input pass through unchanged. On the server
-- and off a simulated touch device every call hands back what it was given.

local Gesture = require(script.Parent:WaitForChild("__rbxnative_touch_gesture"))
local Proxy = require(script.Parent:WaitForChild("__rbxnative_touch_proxy"))
local Fingers = require(script.Parent:WaitForChild("__rbxnative_touch_fingers"))

local M = {}

M.typeof = Gesture.typeof

local OUTSIDE_ACTION = "__rbxnative_pointer_outside"

-- Sober reads the wheel wherever the pointer is; the editor says when the
-- pointer is off the client (`pointer` command), and the wheel then does
-- nothing: no zoom, no pinch. On every device, not only touch ones.
local pointerOutside = false

function M.setPointerInside(inside)
	if pointerOutside == not inside then
		return
	end
	pointerOutside = not inside
	local ContextActionService = game:GetService("ContextActionService")
	if pointerOutside then
		ContextActionService:BindActionAtPriority(OUTSIDE_ACTION, function()
			return Enum.ContextActionResult.Sink
		end, false, Enum.ContextActionPriority.High.Value + 1, Enum.UserInputType.MouseWheel)
	else
		ContextActionService:UnbindAction(OUTSIDE_ACTION)
	end
end

-- Everything that turns the mouse into fingers, shared by the service
-- proxy and every GuiObject's events. Built once per client.
local function build()
	local real = game:GetService("UserInputService")
	local UIT = Enum.UserInputType
	local E = { real = real, signals = {} }
	local fires = {}
	for _, name in Proxy.SIGNALS do
		local signal, fire = Gesture.signal()
		E.signals[name] = signal
		fires[name] = fire
	end

	local function isMouse(t)
		return t == UIT.MouseButton1
			or t == UIT.MouseButton2
			or t == UIT.MouseButton3
			or t == UIT.MouseMovement
			or t == UIT.MouseWheel
	end

	local lastType = UIT.Touch
	E.lastType = function()
		return lastType
	end
	real.LastInputTypeChanged:Connect(function(t)
		local mapped = if isMouse(t) then UIT.Touch else t
		if mapped ~= lastType then
			lastType = mapped
			fires.LastInputTypeChanged(mapped)
		end
	end)

	local fingers = Fingers(real, fires)
	local guiGestures = fingers.guiGestures

	real.InputBegan:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseButton1 then
			fingers.endWheel()
			fingers.announce(fingers.fingerBegin(input.Position.X, input.Position.Y), processed)
		elseif not isMouse(t) then
			fires.InputBegan(input, processed)
		end
	end)
	real.InputChanged:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseMovement then
			fingers.fingerMove(input.Position.X, input.Position.Y)
		elseif t == UIT.MouseWheel then
			if not fingers.current() and not pointerOutside then
				fingers.wheelNotch(input, processed)
			end
		elseif not isMouse(t) then
			fires.InputChanged(input, processed)
		end
	end)
	real.InputEnded:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseButton1 then
			fingers.fingerEnd(input.Position.X, input.Position.Y)
		elseif not isMouse(t) then
			fires.InputEnded(input, processed)
		end
	end)

	-- A GuiObject's events, see M.event.
	local guiCache = setmetatable({}, { __mode = "k" })
	function E.guiEvent(object, name)
		local cached = guiCache[object]
		if not cached then
			cached = {}
			guiCache[object] = cached
		end
		if cached[name] then
			return cached[name]
		end
		local signal, fire = Gesture.signal()
		cached[name] = signal
		if GESTURES[name] then
			local handlers = guiGestures[object]
			if not handlers then
				handlers = {}
				guiGestures[object] = handlers
			end
			handlers[name] = fire
		elseif name == "InputBegan" then
			object.InputBegan:Connect(function(input)
				local t = input.UserInputType
				if t == UIT.MouseButton1 then
					fire(fingers.fingerBegin(input.Position.X, input.Position.Y).touch)
				elseif not isMouse(t) then
					fire(input)
				end
			end)
		elseif name == "InputChanged" then
			object.InputChanged:Connect(function(input)
				local t = input.UserInputType
				if t == UIT.MouseMovement then
					fingers.fingerMove(input.Position.X, input.Position.Y)
					local active = fingers.current()
					if active then
						fire(active.touch)
					end
				elseif not isMouse(t) then
					fire(input)
				end
			end)
		elseif name == "InputEnded" then
			object.InputEnded:Connect(function(input)
				local t = input.UserInputType
				if t == UIT.MouseButton1 then
					local this = fingers.fingerEnd(input.Position.X, input.Position.Y)
					if this then
						fire(this.touch)
					end
				elseif not isMouse(t) then
					fire(input)
				end
			end)
		elseif name == "Activated" then
			object.Activated:Connect(function(input, clicks)
				if input and input.UserInputType == UIT.MouseButton1 then
					-- Activated comes on release: the touch has ended.
					local this = fingers.fingerEnd(input.Position.X, input.Position.Y)
					fire(if this then this.touch else input, clicks)
				else
					fire(input, clicks)
				end
			end)
		end
		return signal
	end

	-- MouseBehavior has no hold on a finger: a phone never pins touches to
	-- the centre. The camera sets LockCenter in first person and Shift
	-- Lock; the real mouse stays free, and the game reads back what it set.
	local mouseBehavior = real.MouseBehavior
	game:GetService("RunService"):BindToRenderStep("__rbxnative_touch_free", Enum.RenderPriority.Last.Value, function()
		if real.MouseBehavior ~= Enum.MouseBehavior.Default then
			real.MouseBehavior = Enum.MouseBehavior.Default
		end
	end)

	local overrides = {
		TouchEnabled = true,
		MouseEnabled = false,
		KeyboardEnabled = false,
		GetLastInputType = function()
			return lastType
		end,
	}
	pcall(function()
		overrides.PreferredInput = Enum.PreferredInput.Touch
	end)
	for name, signal in E.signals do
		overrides[name] = signal
	end

	-- The keyboard's properties are ours, so are their change signals.
	local changed, fireChanged = Gesture.signal()
	local keyboardSignals = {}
	for _, name in Gesture.KEYBOARD_PROPS do
		keyboardSignals[name] = table.pack(Gesture.signal())
	end
	real.Changed:Connect(function(property)
		if not keyboardSignals[property] then
			fireChanged(property)
		end
	end)
	overrides.Changed = changed
	overrides.GetPropertyChangedSignal = function(_, property)
		local entry = keyboardSignals[property]
		if entry then
			return entry[1]
		end
		return real:GetPropertyChangedSignal(property)
	end
	E.keyboardChanged = function(property)
		fireChanged(property)
		keyboardSignals[property][2]()
	end

	E.proxy = Proxy.makeProxy(real, overrides, function(k)
		if k == "MouseBehavior" then
			return mouseBehavior
		end
		return Proxy.keyboardValue(k)
	end, function(k, v)
		if k == "MouseBehavior" then
			mouseBehavior = v
			return true
		end
		return false
	end)
	return E
end

-- The engine on a simulated touch device's or console's client, false
-- anywhere else.
local engine = nil

local function current()
	if engine == nil then
		local real = game:GetService("UserInputService")
		local client = game:GetService("RunService"):IsClient()
		if client and real.TouchEnabled and not Proxy.preferredTouch(real) then
			engine = build()
		elseif client and Proxy.tenFoot() then
			engine = Proxy.console()
		else
			engine = false
		end
	end
	return engine or nil
end


-- The editor's keyboard cover moved (`keyboard` command): x, y, width,
-- height in client pixels, an empty one when it went down.
function M.setKeyboard(x, y, width, height)
	local changes = Proxy.moveKeyboard(x, y, width, height)
	local e = current()
	if e and e.keyboardChanged then
		for _, property in changes do
			e.keyboardChanged(property)
		end
	end
end

-- Started by the probe's client script, so the mouse is a finger even in
-- a game whose scripts never ask for input.
function M.start()
	current()
end

-- `<expr>:GetService("UserInputService")` and every other way to it: the
-- proxy on a simulated touch device, else what it was given.
function M.wrap(value)
	if typeof(value) ~= "Instance" or value.ClassName ~= "UserInputService" then
		return value
	end
	local e = current()
	return if e then e.proxy else value
end

-- `<object>.<name>` for an input event: a GuiObject's stand-in on a
-- simulated touch device, else the real event.
function M.event(object, name)
	if typeof(object) == "Instance" then
		local e = current()
		if e then
			if object == e.real then
				return e.signals[name] or object[name]
			end
			if e.guiEvent and object:IsA("GuiObject") and (name ~= "Activated" or object:IsA("GuiButton")) then
				return e.guiEvent(object, name)
			end
		end
	end
	return object[name]
end

return M
