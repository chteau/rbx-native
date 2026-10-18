-- Touch input for game scripts on a simulated touch device.
--
-- Sober hands every input to the game as a mouse, so a game's own touch
-- code never runs on a simulated phone. Game scripts' UserInputService
-- lookups are wrapped (rbx_play::touch) in `wrap`, whose proxy turns the
-- mouse into one finger:
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
-- - TouchEnabled reads true, MouseEnabled and KeyboardEnabled false,
--   PreferredInput Touch and GetLastInputType Touch.
--
-- Keyboard, gamepad and text input pass through unchanged. On the server,
-- off a simulated touch device, and once a real touch makes PreferredInput
-- Touch, `wrap` returns the real service.

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

-- Proxy --------------------------------------------------------------------------

local TOUCH_META = {
	__index = {
		IsModifierKeyDown = function()
			return false
		end,
	},
	__tostring = function()
		return "InputObject"
	end,
}

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

local function preferredTouch(real)
	local ok, pref = pcall(function()
		return real.PreferredInput == Enum.PreferredInput.Touch
	end)
	return ok and pref
end

local function build(real)
	local UIT = Enum.UserInputType
	local fires = {}
	local overrides = {
		TouchEnabled = true,
		MouseEnabled = false,
		KeyboardEnabled = false,
	}
	pcall(function()
		overrides.PreferredInput = Enum.PreferredInput.Touch
	end)
	for _, name in SIGNALS do
		local signal, fire = M.signal()
		overrides[name] = signal
		fires[name] = fire
	end

	local function vec3(x, y)
		return Vector3.new(x, y, 0)
	end
	local function newTouch(x, y)
		return setmetatable({
			UserInputType = UIT.Touch,
			UserInputState = Enum.UserInputState.Begin,
			KeyCode = Enum.KeyCode.Unknown,
			Position = vec3(x, y),
			Delta = Vector3.zero,
		}, TOUCH_META)
	end
	local function touchBegan(touch, processed)
		fires.InputBegan(touch, processed)
		fires.TouchStarted(touch, processed)
	end
	local function touchMoved(touch, x, y, processed)
		local position = vec3(x, y)
		touch.Delta = position - touch.Position
		touch.Position = position
		touch.UserInputState = Enum.UserInputState.Change
		fires.InputChanged(touch, processed)
		fires.TouchMoved(touch, processed)
	end
	local function touchEnded(touch, processed)
		touch.Delta = Vector3.zero
		touch.UserInputState = Enum.UserInputState.End
		fires.InputEnded(touch, processed)
		fires.TouchEnded(touch, processed)
	end

	local lastType = UIT.Touch
	overrides.GetLastInputType = function()
		return lastType
	end
	local function isMouse(t)
		return t == UIT.MouseButton1
			or t == UIT.MouseButton2
			or t == UIT.MouseButton3
			or t == UIT.MouseMovement
			or t == UIT.MouseWheel
	end
	local function setLastType(t)
		if t ~= lastType then
			lastType = t
			fires.LastInputTypeChanged(t)
		end
	end
	real.LastInputTypeChanged:Connect(function(t)
		setLastType(if isMouse(t) then UIT.Touch else t)
	end)

	-- The mouse finger.
	local finger = nil -- { touch, gesture, processed }

	local function dispatch(events, processed)
		for _, e in events do
			local positions = { Vector2.new(e.x or 0, e.y or 0) }
			if e.kind == "Tap" then
				fires.TouchTap(positions, processed)
			elseif e.kind == "LongPress" then
				fires.TouchLongPress(positions, Enum.UserInputState[e.state], processed)
			elseif e.kind == "Pan" then
				fires.TouchPan(
					positions,
					Vector2.new(e.tx, e.ty),
					Vector2.new(e.vx, e.vy),
					Enum.UserInputState[e.state],
					processed
				)
			elseif e.kind == "Swipe" then
				fires.TouchSwipe(Enum.SwipeDirection[e.direction], 1, processed)
			end
		end
	end

	-- The wheel's two fingers.
	local wheel = nil -- { a, b, cx, cy, span, angle, rotate, processed, value, at, token }

	local function wheelGesture(state, velocity)
		local positions = {
			Vector2.new(wheel.a.Position.X, wheel.a.Position.Y),
			Vector2.new(wheel.b.Position.X, wheel.b.Position.Y),
		}
		local s = Enum.UserInputState[state]
		if wheel.rotate then
			fires.TouchRotate(positions, wheel.angle, velocity, s, wheel.processed)
		else
			fires.TouchPinch(positions, wheel.span / M.PINCH_SPAN, velocity, s, wheel.processed)
		end
	end
	local function endWheel()
		if not wheel then
			return
		end
		wheelGesture("End", 0)
		touchEnded(wheel.a, wheel.processed)
		touchEnded(wheel.b, wheel.processed)
		wheel = nil
	end
	local function wheelNotch(input, processed)
		local rotate = real:IsKeyDown(Enum.KeyCode.LeftControl) or real:IsKeyDown(Enum.KeyCode.RightControl)
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
				at = now,
				token = 0,
			}
			touchBegan(wheel.a, processed)
			touchBegan(wheel.b, processed)
			wheelGesture("Begin", 0)
		end
		local before = if rotate then wheel.angle else wheel.span / M.PINCH_SPAN
		wheel.span, wheel.angle = M.wheelStep(wheel.span, wheel.angle, input.Position.Z > 0, rotate)
		local after = if rotate then wheel.angle else wheel.span / M.PINCH_SPAN
		local dt = math.max(now - wheel.at, 1 / 60)
		wheel.at = now
		local x1, y1, x2, y2 = M.pinchPositions(wheel.cx, wheel.cy, wheel.span, wheel.angle)
		touchMoved(wheel.a, x1, y1, processed)
		touchMoved(wheel.b, x2, y2, processed)
		wheelGesture("Change", (after - before) / dt)
		wheel.token += 1
		local token = wheel.token
		task.delay(M.WHEEL_IDLE, function()
			if wheel and wheel.token == token then
				endWheel()
			end
		end)
	end

	real.InputBegan:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseButton1 then
			endWheel()
			local x, y = input.Position.X, input.Position.Y
			local this = { touch = newTouch(x, y), gesture = M.fingerDown(x, y, os.clock()), processed = processed }
			finger = this
			touchBegan(this.touch, processed)
			task.delay(M.LONG_PRESS, function()
				if finger == this then
					dispatch(M.fingerHeld(this.gesture), this.processed)
				end
			end)
		elseif not isMouse(t) then
			fires.InputBegan(input, processed)
		end
	end)
	real.InputChanged:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseMovement then
			if finger then
				local x, y = input.Position.X, input.Position.Y
				touchMoved(finger.touch, x, y, finger.processed)
				dispatch(M.fingerMove(finger.gesture, x, y, os.clock()), finger.processed)
			end
		elseif t == UIT.MouseWheel then
			if not finger then
				wheelNotch(input, processed)
			end
		elseif not isMouse(t) then
			fires.InputChanged(input, processed)
		end
	end)
	real.InputEnded:Connect(function(input, processed)
		local t = input.UserInputType
		if t == UIT.MouseButton1 then
			local this = finger
			if not this then
				return
			end
			finger = nil
			local x, y = input.Position.X, input.Position.Y
			local now = os.clock()
			M.settle(this.gesture, now)
			if x ~= this.gesture.x or y ~= this.gesture.y then
				touchMoved(this.touch, x, y, this.processed)
			end
			touchEnded(this.touch, this.processed)
			dispatch(M.fingerUp(this.gesture, x, y, now), this.processed)
		elseif not isMouse(t) then
			fires.InputEnded(input, processed)
		end
	end)

	return setmetatable({}, {
		__index = function(_, k)
			local override = overrides[k]
			if override ~= nil then
				return override
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
			real[k] = v
		end,
		__tostring = function()
			return "UserInputService"
		end,
	})
end

local proxy = nil

function M.wrap(real)
	if real == nil then
		return nil
	end
	if not game:GetService("RunService"):IsClient() or not real.TouchEnabled or preferredTouch(real) then
		return real
	end
	if proxy == nil then
		proxy = build(real)
	end
	return proxy
end

return M
