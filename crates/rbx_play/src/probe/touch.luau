-- Stand-in touch controls for a simulated touch device.
--
-- Sober runs touch devices with `touch_mode` on, so `TouchEnabled` is true,
-- but every input reaches the game as a mouse: `PreferredInput` stays
-- KeyboardAndMouse and the PlayerModule never shows its TouchGui (it keys
-- on `PreferredInput == Touch`). This module draws the same controls and
-- drives them with the mouse: Roblox's dynamic thumbstick (press in its
-- region and drag) and jump button, with the sizes, positions and sheet
-- images of the PlayerModule's DynamicThumbstick and TouchJump. The
-- instances carry Roblox's names (TouchGui > TouchControlFrame >
-- JumpButton, DynamicThumbstickFrame) so game scripts that look them up
-- find them. They stand down as soon as a real touch makes PreferredInput
-- Touch and the PlayerModule takes over.

local M = {}

local SHEET = "rbxasset://textures/ui/Input/TouchControlsSheetV2.png"
local MIDDLE_TRANSPARENCIES = { 1 - 0.89, 1 - 0.70, 1 - 0.60, 1 - 0.50, 1 - 0.40, 1 - 0.30, 1 - 0.25 }
local NUM_MIDDLE = #MIDDLE_TRANSPARENCIES
local ACTION = "__rbxnative_touch_stick"

-- DynamicThumbstick's sizes: doubled once the screen's short side passes 500.
function M.metrics(width, height)
	local s = math.min(width, height) > 500 and 2 or 1
	return {
		thumbstickSize = 45 * s,
		ringSize = 20 * s,
		middleSize = 10 * s,
		middleSpacing = 14 * s,
		deadZone = 2 * s,
		maxSpeed = 20 * s,
	}
end

-- The region a press must start in: the lower 40% in portrait, the lower
-- left 40% x 2/3 in landscape. Returns x, y, w, h in screen pixels.
function M.stickRegion(width, height)
	if width < height then
		return 0, height * 0.6, width, height * 0.4
	end
	return 0, height / 3, width * 0.4, height * 2 / 3
end

-- TouchJump's button: 70 px on a small screen (short side <= 500), else 120.
-- Returns size and the UDim2 offsets of its Position (scales are 1, 1).
function M.jumpLayout(width, height)
	local small = math.min(width, height) <= 500
	local size = small and 70 or 120
	local x = -(size * 1.5 - 10)
	local y = small and (-size - 20) or (-size * 1.75)
	return size, x, y
end

-- DynamicThumbstick:DoMove: a scaled radial dead zone, full speed at
-- maxSpeed pixels. Returns the camera-relative x, z of the move vector.
function M.moveVector(dx, dy, m)
	local magnitude = math.sqrt(dx * dx + dy * dy)
	if magnitude < m.deadZone then
		return 0, 0
	end
	local scale = (1 - math.max(0, (m.maxSpeed - magnitude) / m.maxSpeed)) / magnitude
	return dx * scale, dy * scale
end

-- Roblox's jump button needs a humanoid that can jump.
function M.canJump(useJumpPower, jumpPower, jumpHeight, jumpingEnabled)
	local power = if useJumpPower then jumpPower else jumpHeight
	return jumpingEnabled and power > 0
end

function M.start()
	local Players = game:GetService("Players")
	local UserInputService = game:GetService("UserInputService")
	local GuiService = game:GetService("GuiService")
	local RunService = game:GetService("RunService")
	local TweenService = game:GetService("TweenService")
	local ContextActionService = game:GetService("ContextActionService")

	local function preferredTouch()
		local ok, pref = pcall(function()
			return UserInputService.PreferredInput == Enum.PreferredInput.Touch
		end)
		return ok and pref
	end
	-- Only on a simulated touch device; a real touch screen gets Roblox's own.
	if not UserInputService.TouchEnabled or preferredTouch() then
		return
	end

	local player = Players.LocalPlayer
	local playerGui = player:WaitForChild("PlayerGui")
	local fade = TweenInfo.new(0.15, Enum.EasingStyle.Quad, Enum.EasingDirection.InOut)

	local gui = Instance.new("ScreenGui")
	gui.Name = "TouchGui"
	gui.ResetOnSpawn = false
	gui.ZIndexBehavior = Enum.ZIndexBehavior.Sibling
	gui.Enabled = false
	gui:SetAttribute("RbxNativeSimulated", true)
	local controlFrame = Instance.new("Frame")
	controlFrame.Name = "TouchControlFrame"
	controlFrame.Size = UDim2.new(1, 0, 1, 0)
	controlFrame.BackgroundTransparency = 1
	controlFrame.Parent = gui

	local stickFrame = Instance.new("Frame")
	stickFrame.Name = "DynamicThumbstickFrame"
	stickFrame.BorderSizePixel = 0
	stickFrame.BackgroundTransparency = 1
	stickFrame.BackgroundColor3 = Color3.fromRGB(0, 0, 0)
	stickFrame.Active = false
	stickFrame.Parent = controlFrame

	local function image(name, z)
		local i = Instance.new("ImageLabel")
		i.Name = name
		i.BackgroundTransparency = 1
		i.Image = SHEET
		i.ImageRectOffset = Vector2.new(1, 1)
		i.ImageRectSize = Vector2.new(144, 144)
		i.AnchorPoint = Vector2.new(0.5, 0.5)
		i.ZIndex = z
		i.Parent = stickFrame
		return i
	end
	local startImage = image("ThumbstickStart", 10)
	startImage.ImageColor3 = Color3.new(0, 0, 0)
	local endImage = image("ThumbstickEnd", 10)
	local middles = {}
	for i = 1, NUM_MIDDLE do
		middles[i] = image("ThumbstickMiddle", 9)
		middles[i].Visible = false
		middles[i].ImageTransparency = MIDDLE_TRANSPARENCIES[i]
	end

	local jumpButton = Instance.new("ImageButton")
	jumpButton.Name = "JumpButton"
	jumpButton.BackgroundTransparency = 1
	jumpButton.Image = SHEET
	jumpButton.ImageRectOffset = Vector2.new(1, 146)
	jumpButton.ImageRectSize = Vector2.new(144, 144)
	jumpButton.Visible = false
	jumpButton.Parent = controlFrame

	local m = M.metrics(1, 1)
	local firstTouch = true
	local function layout()
		local size = gui.AbsoluteSize
		if size.X <= 0 then
			return
		end
		m = M.metrics(size.X, size.Y)
		local x, y, w, h = M.stickRegion(size.X, size.Y)
		stickFrame.Position = UDim2.new(0, x, 0, y)
		stickFrame.Size = UDim2.new(0, w, 0, h)
		if firstTouch then
			-- Before the first use, a hint ring sits at the default spot.
			startImage.Position = UDim2.new(0, m.ringSize * 3.3, 1, -m.ringSize * 2.8)
			startImage.Size = UDim2.new(0, m.ringSize * 3.7, 0, m.ringSize * 3.7)
			endImage.Position = startImage.Position
			endImage.Size = UDim2.new(0, m.thumbstickSize * 0.8, 0, m.thumbstickSize * 0.8)
		end
		local jump, jx, jy = M.jumpLayout(size.X, size.Y)
		jumpButton.Size = UDim2.new(0, jump, 0, jump)
		jumpButton.Position = UDim2.new(1, jx, 1, jy)
	end
	gui:GetPropertyChangedSignal("AbsoluteSize"):Connect(layout)

	local stickStart = nil
	local moveX, moveZ = 0, 0
	local jumping = false

	local function fadeStick(visible)
		local function to(target, value)
			TweenService:Create(target, fade, { ImageTransparency = value }):Play()
		end
		to(startImage, visible and 0 or 1)
		to(endImage, visible and 0.2 or 1)
		for i = 1, NUM_MIDDLE do
			to(middles[i], visible and MIDDLE_TRANSPARENCIES[i] or 1)
		end
	end

	-- DynamicThumbstick:LayoutMiddleImages, in stickFrame coordinates.
	local function moveStick(position)
		local origin = stickFrame.AbsolutePosition
		local startPos = stickStart - origin
		local endPos = position - origin
		endImage.Position = UDim2.new(0, endPos.X, 0, endPos.Y)
		local vector = endPos - startPos
		local startDist = m.thumbstickSize / 2 + m.middleSize
		local available = vector.Magnitude - m.ringSize / 2 - m.middleSize
		local direction = vector.Magnitude > 0 and vector.Unit or Vector2.zero
		local spacing = m.middleSpacing
		if spacing * NUM_MIDDLE < available then
			spacing = available / NUM_MIDDLE
		end
		for i = 1, NUM_MIDDLE do
			local current = startDist + spacing * (i - 1)
			if startDist + spacing * (i - 2) < available then
				local pos = endPos - direction * current
				local exposed = math.clamp(1 - (current - available) / spacing, 0, 1)
				middles[i].Visible = true
				middles[i].Position = UDim2.new(0, pos.X, 0, pos.Y)
				middles[i].Size = UDim2.new(0, m.middleSize * exposed, 0, m.middleSize * exposed)
			else
				middles[i].Visible = false
			end
		end
	end

	local function endStick()
		stickStart = nil
		moveX, moveZ = 0, 0
		fadeStick(false)
	end

	local function inRegion(position)
		local origin = stickFrame.AbsolutePosition
		local size = stickFrame.AbsoluteSize
		return position.X >= origin.X
			and position.Y >= origin.Y
			and position.X <= origin.X + size.X
			and position.Y <= origin.Y + size.Y
	end

	-- A press on the game's own buttons stays theirs, as a touch would.
	local function overButton(position)
		local ok, hits = pcall(function()
			return playerGui:GetGuiObjectsAtPosition(position.X, position.Y)
		end)
		if not ok then
			return false
		end
		for _, hit in hits do
			if not hit:IsDescendantOf(gui) and (hit:IsA("GuiButton") or hit:IsA("TextBox")) then
				return true
			end
		end
		return false
	end

	local function onPress(_, state, input)
		if state ~= Enum.UserInputState.Begin then
			return Enum.ContextActionResult.Pass
		end
		local position = Vector2.new(input.Position.X, input.Position.Y)
		if stickStart or not stickFrame.Visible or not inRegion(position) or overButton(position) then
			return Enum.ContextActionResult.Pass
		end
		if firstTouch then
			firstTouch = false
			local grow = TweenInfo.new(0.5, Enum.EasingStyle.Quad, Enum.EasingDirection.Out)
			TweenService:Create(startImage, grow, { Size = UDim2.new(0, 0, 0, 0) }):Play()
			TweenService:Create(endImage, grow, {
				Size = UDim2.new(0, m.thumbstickSize, 0, m.thumbstickSize),
				ImageColor3 = Color3.new(0, 0, 0),
			}):Play()
		end
		stickStart = position
		local local_ = position - stickFrame.AbsolutePosition
		startImage.Position = UDim2.new(0, local_.X, 0, local_.Y)
		endImage.Position = startImage.Position
		fadeStick(true)
		moveStick(position)
		-- Sunk, as a touch on the stick is: no tool activation, no click.
		return Enum.ContextActionResult.Sink
	end

	UserInputService.InputChanged:Connect(function(input)
		if stickStart and input.UserInputType == Enum.UserInputType.MouseMovement then
			local position = Vector2.new(input.Position.X, input.Position.Y)
			local delta = position - stickStart
			moveX, moveZ = M.moveVector(delta.X, delta.Y, m)
			moveStick(position)
		end
	end)
	UserInputService.InputEnded:Connect(function(input)
		if input.UserInputType == Enum.UserInputType.MouseButton1 then
			if stickStart then
				endStick()
			end
			if jumping then
				jumping = false
				jumpButton.ImageRectOffset = Vector2.new(1, 146)
			end
		end
	end)
	jumpButton.InputBegan:Connect(function(input)
		if input.UserInputType == Enum.UserInputType.MouseButton1 then
			jumping = true
			jumpButton.ImageRectOffset = Vector2.new(146, 146)
		end
	end)
	GuiService.MenuOpened:Connect(function()
		if stickStart then
			endStick()
		end
		jumping = false
	end)

	local bound = false
	local function bind(on)
		if on == bound then
			return
		end
		bound = on
		if on then
			ContextActionService:BindActionAtPriority(
				ACTION,
				onPress,
				false,
				Enum.ContextActionPriority.High.Value,
				Enum.UserInputType.MouseButton1
			)
		else
			ContextActionService:UnbindAction(ACTION)
			if stickStart then
				endStick()
			end
		end
	end

	-- After the PlayerModule's "ControlScriptRenderstep" (Input priority),
	-- which moves the humanoid by the keyboard's vector every frame.
	RunService:BindToRenderStep(ACTION, Enum.RenderPriority.Input.Value + 1, function()
		local character = player.Character
		local humanoid = character and character:FindFirstChildOfClass("Humanoid")
		if not humanoid or not gui.Enabled then
			return
		end
		if moveX ~= 0 or moveZ ~= 0 then
			humanoid:Move(Vector3.new(moveX, 0, moveZ), true)
		end
		if jumping and jumpButton.Visible then
			humanoid.Jump = true
		end
	end)

	-- ControlModule:UpdateTouchGuiVisibility and TouchJump:UpdateEnabled.
	local function refresh()
		if preferredTouch() then
			bind(false)
			gui:Destroy()
			return false
		end
		local character = player.Character
		local humanoid = character and character:FindFirstChildOfClass("Humanoid")
		local show = humanoid ~= nil and GuiService.TouchControlsEnabled and not UserInputService.ModalEnabled
		gui.Enabled = show
		local mode = player.DevTouchMovementMode
		stickFrame.Visible = show
			and mode ~= Enum.DevTouchMovementMode.Scriptable
			and mode ~= Enum.DevTouchMovementMode.ClickToMove
		bind(stickFrame.Visible)
		jumpButton.Visible = show
			and M.canJump(
				humanoid.UseJumpPower,
				humanoid.JumpPower,
				humanoid.JumpHeight,
				humanoid:GetStateEnabled(Enum.HumanoidStateType.Jumping)
			)
		if not jumpButton.Visible then
			jumping = false
		end
		return true
	end

	gui.Parent = playerGui
	layout()
	task.spawn(function()
		while true do
			local ok, alive = pcall(refresh)
			if ok and not alive then
				return
			end
			task.wait(0.25)
		end
	end)
end

return M
