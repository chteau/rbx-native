-- The touch simulation's fingers (see touch_input): the mouse as one finger
-- and the wheel as two, the touches and gesture events they fire at the
-- service and at the GuiObjects under them, and the camera a free finger
-- turns.
local Gesture = require(script.Parent:WaitForChild("__rbxnative_touch_gesture"))

local WHEEL_ACTION = "__rbxnative_touch_wheel"

-- `fires` is the service's signal table; returns the steps the service and
-- GuiObject listeners drive.
return function(real, fires)
	local Players = game:GetService("Players")
	local ContextActionService = game:GetService("ContextActionService")
	local UIT = Enum.UserInputType
	local State = Enum.UserInputState

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
		return Gesture.newTouch({
			UserInputType = UIT.Touch,
			UserInputState = State.Begin,
			KeyCode = Enum.KeyCode.Unknown,
			Position = Vector3.new(x, y, 0),
			Delta = Vector3.zero,
		})
	end
	local function moveTouch(touch, x, y)
		local position = Vector3.new(x, y, 0)
		Gesture.setTouch(touch, "Delta", position - touch.Position)
		Gesture.setTouch(touch, "Position", position)
		Gesture.setTouch(touch, "UserInputState", State.Change)
	end
	local function endTouch(touch)
		Gesture.setTouch(touch, "Delta", Vector3.zero)
		Gesture.setTouch(touch, "UserInputState", State.End)
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
			gesture = Gesture.fingerDown(x, y, os.clock()),
			processed = false,
			hits = hitsAt(x, y),
			announced = false,
		}
		finger = this
		task.delay(Gesture.LONG_PRESS, function()
			if finger == this then
				dispatch(Gesture.fingerHeld(this.gesture), this)
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
			local yaw, pitchTurn = Gesture.touchRotation(dx, dy, (camera.CFrame:ToEulerAnglesYXZ()))
			pcall(function()
				pitchTurn *= UserSettings():GetService("UserGameSettings"):GetCameraYInvertValue()
			end)
			pitchTurn = Gesture.clampPitch(pitchTurn, math.asin(look.Y))
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
		dispatch(Gesture.fingerMove(this.gesture, x, y, os.clock()), this)
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
		Gesture.settle(this.gesture, now)
		if x ~= this.gesture.x or y ~= this.gesture.y then
			turnCamera(this, x, y)
			moveTouch(this.touch, x, y)
			fires.InputChanged(this.touch, this.processed)
			fires.TouchMoved(this.touch, this.processed)
		end
		endTouch(this.touch)
		fires.InputEnded(this.touch, this.processed)
		fires.TouchEnded(this.touch, this.processed)
		dispatch(Gesture.fingerUp(this.gesture, x, y, now), this)
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
			local scale = wheel.span / Gesture.PINCH_SPAN
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
			local x1, y1, x2, y2 = Gesture.pinchPositions(cx, cy, Gesture.PINCH_SPAN, 0)
			wheel = {
				a = newTouch(x1, y1),
				b = newTouch(x2, y2),
				cx = cx,
				cy = cy,
				span = Gesture.PINCH_SPAN,
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
		local before = if rotate then wheel.angle else wheel.span / Gesture.PINCH_SPAN
		wheel.span, wheel.angle = Gesture.wheelStep(wheel.span, wheel.angle, input.Position.Z > 0, rotate)
		local after = if rotate then wheel.angle else wheel.span / Gesture.PINCH_SPAN
		local dt = math.max(now - wheel.at, 1 / 60)
		wheel.at = now
		local x1, y1, x2, y2 = Gesture.pinchPositions(wheel.cx, wheel.cy, wheel.span, wheel.angle)
		for touch, p in { [wheel.a] = { x1, y1 }, [wheel.b] = { x2, y2 } } do
			moveTouch(touch, p[1], p[2])
			fires.InputChanged(touch, processed)
			fires.TouchMoved(touch, processed)
		end
		wheelGesture("Change", (after - before) / dt)
		wheel.token += 1
		local token = wheel.token
		task.delay(Gesture.WHEEL_IDLE, function()
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

	return {
		guiGestures = guiGestures,
		current = function()
			return finger
		end,
		announce = announce,
		fingerBegin = fingerBegin,
		fingerMove = fingerMove,
		fingerEnd = fingerEnd,
		endWheel = endWheel,
		wheelNotch = wheelNotch,
	}
end
