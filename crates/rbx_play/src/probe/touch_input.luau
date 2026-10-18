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
--
-- A touch is a stand-in InputObject (a table): its properties, Changed,
-- GetPropertyChangedSignal and IsA work, and `typeof` reads "Instance".
-- Keyboard, gamepad and text input pass through unchanged. On the server
-- and off a simulated touch device every call hands back what it was given.

local M = {}

M.SLOP = 10 -- pixels a finger moves before a press becomes a pan
M.LONG_PRESS = 0.5 -- seconds held still before a long press
M.TAP_TIME = 0.5 -- the longest press that still taps
M.SWIPE_SPEED = 400 -- pixels per second at release
M.SWIPE_DISTANCE = 40 -- pixels travelled
M.STALE = 0.1 -- seconds without a move after which the finger is at rest
M.PINCH_SPAN = 100 -- pixels between the two wheel fingers at the start
M.PINCH_STEP = 1.1 -- span factor per wheel notch
M.ROTATE_STEP = 15 -- degrees per wheel notch
M.WHEEL_IDLE = 0.3

-- One finger's gestures --------------------------------------------------------

-- Each function returns the gesture events it produced, in order:
-- { kind = "Tap", x, y }, { kind = "LongPress", state, x, y },
-- { kind = "Pan", state, x, y, tx, ty, vx, vy }, { kind = "Swipe", direction }.
-- States are "Begin", "Change", "End"; directions "Up", "Down", "Left", "Right".

function M.fingerDown(x, y, t)
	return { x0 = x, y0 = y, x = x, y = y, t0 = t, t = t, vx = 0, vy = 0, panning = false, longPress = false }
end

function M.fingerMove(f, x, y, t)
	local dt = t - f.t
	if dt > 0 then
		f.vx = (x - f.x) / dt
		f.vy = (y - f.y) / dt
	end
	f.x, f.y, f.t = x, y, t
	if f.longPress then
		return {}
	end
	local tx, ty = x - f.x0, y - f.y0
	if not f.panning then
		if tx * tx + ty * ty < M.SLOP * M.SLOP then
			return {}
		end
		f.panning = true
		return { { kind = "Pan", state = "Begin", x = x, y = y, tx = tx, ty = ty, vx = f.vx, vy = f.vy } }
	end
	return { { kind = "Pan", state = "Change", x = x, y = y, tx = tx, ty = ty, vx = f.vx, vy = f.vy } }
end

-- Called LONG_PRESS seconds after fingerDown.
function M.fingerHeld(f)
	if f.panning or f.longPress then
		return {}
	end
	f.longPress = true
	return { { kind = "LongPress", state = "Begin", x = f.x, y = f.y } }
end

function M.swipeDirection(tx, ty)
	if math.abs(tx) >= math.abs(ty) then
		return if tx > 0 then "Right" else "Left"
	end
	return if ty > 0 then "Down" else "Up"
end

function M.fingerUp(f, x, y, t)
	-- A release where the last move left the finger keeps that move's velocity.
	local events = if x ~= f.x or y ~= f.y then M.fingerMove(f, x, y, t) else {}
	if f.longPress then
		table.insert(events, { kind = "LongPress", state = "End", x = x, y = y })
		return events
	end
	if not f.panning then
		if t - f.t0 <= M.TAP_TIME then
			table.insert(events, { kind = "Tap", x = x, y = y })
		end
		return events
	end
	local tx, ty = x - f.x0, y - f.y0
	local vx, vy = f.vx, f.vy
	table.insert(events, { kind = "Pan", state = "End", x = x, y = y, tx = tx, ty = ty, vx = vx, vy = vy })
	local speed = math.sqrt(vx * vx + vy * vy)
	if speed >= M.SWIPE_SPEED and tx * tx + ty * ty >= M.SWIPE_DISTANCE * M.SWIPE_DISTANCE then
		table.insert(events, { kind = "Swipe", direction = M.swipeDirection(tx, ty) })
	end
	return events
end

-- Velocity at rest: a finger that stopped before lifting has none.
function M.settle(f, t)
	if t - f.t > M.STALE then
		f.vx, f.vy = 0, 0
	end
end

-- The wheel's two fingers --------------------------------------------------------

-- The two fingers `span` pixels apart, centred on (cx, cy), turned `angle`
-- degrees clockwise from horizontal. Returns x1, y1, x2, y2.
function M.pinchPositions(cx, cy, span, angle)
	local r = math.rad(angle)
	local dx, dy = math.cos(r) * span / 2, math.sin(r) * span / 2
	return cx - dx, cy - dy, cx + dx, cy + dy
end

-- One notch: wheel up spreads (or turns clockwise). Returns span, angle.
function M.wheelStep(span, angle, up, rotate)
	if rotate then
		return span, angle + (if up then M.ROTATE_STEP else -M.ROTATE_STEP)
	end
	return (if up then span * M.PINCH_STEP else span / M.PINCH_STEP), angle
end

-- The camera under a finger ------------------------------------------------------

-- CameraInput's touch rotation (PlayerModule): a finger moved (dx, dy)
-- pixels turns the camera by these radians, with pitch slowed towards the
-- poles. `pitch` is the camera's, in radians. Returns yaw, pitch.
M.ROTATION_SPEED_TOUCH = { x = math.rad(1), y = 0.66 * math.rad(1) }
M.MIN_TOUCH_SENSITIVITY_FRACTION = 0.25

function M.touchRotation(dx, dy, pitch)
	if dy * pitch < 0 then
		local curve = 1 - (2 * math.abs(pitch) / math.pi) ^ 0.75
		dy *= curve * (1 - M.MIN_TOUCH_SENSITIVITY_FRACTION) + M.MIN_TOUCH_SENSITIVITY_FRACTION
	end
	return dx * M.ROTATION_SPEED_TOUCH.x, dy * M.ROTATION_SPEED_TOUCH.y
end

-- BaseCamera keeps the pitch within ±80°: the pitch turn allowed from a
-- look vector at `pitch` radians.
function M.clampPitch(turn, pitch)
	return math.clamp(turn, math.rad(-80) + pitch, math.rad(80) + pitch)
end

-- Signals --------------------------------------------------------------------------

-- A Luau signal: a BindableEvent would copy the InputObject tables, and a
-- touch's handlers must all see the same object.
function M.signal()
	local handlers = {}
	local signal = {}
	local spawn = if task then task.spawn else function(fn, ...)
		coroutine.wrap(fn)(...)
	end
	function signal.Connect(_, fn)
		local entry = { fn = fn }
		table.insert(handlers, entry)
		local connection = { Connected = true }
		function connection.Disconnect(c)
			if not c.Connected then
				return
			end
			c.Connected = false
			local at = table.find(handlers, entry)
			if at then
				table.remove(handlers, at)
			end
		end
		return connection
	end
	signal.ConnectParallel = signal.Connect
	function signal.Once(self, fn)
		local connection
		connection = self:Connect(function(...)
			connection:Disconnect()
			fn(...)
		end)
		return connection
	end
	function signal.Wait(self)
		local thread = coroutine.running()
		self:Once(function(...)
			spawn(thread, ...)
		end)
		return coroutine.yield()
	end
	local function fire(...)
		for _, entry in table.clone(handlers) do
			spawn(entry.fn, ...)
		end
	end
	return signal, fire
end

-- Touch objects ------------------------------------------------------------------

-- A stand-in InputObject: a table whose properties change as the finger
-- moves, with the Changed and GetPropertyChangedSignal of the real one
-- (drag scripts watch `input.Changed` for the End state).
local touchData = setmetatable({}, { __mode = "k" })

local TOUCH_METHODS = {
	IsModifierKeyDown = function()
		return false
	end,
	IsA = function(_, class)
		return class == "InputObject" or class == "Instance"
	end,
	GetPropertyChangedSignal = function(self, property)
		local data = touchData[self]
		local entry = data.propertySignals[property]
		if not entry then
			local signal, fire = M.signal()
			entry = { signal = signal, fire = fire }
			data.propertySignals[property] = entry
		end
		return entry.signal
	end,
}

local TOUCH_META = {
	__index = function(self, k)
		local method = TOUCH_METHODS[k]
		if method then
			return method
		end
		local data = touchData[self]
		if k == "Changed" then
			return data.changed
		end
		return data.props[k]
	end,
	__newindex = function(self, k, v)
		M.setTouch(self, k, v)
	end,
	__tostring = function()
		return "InputObject"
	end,
}

-- `props` holds the initial properties.
function M.newTouch(props)
	local touch = setmetatable({}, TOUCH_META)
	local changed, fireChanged = M.signal()
	props.ClassName = "InputObject"
	props.Name = "InputObject"
	touchData[touch] = { props = props, changed = changed, fireChanged = fireChanged, propertySignals = {} }
	return touch
end

function M.setTouch(touch, k, v)
	local data = touchData[touch]
	if data.props[k] == v then
		return
	end
	data.props[k] = v
	data.fireChanged(k)
	local entry = data.propertySignals[k]
	if entry then
		entry.fire()
	end
end

function M.isTouch(v)
	return type(v) == "table" and touchData[v] ~= nil
end

-- Game scripts that say `typeof` get this one: a stand-in touch is the
-- Instance a real InputObject is.
function M.typeof(v)
	if M.isTouch(v) then
		return "Instance"
	end
	return typeof(v)
end

-- The engine ---------------------------------------------------------------------

local SIGNALS = {
	"InputBegan",
	"InputChanged",
	"InputEnded",
	"TouchStarted",
	"TouchMoved",
	"TouchEnded",
	"TouchTap",
	"TouchLongPress",
	"TouchPan",
	"TouchSwipe",
	"TouchPinch",
	"TouchRotate",
	"LastInputTypeChanged",
}

-- A GuiObject's gesture events, fired when the gesture starts on it.
local GESTURES = {
	TouchTap = true,
	TouchLongPress = true,
	TouchPan = true,
	TouchSwipe = true,
	TouchPinch = true,
	TouchRotate = true,
}

local WHEEL_ACTION = "__rbxnative_touch_wheel"
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

local function preferredTouch(real)
	local ok, pref = pcall(function()
		return real.PreferredInput == Enum.PreferredInput.Touch
	end)
	return ok and pref
end

-- Everything that turns the mouse into fingers, shared by the service
-- proxy and every GuiObject's events. Built once per client.
local function build()
	local real = game:GetService("UserInputService")
	local Players = game:GetService("Players")
	local ContextActionService = game:GetService("ContextActionService")
	local UIT = Enum.UserInputType
	local State = Enum.UserInputState
	local E = { real = real, signals = {} }
	local fires = {}
	for _, name in SIGNALS do
		local signal, fire = M.signal()
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

	local function playerGui()
		local player = Players.LocalPlayer
		return player and player:FindFirstChildOfClass("PlayerGui")
	end
	-- The game's GuiObjects under (x, y), as a set.
	local function hitsAt(x, y)
		local set = {}
		local gui = playerGui()
		if gui then
			local ok, list = pcall(gui.GetGuiObjectsAtPosition, gui, x, y)
			if ok then
				for _, object in list do
					set[object] = true
				end
			end
		end
		return set
	end

	-- GuiObject -> { gesture name -> fire }.
	local guiGestures = setmetatable({}, { __mode = "k" })
	local function guiFire(hits, name, ...)
		for object, handlers in guiGestures do
			local fire = handlers[name]
			if fire and hits[object] then
				fire(...)
			end
		end
	end

	local function newTouch(x, y)
		return M.newTouch({
			UserInputType = UIT.Touch,
			UserInputState = State.Begin,
			KeyCode = Enum.KeyCode.Unknown,
			Position = Vector3.new(x, y, 0),
			Delta = Vector3.zero,
		})
	end
	local function moveTouch(touch, x, y)
		local position = Vector3.new(x, y, 0)
		M.setTouch(touch, "Delta", position - touch.Position)
		M.setTouch(touch, "Position", position)
		M.setTouch(touch, "UserInputState", State.Change)
	end
	local function endTouch(touch)
		M.setTouch(touch, "Delta", Vector3.zero)
		M.setTouch(touch, "UserInputState", State.End)
	end

	-- Gesture events from the pure recognisers, to the service and to the
	-- GuiObjects the gesture started on.
	local function dispatch(events, source)
		local processed, hits = source.processed, source.hits
		for _, e in events do
			local positions = { Vector2.new(e.x or 0, e.y or 0) }
			if e.kind == "Tap" then
				fires.TouchTap(positions, processed)
				guiFire(hits, "TouchTap", positions)
			elseif e.kind == "LongPress" then
				local s = State[e.state]
				fires.TouchLongPress(positions, s, processed)
				guiFire(hits, "TouchLongPress", positions, s)
			elseif e.kind == "Pan" then
				local s = State[e.state]
				local translation, velocity = Vector2.new(e.tx, e.ty), Vector2.new(e.vx, e.vy)
				fires.TouchPan(positions, translation, velocity, s, processed)
				guiFire(hits, "TouchPan", positions, translation, velocity, s)
			elseif e.kind == "Swipe" then
				local direction = Enum.SwipeDirection[e.direction]
				fires.TouchSwipe(direction, 1, processed)
				guiFire(hits, "TouchSwipe", direction, 1)
			end
		end
	end

	-- The mouse finger. Its start, moves and end can each come first from
	-- the service or from a GuiObject's own event, so each step runs once.
	local finger = nil -- { touch, gesture, processed, hits, announced }
	local lastFinger = nil

	local function announce(this, processed)
		if this.announced then
			return
		end
		this.announced = true
		this.processed = processed
		fires.InputBegan(this.touch, processed)
		fires.TouchStarted(this.touch, processed)
	end
	local function fingerBegin(x, y)
		if finger then
			return finger
		end
		local this = {
			touch = newTouch(x, y),
			gesture = M.fingerDown(x, y, os.clock()),
			processed = false,
			hits = hitsAt(x, y),
			announced = false,
		}
		finger = this
		task.delay(M.LONG_PRESS, function()
			if finger == this then
				dispatch(M.fingerHeld(this.gesture), this)
			end
		end)
		return this
	end
	-- A finger the game did not take turns the camera, as on a phone.
	local cameraX, cameraY = 0, 0
	local function turnCamera(this, x, y)
		if not this.processed then
			cameraX += x - this.touch.Position.X
			cameraY += y - this.touch.Position.Y
		end
	end
	game:GetService("RunService"):BindToRenderStep(
		"__rbxnative_touch_camera",
		Enum.RenderPriority.Camera.Value + 1,
		function()
			if cameraX == 0 and cameraY == 0 then
				return
			end
			local dx, dy = cameraX, cameraY
			cameraX, cameraY = 0, 0
			local camera = workspace.CurrentCamera
			if not camera or camera.CameraType == Enum.CameraType.Scriptable then
				return
			end
			local look = camera.CFrame.LookVector
			local yaw, pitchTurn = M.touchRotation(dx, dy, (camera.CFrame:ToEulerAnglesYXZ()))
			pcall(function()
				pitchTurn *= UserSettings():GetService("UserGameSettings"):GetCameraYInvertValue()
			end)
			pitchTurn = M.clampPitch(pitchTurn, math.asin(look.Y))
			-- BaseCamera:CalculateNewLookCFrameFromArg; the camera starts
			-- from this look vector next frame, so the turn stays.
			local turned = (CFrame.Angles(0, -yaw, 0) * CFrame.new(Vector3.zero, look) * CFrame.Angles(-pitchTurn, 0, 0)).LookVector
			local focus = camera.Focus.Position
			local position = focus - turned * (camera.CFrame.Position - focus).Magnitude
			camera.CFrame = CFrame.lookAt(position, position + turned)
		end
	)

	local function fingerMove(x, y)
		local this = finger
		if not this or (x == this.gesture.x and y == this.gesture.y) then
			return
		end
		announce(this, this.processed)
		turnCamera(this, x, y)
		moveTouch(this.touch, x, y)
		fires.InputChanged(this.touch, this.processed)
		fires.TouchMoved(this.touch, this.processed)
		dispatch(M.fingerMove(this.gesture, x, y, os.clock()), this)
	end
	local function fingerEnd(x, y)
		local this = finger
		if not this then
			return lastFinger
		end
		finger = nil
		lastFinger = this
		announce(this, this.processed)
		local now = os.clock()
		M.settle(this.gesture, now)
		if x ~= this.gesture.x or y ~= this.gesture.y then
			turnCamera(this, x, y)
			moveTouch(this.touch, x, y)
			fires.InputChanged(this.touch, this.processed)
			fires.TouchMoved(this.touch, this.processed)
		end
		endTouch(this.touch)
		fires.InputEnded(this.touch, this.processed)
		fires.TouchEnded(this.touch, this.processed)
		dispatch(M.fingerUp(this.gesture, x, y, now), this)
		return this
	end

	-- The wheel's two fingers.
	local wheel = nil -- { a, b, cx, cy, span, angle, rotate, processed, hits, at, token }

	local function wheelGesture(state, velocity)
		local positions = {
			Vector2.new(wheel.a.Position.X, wheel.a.Position.Y),
			Vector2.new(wheel.b.Position.X, wheel.b.Position.Y),
		}
		local s = State[state]
		if wheel.rotate then
			fires.TouchRotate(positions, wheel.angle, velocity, s, wheel.processed)
			guiFire(wheel.hits, "TouchRotate", positions, wheel.angle, velocity, s)
		else
			local scale = wheel.span / M.PINCH_SPAN
			fires.TouchPinch(positions, scale, velocity, s, wheel.processed)
			guiFire(wheel.hits, "TouchPinch", positions, scale, velocity, s)
		end
	end
	local function endWheel()
		if not wheel then
			return
		end
		wheelGesture("End", 0)
		for _, touch in { wheel.a, wheel.b } do
			endTouch(touch)
			fires.InputEnded(touch, wheel.processed)
			fires.TouchEnded(touch, wheel.processed)
		end
		wheel = nil
	end
	local function rotating()
		return real:IsKeyDown(Enum.KeyCode.LeftControl) or real:IsKeyDown(Enum.KeyCode.RightControl)
	end
	local function wheelNotch(input, processed)
		local rotate = rotating()
		if wheel and wheel.rotate ~= rotate then
			endWheel()
		end
		local now = os.clock()
		if not wheel then
			local cx, cy = input.Position.X, input.Position.Y
			local x1, y1, x2, y2 = M.pinchPositions(cx, cy, M.PINCH_SPAN, 0)
			wheel = {
				a = newTouch(x1, y1),
				b = newTouch(x2, y2),
				cx = cx,
				cy = cy,
				span = M.PINCH_SPAN,
				angle = 0,
				rotate = rotate,
				processed = processed,
				hits = hitsAt(cx, cy),
				at = now,
				token = 0,
			}
			for _, touch in { wheel.a, wheel.b } do
				fires.InputBegan(touch, processed)
				fires.TouchStarted(touch, processed)
			end
			wheelGesture("Begin", 0)
		end
		local before = if rotate then wheel.angle else wheel.span / M.PINCH_SPAN
		wheel.span, wheel.angle = M.wheelStep(wheel.span, wheel.angle, input.Position.Z > 0, rotate)
		local after = if rotate then wheel.angle else wheel.span / M.PINCH_SPAN
		local dt = math.max(now - wheel.at, 1 / 60)
		wheel.at = now
		local x1, y1, x2, y2 = M.pinchPositions(wheel.cx, wheel.cy, wheel.span, wheel.angle)
		for touch, p in { [wheel.a] = { x1, y1 }, [wheel.b] = { x2, y2 } } do
			moveTouch(touch, p[1], p[2])
			fires.InputChanged(touch, processed)
			fires.TouchMoved(touch, processed)
		end
		wheelGesture("Change", (after - before) / dt)
		wheel.token += 1
		local token = wheel.token
		task.delay(M.WHEEL_IDLE, function()
			if wheel and wheel.token == token then
				endWheel()
			end
		end)
	end

	-- The camera zooms on the wheel, as it does on a pinch over the world.
	-- A rotation, and a pinch over the game's active UI, keep it still.
	pcall(function()
		ContextActionService:BindActionAtPriority(WHEEL_ACTION, function(_, _, input)
			if rotating() then
				return Enum.ContextActionResult.Sink
			end
			for object in hitsAt(input.Position.X, input.Position.Y) do
				if object.Active and object.Visible then
					return Enum.ContextActionResult.Sink
				end
			end
			return Enum.ContextActionResult.Pass
		end, false, Enum.ContextActionPriority.High.Value, UIT.MouseWheel)
	end)

	real.InputBegan:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseButton1 then
			endWheel()
			announce(fingerBegin(input.Position.X, input.Position.Y), processed)
		elseif not isMouse(t) then
			fires.InputBegan(input, processed)
		end
	end)
	real.InputChanged:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseMovement then
			fingerMove(input.Position.X, input.Position.Y)
		elseif t == UIT.MouseWheel then
			if not finger and not pointerOutside then
				wheelNotch(input, processed)
			end
		elseif not isMouse(t) then
			fires.InputChanged(input, processed)
		end
	end)
	real.InputEnded:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseButton1 then
			fingerEnd(input.Position.X, input.Position.Y)
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
		local signal, fire = M.signal()
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
					fire(fingerBegin(input.Position.X, input.Position.Y).touch)
				elseif not isMouse(t) then
					fire(input)
				end
			end)
		elseif name == "InputChanged" then
			object.InputChanged:Connect(function(input)
				local t = input.UserInputType
				if t == UIT.MouseMovement then
					fingerMove(input.Position.X, input.Position.Y)
					if finger then
						fire(finger.touch)
					end
				elseif not isMouse(t) then
					fire(input)
				end
			end)
		elseif name == "InputEnded" then
			object.InputEnded:Connect(function(input)
				local t = input.UserInputType
				if t == UIT.MouseButton1 then
					local this = fingerEnd(input.Position.X, input.Position.Y)
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
					local this = fingerEnd(input.Position.X, input.Position.Y)
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
	E.proxy = setmetatable({}, {
		__index = function(_, k)
			local override = overrides[k]
			if override ~= nil then
				return override
			end
			if k == "MouseBehavior" then
				return mouseBehavior
			end
			local value = real[k]
			if type(value) == "function" then
				return function(_, ...)
					return real[k](real, ...)
				end
			end
			return value
		end,
		__newindex = function(_, k, v)
			if k == "MouseBehavior" then
				mouseBehavior = v
				return
			end
			real[k] = v
		end,
		__tostring = function()
			return "UserInputService"
		end,
	})
	return E
end

-- The engine on a simulated touch device's client, false anywhere else.
local engine = nil

local function current()
	if engine == nil then
		local real = game:GetService("UserInputService")
		local simulated = game:GetService("RunService"):IsClient() and real.TouchEnabled and not preferredTouch(real)
		engine = if simulated then build() else false
	end
	return engine or nil
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
			if object:IsA("GuiObject") and (name ~= "Activated" or object:IsA("GuiButton")) then
				return e.guiEvent(object, name)
			end
		end
	end
	return object[name]
end

return M
