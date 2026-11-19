--!nonstrict
-- Our own PlayerModule for Play (local): the default controls and camera
-- of a Roblox client, written from the creator docs and from watching the
-- real client (docs/research/local-run/PLAN.md, Milestone 4). Not Roblox's
-- PlayerModule: a place that ships its own in StarterPlayerScripts keeps
-- it, and this one stays out.
--
-- The same surface games call on Roblox's: require(PlayerModule) gives
-- :GetControls() (Enable, Disable, GetMoveVector) and :GetCameras().

local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local UserInputService = game:GetService("UserInputService")
local ContextActionService = game:GetService("ContextActionService")

local player = Players.LocalPlayer

-- Measured on the real client (see PLAN.md): the follow distance a
-- character starts at, the turn a pixel of mouse motion gives, the height
-- of the focus over the root, shift lock's sideways offset.
local START_ZOOM = 12.5
local START_PITCH = math.rad(-20)
local TURN_PER_PIXEL = math.rad(0.5)
local PITCH_LIMIT = math.rad(80)
local FOCUS_ABOVE_ROOT = 1.5
local SHIFT_LOCK_OFFSET = Vector3.new(1.75, 0, 0)
local ZOOM_PER_NOTCH = 0.15
-- Closer than this the camera is in first person, at the minimum zoom.
local FIRST_PERSON = 1
-- The character fades out between these distances (fully gone in first
-- person).
local FADE_FROM, FADE_TO = 2, 0.5
-- How far in front of a wall the camera stops.
local WALL_MARGIN = 0.25

local PlayerModule = {}

---------------------------------------------------------------- controls --

local Controls = { enabled = false, moveVector = Vector3.zero }
local held = { forward = 0, back = 0, left = 0, right = 0, jump = false }

local MOVE_KEYS = {
	moveForwardAction = { "forward", Enum.KeyCode.W, Enum.KeyCode.Up },
	moveBackwardAction = { "back", Enum.KeyCode.S, Enum.KeyCode.Down },
	moveLeftAction = { "left", Enum.KeyCode.A, Enum.KeyCode.Left },
	moveRightAction = { "right", Enum.KeyCode.D, Enum.KeyCode.Right },
}

local function humanoid()
	local character = player.Character
	return character and character:FindFirstChildOfClass("Humanoid")
end

function Controls:Enable(on)
	if on == false then
		return self:Disable()
	end
	if self.enabled then
		return
	end
	self.enabled = true
	local priority = Enum.ContextActionPriority.Medium.Value
	for action, spec in MOVE_KEYS do
		local slot = spec[1]
		ContextActionService:BindActionAtPriority(action, function(_, state)
			held[slot] = if state == Enum.UserInputState.Begin then 1 else 0
			return Enum.ContextActionResult.Pass
		end, false, priority, spec[2], spec[3])
	end
	ContextActionService:BindActionAtPriority("jumpAction", function(_, state)
		held.jump = state == Enum.UserInputState.Begin
		return Enum.ContextActionResult.Pass
	end, false, priority, Enum.KeyCode.Space)
end

function Controls:Disable()
	if not self.enabled then
		return
	end
	self.enabled = false
	for action in MOVE_KEYS do
		ContextActionService:UnbindAction(action)
	end
	ContextActionService:UnbindAction("jumpAction")
	held.forward, held.back, held.left, held.right, held.jump = 0, 0, 0, 0, false
	self.moveVector = Vector3.zero
	local h = humanoid()
	if h then
		h:Move(Vector3.zero, true)
	end
end

function Controls:GetMoveVector()
	return self.moveVector
end

local function steer()
	if not Controls.enabled then
		return
	end
	local h = humanoid()
	if not h then
		return
	end
	local move = Vector3.new(held.right - held.left, 0, held.back - held.forward)
	if move.Magnitude > 1 then
		move = move.Unit
	end
	Controls.moveVector = move
	h:Move(move, true)
	if held.jump then
		h.Jump = true
	end
end

------------------------------------------------------------------ camera --

local Cameras = {}
local yaw, pitch, zoom = 0, START_PITCH, START_ZOOM
local looking = false
local shiftLock = false
local subject = nil
local faded = {}
local autoRotate = nil

local function zoomLimits()
	local low = player.CameraMinZoomDistance
	local high = math.max(player.CameraMaxZoomDistance, low)
	return low, high
end

local function setZoom(distance)
	local low, high = zoomLimits()
	if player.CameraMode == Enum.CameraMode.LockFirstPerson then
		zoom = low
		return
	end
	if distance < FIRST_PERSON and distance < zoom then
		distance = low
	elseif zoom <= low and distance > zoom then
		distance = math.max(distance, FIRST_PERSON)
	end
	zoom = math.clamp(distance, low, high)
end

local function firstPerson()
	local low = zoomLimits()
	return zoom <= low + 1e-3 and low < FIRST_PERSON
end

local function rootOf(target)
	if target == nil then
		return nil
	end
	if target:IsA("Humanoid") then
		return target.RootPart or (target.Parent and target.Parent:FindFirstChild("HumanoidRootPart"))
	elseif target:IsA("BasePart") then
		return target
	elseif target:IsA("Model") then
		return target.PrimaryPart or target:FindFirstChildWhichIsA("BasePart", true)
	end
	return nil
end

local function focusOf(target, root)
	if target:IsA("Humanoid") then
		return root.Position + Vector3.new(0, FOCUS_ABOVE_ROOT, 0) + target.CameraOffset
	end
	return root.Position
end

local function lookOf()
	return CFrame.fromEulerAnglesYXZ(pitch, yaw, 0)
end

-- How far the camera can sit from `focus` along `back` before a wall: our
-- popper, a ray from the focus past the character and anything nearly
-- invisible.
local function clearance(focus, back, distance, character)
	local ignore = { character }
	local params = RaycastParams.new()
	params.FilterType = Enum.RaycastFilterType.Exclude
	for _ = 1, 8 do
		params.FilterDescendantsInstances = ignore
		local hit = workspace:Raycast(focus, back * (distance + WALL_MARGIN), params)
		if not hit then
			return distance
		end
		if hit.Instance.Transparency < 0.95 and hit.Instance.CanCollide then
			return math.max(hit.Distance - WALL_MARGIN, 0)
		end
		table.insert(ignore, hit.Instance)
	end
	return distance
end

local function fade(character, amount)
	if not character then
		return
	end
	for _, part in character:GetDescendants() do
		if part:IsA("BasePart") then
			if part.LocalTransparencyModifier ~= amount then
				part.LocalTransparencyModifier = amount
			end
			faded[part] = true
		end
	end
end

local function unfade()
	for part in faded do
		if part.Parent then
			part.LocalTransparencyModifier = 0
		end
	end
	table.clear(faded)
end

local function faceCamera(h, root)
	if autoRotate == nil then
		autoRotate = h.AutoRotate
	end
	h.AutoRotate = false
	local flat = Vector3.new(root.Position.X, root.Position.Y, root.Position.Z)
	root.CFrame = CFrame.new(flat) * CFrame.fromEulerAnglesYXZ(0, yaw, 0)
end

local function releaseFacing(h)
	if autoRotate ~= nil and h then
		h.AutoRotate = autoRotate
	end
	autoRotate = nil
end

local function update()
	local camera = workspace.CurrentCamera
	if not camera then
		return
	end
	local kind = camera.CameraType
	local h = humanoid()
	if kind == Enum.CameraType.Scriptable or kind == Enum.CameraType.Fixed then
		UserInputService.MouseBehavior = Enum.MouseBehavior.Default
		releaseFacing(h)
		unfade()
		return
	end
	local target = camera.CameraSubject
	if target == nil and h then
		camera.CameraSubject = h
		target = h
	end
	local root = rootOf(target)
	if not root then
		return
	end
	if target ~= subject then
		subject = target
		local _, facing = root.CFrame:ToEulerAnglesYXZ()
		yaw, pitch = facing, START_PITCH
	end
	if player.CameraMode == Enum.CameraMode.LockFirstPerson then
		setZoom(0)
	end

	local first = firstPerson()
	local locked = first or shiftLock
	if locked then
		UserInputService.MouseBehavior = Enum.MouseBehavior.LockCenter
	elseif looking then
		UserInputService.MouseBehavior = Enum.MouseBehavior.LockCurrentPosition
	else
		UserInputService.MouseBehavior = Enum.MouseBehavior.Default
	end
	if locked or looking then
		local delta = UserInputService:GetMouseDelta()
		yaw -= delta.X * TURN_PER_PIXEL
		pitch = math.clamp(pitch - delta.Y * TURN_PER_PIXEL, -PITCH_LIMIT, PITCH_LIMIT)
	end

	local focus = focusOf(target, root)
	local look = lookOf()
	local back = -look.LookVector
	local character = target:IsA("Humanoid") and target.Parent or nil
	local distance = if first then 0 else clearance(focus, back, zoom, character or root)
	local eye = focus + back * distance
	if shiftLock and not first then
		eye += look:VectorToWorldSpace(SHIFT_LOCK_OFFSET)
	end
	camera.CFrame = CFrame.new(eye) * look
	camera.Focus = CFrame.new(focus)

	if character and distance < FADE_FROM then
		fade(character, math.clamp((FADE_FROM - distance) / (FADE_FROM - FADE_TO), 0, 1))
	else
		unfade()
	end
	if h and locked and target == h then
		faceCamera(h, root)
	else
		releaseFacing(h)
	end
end

UserInputService.InputBegan:Connect(function(input, processed)
	if input.UserInputType == Enum.UserInputType.MouseButton2 and not processed then
		looking = true
	elseif input.UserInputType == Enum.UserInputType.Keyboard and not processed then
		local key = input.KeyCode
		if key == Enum.KeyCode.LeftShift or key == Enum.KeyCode.RightShift then
			if player.DevEnableMouseLock then
				shiftLock = not shiftLock
			end
		elseif key == Enum.KeyCode.I then
			setZoom(zoom * (1 - ZOOM_PER_NOTCH))
		elseif key == Enum.KeyCode.O then
			setZoom(zoom / (1 - ZOOM_PER_NOTCH))
		end
	end
end)

UserInputService.InputEnded:Connect(function(input)
	if input.UserInputType == Enum.UserInputType.MouseButton2 then
		looking = false
	end
end)

UserInputService.InputChanged:Connect(function(input, processed)
	if input.UserInputType == Enum.UserInputType.MouseWheel and not processed then
		setZoom(zoom * (1 - ZOOM_PER_NOTCH) ^ input.Position.Z)
	end
end)

function Cameras:GetShiftLock()
	return shiftLock
end

function Cameras:SetShiftLock(on)
	shiftLock = on == true
end

function PlayerModule:GetControls()
	return Controls
end

function PlayerModule:GetCameras()
	return Cameras
end

Controls:Enable()
RunService:BindToRenderStep("LocalRunControls", Enum.RenderPriority.Input.Value, steer)
RunService:BindToRenderStep("LocalRunCamera", Enum.RenderPriority.Camera.Value, update)

return PlayerModule
