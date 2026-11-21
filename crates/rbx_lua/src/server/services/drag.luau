--!nonstrict
-- DragDetector, UIDragDetector and ClickDetector clicks. Our own, from the
-- creator docs and the DragDetectorApi golden: the axes are the detector's
-- Orientation turned (Axis its X, SecondaryAxis its Y), angles clamp to
-- 180 and forces to 0, and a server reads an identity reference frame
-- while nothing drags.
--
-- On a client a left press on a part (or a model) holding a DragDetector
-- starts a drag; the cursor ray goes to the server unless RunLocally,
-- where the motion is worked out (drag style, limits, constraint
-- functions), applied (Geometric: pivoted; Physical: driven by velocity
-- with Responsiveness, held by the server while it drags; Custom: only
-- the events) and DragStart/DragContinue/DragEnd fire with the player. A
-- press and release on a ClickDetector's part fires its MouseClick. A
-- left press on a GUI object with a UIDragDetector drags the object.
local host = ...

local RunService = game:GetService("RunService")
local Players = game:GetService("Players")
local UserInputService = game:GetService("UserInputService")

local M = {}

local SIGNALS = {
	DragStart = true,
	DragContinue = true,
	DragEnd = true,
	MouseClick = true,
	RightMouseClick = true,
	MouseHoverEnter = true,
	MouseHoverLeave = true,
}
local CLICK_SIGNALS = { MouseClick = true, RightMouseClick = true, MouseHoverEnter = true, MouseHoverLeave = true }
local REACH = 1000

-- Per detector id: the functions scripts handed it.
local extras = {}
-- Drags in progress: by detector id on a server (one player each), the
-- one this client holds on a client.
local drags = {}
local local3d, localUi, pressedClick
local viewportRay
local heartbeat

local function extra(id)
	local e = extras[id]
	if not e then
		e = { constraints = {} }
		extras[id] = e
	end
	return e
end

local function frameOf(detector)
	local o = detector.Orientation
	return CFrame.fromOrientation(math.rad(o.X), math.rad(o.Y), math.rad(o.Z))
end

local function referenceFrame(detector)
	local drag = drags[host.id(detector)]
	if drag then
		return drag.reference
	end
	if host.server() then
		return CFrame.identity
	end
	local reference = detector.ReferenceInstance
	if reference and (reference:IsA("PVInstance")) then
		return reference:GetPivot()
	end
	return CFrame.identity
end

-- The Orientation whose X is `axis`, keeping `other` (its Y) as near as
-- it can.
local function orientationFor(axis, other, axisIsX)
	if axis.Magnitude < 1e-6 then
		return nil
	end
	axis = axis.Unit
	local second = other - axis * axis:Dot(other)
	if second.Magnitude < 1e-6 then
		second = math.abs(axis.Y) < 0.9 and Vector3.yAxis or Vector3.xAxis
		second = (second - axis * axis:Dot(second)).Unit
	else
		second = second.Unit
	end
	local x, y = axis, second
	if not axisIsX then
		x, y = second, axis
	end
	local cf = CFrame.fromMatrix(Vector3.zero, x, y, x:Cross(y))
	local rx, ry, rz = cf:ToOrientation()
	return Vector3.new(math.deg(rx), math.deg(ry), math.deg(rz))
end

local function constraintConnection(id)
	return host.signal(id, "RbxnConstraint"):Connect(function() end)
end

local function addConstraint(self, priority, f)
	local id = host.id(self)
	local connection = constraintConnection(id)
	local list = extra(id).constraints
	table.insert(list, { priority = priority, f = f, connection = connection, order = #list })
	table.sort(list, function(a, b)
		if a.priority ~= b.priority then
			return a.priority < b.priority
		end
		return a.order < b.order
	end)
	return connection
end

local function constrained(id, ...)
	local values = table.pack(...)
	for _, c in extra(id).constraints do
		if c.connection.Connected then
			local out = table.pack(c.f(table.unpack(values, 1, values.n)))
			if out[1] ~= nil then
				values = out
			end
		end
	end
	return table.unpack(values, 1, values.n)
end

-- Camera's screen math, which the drags share: a ray through a viewport
-- pixel (from the camera's position, `depth` studs along it) and back.
-- Screen points count from below the top bar, viewport points from the
-- window's top.
local TOPBAR_INSET = 58

viewportRay = function(camera, x, y, depth)
	local size = camera.ViewportSize
	local cf = camera.CFrame
	local tan = math.tan(math.rad(camera.FieldOfView) / 2)
	local aspect = size.X / math.max(size.Y, 1)
	local px = (2 * x / math.max(size.X, 1) - 1) * aspect * tan
	local py = (1 - 2 * y / math.max(size.Y, 1)) * tan
	local direction = cf:VectorToWorldSpace(Vector3.new(px, py, -1)).Unit
	return Ray.new(cf.Position + direction * (depth or 0), direction)
end

local function toViewport(camera, point)
	local size = camera.ViewportSize
	local tan = math.tan(math.rad(camera.FieldOfView) / 2)
	local aspect = size.X / math.max(size.Y, 1)
	local p = camera.CFrame:PointToObjectSpace(point)
	local depth = -p.Z
	local d = if math.abs(depth) < 1e-6 then 1e-6 else depth
	local x = (p.X / (d * aspect * tan) + 1) / 2 * size.X
	local y = (1 - p.Y / (d * tan)) / 2 * size.Y
	local onScreen = depth > 0 and x >= 0 and y >= 0 and x <= size.X and y <= size.Y
	return Vector3.new(x, y, depth), onScreen
end

local CAMERA = {
	ViewportPointToRay = function(self, x, y, depth)
		return viewportRay(self, x, y, depth)
	end,
	ScreenPointToRay = function(self, x, y, depth)
		return viewportRay(self, x, y + TOPBAR_INSET, depth)
	end,
	WorldToViewportPoint = function(self, point)
		return toViewport(self, point)
	end,
	WorldToScreenPoint = function(self, point)
		local v, onScreen = toViewport(self, point)
		return Vector3.new(v.X, v.Y - TOPBAR_INSET, v.Z), onScreen
	end,
	GetRenderCFrame = function(self)
		return self.CFrame
	end,
	GetRoll = function(self)
		local cf = self.CFrame
		local flat = CFrame.lookAt(Vector3.zero, cf.LookVector)
		local up = flat:VectorToObjectSpace(cf.UpVector)
		return math.atan2(-up.X, up.Y)
	end,
	SetRoll = function(self, angle)
		local cf = self.CFrame
		self.CFrame = CFrame.lookAt(cf.Position, cf.Position + cf.LookVector) * CFrame.Angles(0, 0, angle)
	end,
	-- Backs the camera off along its own look until the box's bounding
	-- sphere fits the narrower field of view; it keeps its rotation and
	-- focuses on the box.
	ZoomToExtents = function(self, cframe, size)
		if typeof(cframe) ~= "CFrame" then
			error("Argument 1 missing or nil", 2)
		end
		if typeof(size) ~= "Vector3" then
			error("Argument 2 missing or nil", 2)
		end
		local viewport = self.ViewportSize
		local half = math.rad(self.FieldOfView) / 2
		local aspect = viewport.X / math.max(viewport.Y, 1)
		local narrow = math.min(half, math.atan(math.tan(half) * aspect))
		local distance = size.Magnitude / 2 / math.sin(narrow)
		local cf = self.CFrame
		self.CFrame = cf.Rotation + cframe.Position - cf.LookVector * distance
		self.Focus = CFrame.new(cframe.Position)
	end,
}

local DRAG_METHODS = {
	GetReferenceFrame = function(self)
		return referenceFrame(self)
	end,
	SetDragStyleFunction = function(self, f)
		extra(host.id(self)).style = f
	end,
	SetPermissionPolicyFunction = function(self, f)
		extra(host.id(self)).permission = f
	end,
	AddConstraintFunction = addConstraint,
	RestartDrag = function(self)
		local drag = drags[host.id(self)] or (local3d and local3d.detector == self and local3d)
		if drag then
			drag.restart = true
		end
	end,
}

local UI_METHODS = {
	GetReferencePosition = function(self)
		local drag = localUi and localUi.detector == self and localUi
		if drag then
			return drag.startPosition
		end
		local parent = self.Parent
		return if parent and parent:IsA("GuiObject") then parent.Position else UDim2.new()
	end,
	GetReferenceRotation = function(self)
		local drag = localUi and localUi.detector == self and localUi
		if drag then
			return drag.startRotation
		end
		local parent = self.Parent
		return if parent and parent:IsA("GuiObject") then parent.Rotation else 0
	end,
	SetDragStyleFunction = function(self, f)
		extra(host.id(self)).style = f
	end,
	AddConstraintFunction = addConstraint,
}

-- Player:GetMouse() ----------------------------------------------------------
-- The local player's PlayerMouse: one per client, read live from the
-- cursor (X/Y below the top bar, Hit/Target from the camera ray, skipping
-- TargetFilter), its signals fired from the client's input.

local MOUSE_SIGNALS = {
	Button1Down = true,
	Button1Up = true,
	Button2Down = true,
	Button2Up = true,
	Move = true,
	WheelForward = true,
	WheelBackward = true,
	Idle = true,
	KeyDown = true,
	KeyUp = true,
}
local mouse, mouseId

local function mouseHit()
	local camera = workspace.CurrentCamera
	local location = UserInputService:GetMouseLocation()
	local ray = viewportRay(camera, location.X, location.Y, 0)
	local params = RaycastParams.new()
	if mouse.TargetFilter then
		params.FilterDescendantsInstances = { mouse.TargetFilter }
	end
	local result = workspace:Raycast(ray.Origin, ray.Direction * REACH, params)
	local at = if result then result.Position else ray.Origin + ray.Direction * REACH
	return ray, at, result
end

local function surfaceOf(part, normal)
	local n = part.CFrame:VectorToObjectSpace(normal)
	local x, y, z = math.abs(n.X), math.abs(n.Y), math.abs(n.Z)
	if x >= y and x >= z then
		return if n.X > 0 then Enum.NormalId.Right else Enum.NormalId.Left
	elseif y >= z then
		return if n.Y > 0 then Enum.NormalId.Top else Enum.NormalId.Bottom
	end
	return if n.Z > 0 then Enum.NormalId.Back else Enum.NormalId.Front
end

local MOUSE = {
	Hit = function()
		local ray, at = mouseHit()
		return CFrame.lookAt(at, at + ray.Direction)
	end,
	Target = function()
		local _, _, result = mouseHit()
		return result and result.Instance
	end,
	TargetSurface = function()
		local _, _, result = mouseHit()
		return if result and result.Instance:IsA("BasePart")
			then surfaceOf(result.Instance, result.Normal)
			else Enum.NormalId.Right
	end,
	UnitRay = function()
		local ray = mouseHit()
		return ray
	end,
	Origin = function()
		local ray = mouseHit()
		return CFrame.lookAt(ray.Origin, ray.Origin + ray.Direction)
	end,
	X = function()
		return UserInputService:GetMouseLocation().X
	end,
	Y = function()
		return UserInputService:GetMouseLocation().Y - TOPBAR_INSET
	end,
	ViewSizeX = function()
		return workspace.CurrentCamera.ViewportSize.X
	end,
	ViewSizeY = function()
		return workspace.CurrentCamera.ViewportSize.Y - TOPBAR_INSET
	end,
}

local function getMouse(player)
	if host.server() or player ~= Players.LocalPlayer then
		return nil
	end
	if not mouse then
		mouse, mouseId = host.new("PlayerMouse", "Instance")
	end
	return mouse
end

local function mouseInput(input, processed)
	if not mouse then
		return
	end
	local kind = input.UserInputType
	local state = input.UserInputState
	local began = state == Enum.UserInputState.Begin
	if kind == Enum.UserInputType.MouseMovement then
		host.fire(mouseId, "Move")
	elseif kind == Enum.UserInputType.MouseWheel and not processed then
		host.fire(mouseId, if input.Position.Z > 0 then "WheelForward" else "WheelBackward")
	elseif kind == Enum.UserInputType.MouseButton1 and not (processed and began) then
		host.fire(mouseId, if began then "Button1Down" else "Button1Up")
	elseif kind == Enum.UserInputType.MouseButton2 and not (processed and began) then
		host.fire(mouseId, if began then "Button2Down" else "Button2Up")
	elseif kind == Enum.UserInputType.Keyboard and not processed then
		local key = UserInputService:GetStringForKeyCode(input.KeyCode):lower()
		host.fire(mouseId, if began then "KeyDown" else "KeyUp", key)
	end
end

function M.member(class, id, key)
	if class == "Player" then
		if key == "GetMouse" then
			return getMouse, true
		end
		return nil, false
	end
	if class == "PlayerMouse" then
		if MOUSE_SIGNALS[key] then
			return host.signal(id, key), true
		end
		local f = MOUSE[key]
		if f then
			return f(), true
		end
		return nil, false
	end
	if class == "Camera" then
		if key == "InterpolationFinished" then
			return host.signal(id, key), true
		end
		local f = CAMERA[key]
		return f, f ~= nil
	end
	if SIGNALS[key] and (class ~= "UIDragDetector" or not CLICK_SIGNALS[key]) then
		return host.signal(id, key), true
	end
	if class == "ClickDetector" then
		return nil, false
	end
	if class == "UIDragDetector" then
		local f = UI_METHODS[key]
		return f, f ~= nil
	end
	local f = DRAG_METHODS[key]
	return f, f ~= nil
end

-- Reads go through here for the computed axes: `instance` is the detector.
local function axisRead(detector, key)
	local frame = frameOf(detector)
	local vector = if key == "Axis" or key == "WorldAxis" then frame.RightVector else frame.UpVector
	if key == "WorldAxis" or key == "WorldSecondaryAxis" then
		vector = referenceFrame(detector):VectorToWorldSpace(vector)
	end
	return vector
end

local AXES = { Axis = true, SecondaryAxis = true, WorldAxis = true, WorldSecondaryAxis = true }

local base = M.member
function M.member(class, id, key)
	if class == "DragDetector" and AXES[key] then
		return axisRead(host.instance(id), key), true
	end
	return base(class, id, key)
end

function M.set(class, id, key, value)
	if class ~= "DragDetector" then
		return false
	end
	local detector = host.instance(id)
	if AXES[key] then
		if typeof(value) ~= "Vector3" then
			return false
		end
		local world = key == "WorldAxis" or key == "WorldSecondaryAxis"
		if world then
			value = referenceFrame(detector):VectorToObjectSpace(value)
		end
		local isX = key == "Axis" or key == "WorldAxis"
		local frame = frameOf(detector)
		local o = orientationFor(value, if isX then frame.UpVector else frame.RightVector, isX)
		if o then
			detector.Orientation = o
		end
		return true
	end
	if type(value) ~= "number" then
		return false
	end
	if key == "MaxDragAngle" or key == "MinDragAngle" then
		return false, math.clamp(value, -180, 180)
	end
	if key == "MaxForce" or key == "MaxTorque" then
		return false, math.max(value, 0)
	end
	return false
end

-- The motion -----------------------------------------------------------

local function limited(value, low, high)
	if low < high then
		return math.clamp(value, low, high)
	end
	return value
end

local function limitTranslation(detector, v)
	local low, high = detector.MinDragTranslation, detector.MaxDragTranslation
	return Vector3.new(limited(v.X, low.X, high.X), limited(v.Y, low.Y, high.Y), limited(v.Z, low.Z, high.Z))
end

local function planeHit(origin, direction, point, normal)
	local d = direction:Dot(normal)
	if math.abs(d) < 1e-6 then
		return nil
	end
	local t = (point - origin):Dot(normal) / d
	if t <= 1e-6 then
		return nil
	end
	return origin + direction * t
end

-- The point on the line through `point` along `axis` nearest the ray.
local function lineHit(origin, direction, point, axis)
	local w = point - origin
	local b = axis:Dot(direction)
	local denom = 1 - b * b
	if denom < 1e-6 then
		return nil
	end
	local s = (b * direction:Dot(w) - axis:Dot(w)) / denom
	return point + axis * s
end

local function signedAngle(a, b, axis)
	return math.atan2(axis:Dot(a:Cross(b)), a:Dot(b))
end

local function styleOf(drag)
	local style = drag.detector.DragStyle
	local switched = drag.switched
	if style == Enum.DragDetectorDragStyle.TranslatePlaneOrLine then
		return if switched then "line" else "plane"
	elseif style == Enum.DragDetectorDragStyle.TranslateLineOrPlane then
		return if switched then "plane" else "line"
	elseif style == Enum.DragDetectorDragStyle.TranslateLine then
		return "line"
	elseif style == Enum.DragDetectorDragStyle.TranslateViewPlane then
		return "view"
	elseif style == Enum.DragDetectorDragStyle.RotateAxis then
		return "rotate"
	elseif style == Enum.DragDetectorDragStyle.RotateTrackball then
		return "trackball"
	elseif style == Enum.DragDetectorDragStyle.Scriptable then
		return "script"
	end
	return "plane"
end

-- The drag motion for `ray` in the reference frame (DragFrame), or the
-- world pivot a scripted style asked for.
local function motion(drag, ray, view)
	local detector = drag.detector
	local reference = drag.reference.Rotation
	local axis = reference:VectorToWorldSpace(frameOf(detector).RightVector)
	local origin, direction = ray.Origin, ray.Direction.Unit
	local kind = styleOf(drag)
	if kind == "script" then
		local f = extra(host.id(detector)).style
		if not f then
			return nil
		end
		local target = f(ray, view)
		if typeof(target) ~= "CFrame" then
			return nil
		end
		return nil, target
	end
	if kind == "rotate" or kind == "trackball" then
		local center = drag.startPivot.Position
		local normal = if kind == "rotate" then axis else view.LookVector
		local now = planeHit(origin, direction, center, normal)
		local was = planeHit(drag.startRay.Origin, drag.startRay.Direction.Unit, center, normal)
		if not (now and was) or (now - center).Magnitude < 1e-4 or (was - center).Magnitude < 1e-4 then
			return drag.last
		end
		local a, b = was - center, now - center
		if kind == "rotate" then
			local angle = math.deg(signedAngle(a, b, axis))
			angle = limited(angle, detector.MinDragAngle, detector.MaxDragAngle)
			local localAxis = reference:VectorToObjectSpace(axis)
			return CFrame.fromAxisAngle(localAxis, math.rad(angle))
		end
		local turn = a:Cross(b)
		if turn.Magnitude < 1e-6 then
			return CFrame.identity
		end
		local angle = math.atan2(turn.Magnitude, a:Dot(b)) * detector.TrackballRadialPullFactor
		return CFrame.fromAxisAngle(reference:VectorToObjectSpace(turn.Unit), angle)
	end
	local hit
	if kind == "line" then
		hit = lineHit(origin, direction, drag.startHit, axis)
	else
		local normal = if kind == "view" then view.LookVector else axis
		hit = planeHit(origin, direction, drag.startHit, normal)
	end
	if not hit then
		return drag.last
	end
	local offset = reference:VectorToObjectSpace(hit - drag.startHit)
	return CFrame.new(limitTranslation(detector, offset))
end

local function targetOf(detector)
	local parent = detector.Parent
	if parent and parent:IsA("Model") then
		return parent
	end
	return parent
end

local function rootOf(target)
	if target:IsA("Model") then
		local primary = target.PrimaryPart or target:FindFirstChildWhichIsA("BasePart", true)
		return primary and primary.AssemblyRootPart or primary
	end
	return target.AssemblyRootPart or target
end

local function physical(drag)
	local detector = drag.detector
	if detector.ResponseStyle ~= Enum.DragDetectorResponseStyle.Physical then
		return false
	end
	local root = drag.root
	return root ~= nil and not root.Anchored
end

local function apply(drag, ray, view)
	local detector = drag.detector
	local id = host.id(detector)
	local dragFrame, world = motion(drag, ray, view)
	if dragFrame then
		dragFrame = constrained(id, dragFrame) or dragFrame
		drag.last = dragFrame
		detector.DragFrame = dragFrame
		local at = CFrame.new(drag.startPivot.Position) * drag.reference.Rotation
		world = at * dragFrame * at:Inverse() * drag.startPivot
	elseif world then
		world = constrained(id, world) or world
	end
	if not world or detector.ResponseStyle == Enum.DragDetectorResponseStyle.Custom then
		return
	end
	if physical(drag) then
		drag.goal = world
	else
		drag.target:PivotTo(world)
	end
end

-- Physical response: each frame the assembly is pushed toward the goal.
local function drive(drag, dt)
	local root, goal = drag.root, drag.goal
	if not (root and goal) or root.Anchored then
		return
	end
	local detector = drag.detector
	local pivot = drag.target:GetPivot()
	local rate = math.max(detector.Responsiveness, 0)
	local velocity = (goal.Position - pivot.Position) * rate
	local mass = math.max(root.AssemblyMass, 1e-3)
	local change = velocity - root.AssemblyLinearVelocity
	local most = detector.MaxForce / mass * math.max(dt, 1 / 240)
	if change.Magnitude > most then
		velocity = root.AssemblyLinearVelocity + change.Unit * most
	end
	root.AssemblyLinearVelocity = velocity
	local turn = goal.Rotation * pivot.Rotation:Inverse()
	local axis, angle = turn:ToAxisAngle()
	if angle > math.pi then
		angle -= 2 * math.pi
	end
	if detector.MaxTorque > 0 and math.abs(angle) > 1e-4 then
		root.AssemblyAngularVelocity = axis * angle * rate
	end
end

local function step(dt)
	for _, drag in drags do
		if physical(drag) then
			drive(drag, dt)
		end
	end
	if local3d and local3d.runLocal and physical(local3d) then
		drive(local3d, dt)
	end
end

local function ensureHeartbeat()
	if not heartbeat then
		heartbeat = RunService.Heartbeat:Connect(step)
	end
end

local function newDrag(detector, part, ray, view, hit)
	local target = targetOf(detector)
	if not target or not target:IsA("PVInstance") then
		return nil
	end
	local reference = CFrame.new(target:GetPivot().Position)
	local referenceInstance = detector.ReferenceInstance
	if referenceInstance and referenceInstance:IsA("PVInstance") then
		reference = referenceInstance:GetPivot()
	end
	return {
		detector = detector,
		part = part,
		target = target,
		root = rootOf(target),
		reference = reference,
		startPivot = target:GetPivot(),
		startRay = ray,
		startHit = hit,
		last = CFrame.identity,
		view = view,
	}
end

local function permitted(detector, player, part)
	local policy = detector.PermissionPolicy
	if policy == Enum.DragDetectorPermissionPolicy.Nobody then
		return false
	end
	if policy == Enum.DragDetectorPermissionPolicy.Scriptable then
		local f = extra(host.id(detector)).permission
		if f then
			return f(player, part) and true or false
		end
	end
	return detector.Enabled
end

local function begin(drag, player, ray, view, hitFrame)
	local detector = drag.detector
	if physical(drag) and host.server() then
		pcall(drag.root.SetNetworkOwner, drag.root, nil)
	end
	ensureHeartbeat()
	host.fire(host.id(detector), "DragStart", player, ray, view, hitFrame, drag.part, CFrame.identity, drag.switched)
end

local function finish(drag, player)
	local detector = drag.detector
	if physical(drag) and host.server() then
		pcall(drag.root.SetNetworkOwnershipAuto, drag.root)
	end
	host.fire(host.id(detector), "DragEnd", player)
end

-- The server's half: a client's ray, as `send` packed it.
local function unpackRay(n)
	local ray = Ray.new(Vector3.new(n[1], n[2], n[3]), Vector3.new(n[4], n[5], n[6]))
	local view = CFrame.new(n[7], n[8], n[9], n[10], n[11], n[12], n[13], n[14], n[15], n[16], n[17], n[18])
	return ray, view, Vector3.new(n[19] or 0, n[20] or 0, n[21] or 0), (n[22] or 0) ~= 0
end

function M.remote(player, phase, detector, part, numbers)
	local id = host.id(detector)
	if phase == "click" then
		if detector:IsA("ClickDetector") then
			host.fire(id, numbers[1] == 2 and "RightMouseClick" or "MouseClick", player)
		end
		return
	end
	if not detector:IsA("DragDetector") then
		return
	end
	local ray, view, hit, switched = unpackRay(numbers)
	local drag = drags[id]
	if phase == "start" then
		if drag or not permitted(detector, player, part) then
			return
		end
		drag = newDrag(detector, part, ray, view, hit)
		if not drag then
			return
		end
		drag.player = player
		drag.switched = switched
		drags[id] = drag
		begin(drag, player, ray, view, CFrame.new(hit))
	elseif not drag or drag.player ~= player then
		return
	elseif phase == "continue" then
		drag.switched = switched
		apply(drag, ray, view)
		host.fire(id, "DragContinue", player, ray, view, CFrame.identity, switched)
	elseif phase == "end" then
		drags[id] = nil
		finish(drag, player)
	end
end

-- The client's half --------------------------------------------------------

local function cursorRay()
	local camera = workspace.CurrentCamera
	local location = UserInputService:GetMouseLocation()
	return viewportRay(camera, location.X, location.Y, 0), camera.CFrame
end

local function find(part, class)
	local node = part
	while node and node ~= workspace do
		local found = node:FindFirstChildWhichIsA(class)
		if found then
			return found
		end
		node = node.Parent
	end
	return nil
end

local function reach(detector, point)
	local camera = workspace.CurrentCamera
	local player = Players.LocalPlayer
	local character = player and player.Character
	local from = character and character:FindFirstChild("HumanoidRootPart")
	local origin = if from then from.Position else camera.CFrame.Position
	return (point - origin).Magnitude <= detector.MaxActivationDistance
end

local function modeSwitch(detector)
	return UserInputService:IsKeyDown(detector.KeyboardModeSwitchKeyCode)
end

local function pack(ray, view, hit, switched)
	local n = { ray.Origin.X, ray.Origin.Y, ray.Origin.Z, ray.Direction.X, ray.Direction.Y, ray.Direction.Z }
	for _, v in { view:GetComponents() } do
		table.insert(n, v)
	end
	table.insert(n, hit.X)
	table.insert(n, hit.Y)
	table.insert(n, hit.Z)
	table.insert(n, switched and 1 or 0)
	return n
end

local function pick()
	local ray, view = cursorRay()
	local result = workspace:Raycast(ray.Origin, ray.Direction * REACH)
	return result, ray, view
end

local function press3d(button)
	local result, ray, view = pick()
	if not result or not result.Instance then
		return
	end
	local part = result.Instance
	local player = Players.LocalPlayer
	if button == 1 then
		local detector = find(part, "DragDetector")
		if detector and detector.Enabled and reach(detector, result.Position) then
			if detector.PermissionPolicy == Enum.DragDetectorPermissionPolicy.Nobody then
				return
			end
			local drag = newDrag(detector, part, ray, view, result.Position)
			if drag then
				drag.switched = modeSwitch(detector)
				drag.runLocal = detector.RunLocally
				local3d = drag
				if drag.runLocal then
					begin(drag, player, ray, view, CFrame.new(result.Position))
				else
					ensureHeartbeat()
					host.fire(host.id(detector), "DragStart", player, ray, view, CFrame.new(result.Position), part, CFrame.identity, drag.switched)
					host.send_drag("start", detector, part, pack(ray, view, result.Position, drag.switched))
				end
				return
			end
		end
	end
	local click = find(part, "ClickDetector")
	if click and not click:IsA("DragDetector") and reach(click, result.Position) then
		pressedClick = { detector = click, button = button }
	end
end

local function release3d(button)
	if button == 1 and local3d then
		local drag = local3d
		local3d = nil
		local player = Players.LocalPlayer
		if drag.runLocal then
			finish(drag, player)
		else
			host.fire(host.id(drag.detector), "DragEnd", player)
			host.send_drag("end", drag.detector, drag.part, pack(drag.startRay, drag.view, Vector3.zero, false))
		end
		return
	end
	local pressed = pressedClick
	pressedClick = nil
	if not pressed or pressed.button ~= button then
		return
	end
	local result = pick()
	if result and find(result.Instance, "ClickDetector") == pressed.detector then
		host.send_drag("click", pressed.detector, result.Instance, { button })
		host.fire(host.id(pressed.detector), if button == 2 then "RightMouseClick" else "MouseClick", Players.LocalPlayer)
	end
end

local function move3d()
	local drag = local3d
	if not drag then
		return
	end
	local ray, view = cursorRay()
	drag.view = view
	drag.switched = modeSwitch(drag.detector)
	local player = Players.LocalPlayer
	if drag.runLocal then
		apply(drag, ray, view)
	else
		host.send_drag("continue", drag.detector, drag.part, pack(ray, view, Vector3.zero, drag.switched))
	end
	host.fire(host.id(drag.detector), "DragContinue", player, ray, view, CFrame.identity, drag.switched)
end

-- UIDragDetector -------------------------------------------------------------

local function inside(object, point)
	local p, s = object.AbsolutePosition, object.AbsoluteSize
	return point.X >= p.X and point.Y >= p.Y and point.X <= p.X + s.X and point.Y <= p.Y + s.Y
end

local function visible(object)
	local node = object
	while node do
		if node:IsA("GuiObject") and not node.Visible then
			return false
		end
		if node:IsA("LayerCollector") then
			return node.Enabled ~= false
		end
		node = node.Parent
	end
	return false
end

local function pressUi(point)
	local player = Players.LocalPlayer
	local playerGui = player and player:FindFirstChildOfClass("PlayerGui")
	if not playerGui then
		return false
	end
	local best, bestZ
	for _, detector in host.of_class("UIDragDetector") do
		local object = detector.Parent
		local shown = object and object:IsA("GuiObject") and object:IsDescendantOf(playerGui) and visible(object)
		if detector.Enabled and shown and inside(object, point) then
			local z = object.ZIndex
			if not best or z >= bestZ then
				best, bestZ = detector, z
			end
		end
	end
	if not best then
		return false
	end
	local object = best.Parent
	localUi = {
		detector = best,
		object = object,
		start = point,
		startPosition = object.Position,
		startRotation = object.Rotation,
		startAbsolute = object.AbsolutePosition,
		center = object.AbsolutePosition + object.AbsoluteSize / 2,
	}
	host.fire(host.id(best), "DragStart", point)
	return true
end

local function limitUDim(value, low, high)
	return UDim2.new(
		limited(value.X.Scale, low.X.Scale, high.X.Scale),
		limited(value.X.Offset, low.X.Offset, high.X.Offset),
		limited(value.Y.Scale, low.Y.Scale, high.Y.Scale),
		limited(value.Y.Offset, low.Y.Offset, high.Y.Offset)
	)
end

local function moveUi(point)
	local drag = localUi
	local detector, object = drag.detector, drag.object
	local id = host.id(detector)
	local delta = point - drag.start
	local style = detector.DragStyle
	local response = detector.ResponseStyle
	local offset, rotation = UDim2.new(), 0
	if style == Enum.UIDragDetectorDragStyle.Rotate then
		local a, b = drag.start - drag.center, point - drag.center
		if a.Magnitude > 1e-3 and b.Magnitude > 1e-3 then
			rotation = math.deg(math.atan2(a.X * b.Y - a.Y * b.X, a:Dot(b)))
			rotation = limited(rotation, detector.MinDragAngle, detector.MaxDragAngle)
		end
	elseif style == Enum.UIDragDetectorDragStyle.Scriptable then
		local f = extra(id).style
		local position, turned = nil, nil
		if f then
			position, turned = f()
		end
		offset = if typeof(position) == "UDim2" then position else UDim2.new()
		rotation = if type(turned) == "number" then turned else 0
	else
		if style == Enum.UIDragDetectorDragStyle.TranslateLine then
			local axis = detector.DragAxis
			axis = if axis.Magnitude > 1e-6 then axis.Unit else Vector2.xAxis
			delta = axis * delta:Dot(axis)
		end
		local bounding = detector.BoundingUI
		if bounding and bounding:IsA("GuiBase2d") then
			local low = bounding.AbsolutePosition - drag.startAbsolute
			local high = low + bounding.AbsoluteSize - object.AbsoluteSize
			delta = Vector2.new(limited(delta.X, low.X, high.X), limited(delta.Y, low.Y, high.Y))
		end
		local scaled = response == Enum.UIDragDetectorResponseStyle.Scale
			or response == Enum.UIDragDetectorResponseStyle.CustomScale
		if scaled then
			local parent = object.Parent
			local size = if parent and parent:IsA("GuiBase2d") then parent.AbsoluteSize else workspace.CurrentCamera.ViewportSize
			offset = UDim2.fromScale(delta.X / math.max(size.X, 1), delta.Y / math.max(size.Y, 1))
		else
			offset = UDim2.fromOffset(delta.X, delta.Y)
		end
		offset = limitUDim(offset, detector.MinDragTranslation, detector.MaxDragTranslation)
	end
	local position, turned = constrained(id, drag.startPosition + offset, drag.startRotation + rotation)
	position = position or (drag.startPosition + offset)
	turned = turned or (drag.startRotation + rotation)
	detector.DragUDim2 = position - drag.startPosition
	detector.DragRotation = turned - drag.startRotation
	if response == Enum.UIDragDetectorResponseStyle.Offset or response == Enum.UIDragDetectorResponseStyle.Scale then
		object.Position = position
		object.Rotation = turned
	end
	host.fire(id, "DragContinue", point)
end

-- Every client input, after the game's own bindings had it.
function M.input(input, processed)
	if host.server() then
		return
	end
	mouseInput(input, processed)
	local kind = input.UserInputType
	local state = input.UserInputState
	local point = Vector2.new(input.Position.X, input.Position.Y)
	local button = if kind == Enum.UserInputType.MouseButton1 then 1 elseif kind == Enum.UserInputType.MouseButton2 then 2 else nil
	if button and state == Enum.UserInputState.Begin then
		if processed then
			return
		end
		if button == 1 and pressUi(point) then
			return
		end
		press3d(button)
	elseif button and state == Enum.UserInputState.End then
		if button == 1 and localUi then
			local drag = localUi
			localUi = nil
			host.fire(host.id(drag.detector), "DragEnd", point)
			return
		end
		release3d(button)
	elseif kind == Enum.UserInputType.MouseMovement then
		if localUi then
			moveUi(point)
		else
			move3d()
		end
	end
end

return M
