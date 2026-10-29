-- The pure half of the touch stand-in (see touch_input): the finger and
-- wheel recognisers, the camera's touch rotation, signals, the stand-in
-- InputObject and the keyboard cover's values. Nothing here reads the
-- engine, so it runs outside Roblox too.
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

-- The keyboard cover ---------------------------------------------------------------

M.KEYBOARD_PROPS = { "OnScreenKeyboardVisible", "OnScreenKeyboardPosition", "OnScreenKeyboardSize" }

-- The OnScreenKeyboard* values for a cover at (x, y), w by h client pixels;
-- an empty one is a keyboard that is down. Vectors are { x, y } here, so
-- the logic runs outside Roblox; the proxy hands out Vector2s.
function M.keyboardProps(x, y, w, h)
	if w <= 0 or h <= 0 then
		x, y, w, h = 0, 0, 0, 0
	end
	return {
		OnScreenKeyboardVisible = w > 0,
		OnScreenKeyboardPosition = { x, y },
		OnScreenKeyboardSize = { w, h },
	}
end

-- The properties that differ between two `keyboardProps`, in a fixed order.
function M.keyboardChanges(old, new)
	local changed = {}
	for _, name in M.KEYBOARD_PROPS do
		local a, b = old[name], new[name]
		local same = if type(a) == "table" then a[1] == b[1] and a[2] == b[2] else a == b
		if not same then
			table.insert(changed, name)
		end
	end
	return changed
end

return M
