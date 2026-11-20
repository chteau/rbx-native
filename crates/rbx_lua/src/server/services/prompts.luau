--!nonstrict
-- ProximityPrompt and ProximityPromptService. Our own, from the creator
-- docs: on a client, each frame the enabled prompts in the workspace within
-- MaxActivationDistance of the character (and in sight of the camera when
-- RequiresLineOfSight) are candidates; OnePerButton keeps the closest per
-- key, OneGlobally the closest of all, AlwaysShow every one, up to
-- MaxPromptsVisible. A shown prompt fires PromptShown (and PromptHidden when
-- it goes), and with the Default style gets a BillboardGui in the player's
-- ProximityPrompts ScreenGui. Its key (or InputHoldBegin) holds it:
-- PromptButtonHoldBegan, Triggered once HoldDuration has passed (at once
-- when it is 0), TriggerEnded and PromptButtonHoldEnded on release; each
-- reaches the server, where the prompt's and the service's signals fire
-- with the player.
local host = ...

local Players = game:GetService("Players")
local UserInputService = game:GetService("UserInputService")
local TextService = game:GetService("TextService")

local M = {}

local PROMPT_SIGNALS = {
	Triggered = true,
	TriggerEnded = true,
	PromptButtonHoldBegan = true,
	PromptButtonHoldEnded = true,
	PromptShown = true,
	PromptHidden = true,
}

-- Client state: the prompts in the workspace, the ones shown (with their
-- billboard), and the hold in progress.
local known = {}
local shown = {}
local holding
local watching = false
local screen

local function service()
	return game:GetService("ProximityPromptService")
end

local function fire(prompt, name, ...)
	host.fire(host.id(prompt), name, ...)
	host.fire(host.id(service()), name == "Triggered" and "PromptTriggered"
		or name == "TriggerEnded" and "PromptTriggerEnded"
		or name, prompt, ...)
end

local function position(prompt)
	local parent = prompt.Parent
	if not parent then
		return nil
	elseif parent:IsA("Attachment") then
		return parent.WorldPosition, parent.Parent
	elseif parent:IsA("BasePart") then
		return parent.Position, parent
	elseif parent:IsA("Model") then
		return parent:GetPivot().Position, parent
	end
	return nil
end

local function watch()
	watching = true
	for _, d in workspace:GetDescendants() do
		if d:IsA("ProximityPrompt") then
			known[d] = true
		end
	end
	workspace.DescendantAdded:Connect(function(d)
		if d:IsA("ProximityPrompt") then
			known[d] = true
		end
	end)
	workspace.DescendantRemoving:Connect(function(d)
		known[d] = nil
	end)
end

-- The default look: a dark rounded card with the key in a ring on the
-- left, the object over the action on the right.
local function billboard(prompt, adornee)
	if not screen or not screen.Parent then
		local playerGui = Players.LocalPlayer:FindFirstChildOfClass("PlayerGui")
		if not playerGui then
			return nil
		end
		screen = Instance.new("ScreenGui")
		screen.Name = "ProximityPrompts"
		screen.ResetOnSpawn = false
		screen.Parent = playerGui
	end
	local action = prompt.ActionText
	local object = prompt.ObjectText
	local actionWidth = TextService:GetTextSize(action, 19, Enum.Font.GothamMedium, Vector2.new(1000, 72)).X
	local objectWidth = TextService:GetTextSize(object, 14, Enum.Font.GothamMedium, Vector2.new(1000, 72)).X
	local width = 72 + math.max(actionWidth, objectWidth) + 24

	local gui = Instance.new("BillboardGui")
	gui.Name = "Prompt"
	gui.AlwaysOnTop = true
	gui.Active = true
	gui.Size = UDim2.fromOffset(width, 72)
	gui.SizeOffset = Vector2.new(prompt.UIOffset.X / width, prompt.UIOffset.Y / 72)
	gui.Adornee = adornee

	local card = Instance.new("Frame")
	card.Name = "Frame"
	card.Size = UDim2.fromScale(1, 1)
	card.BackgroundColor3 = Color3.new(0.07, 0.07, 0.07)
	card.BackgroundTransparency = 0.2
	card.BorderSizePixel = 0
	card.Parent = gui
	Instance.new("UICorner", card).CornerRadius = UDim.new(0, 9)

	local ring = Instance.new("Frame")
	ring.Name = "Key"
	ring.AnchorPoint = Vector2.new(0.5, 0.5)
	ring.Position = UDim2.fromOffset(36, 36)
	ring.Size = UDim2.fromOffset(40, 40)
	ring.BackgroundTransparency = 1
	ring.Parent = card
	Instance.new("UICorner", ring).CornerRadius = UDim.new(0.5, 0)
	local stroke = Instance.new("UIStroke", ring)
	stroke.Color = Color3.new(1, 1, 1)
	stroke.Thickness = 2

	local key = Instance.new("TextLabel")
	key.Name = "KeyLabel"
	key.Size = UDim2.fromScale(1, 1)
	key.BackgroundTransparency = 1
	key.Font = Enum.Font.GothamMedium
	key.TextSize = 20
	key.TextColor3 = Color3.new(1, 1, 1)
	key.Text = UserInputService:GetStringForKeyCode(prompt.KeyboardKeyCode):upper()
	key.Parent = ring

	local progress = Instance.new("Frame")
	progress.Name = "Progress"
	progress.AnchorPoint = Vector2.new(0, 1)
	progress.Position = UDim2.new(0, 0, 1, 6)
	progress.Size = UDim2.fromOffset(0, 3)
	progress.BackgroundColor3 = Color3.new(1, 1, 1)
	progress.BorderSizePixel = 0
	progress.Parent = ring

	local label = function(name, text, size, color, y)
		local l = Instance.new("TextLabel")
		l.Name = name
		l.Position = UDim2.fromOffset(72, y)
		l.Size = UDim2.new(1, -84, 0, size + 4)
		l.BackgroundTransparency = 1
		l.Font = Enum.Font.GothamMedium
		l.TextSize = size
		l.TextColor3 = color
		l.TextXAlignment = Enum.TextXAlignment.Left
		l.Text = text
		l.Parent = card
	end
	if object ~= "" then
		label("ObjectText", object, 14, Color3.new(0.7, 0.7, 0.7), 14)
		label("ActionText", action, 19, Color3.new(1, 1, 1), 34)
	else
		label("ActionText", action, 19, Color3.new(1, 1, 1), 24)
	end
	gui.Parent = screen
	return gui
end

local function remote(phase, prompt)
	host.send_drag(phase, prompt, nil, {})
end

local function endHold()
	local hold = holding
	holding = nil
	if not hold then
		return
	end
	local player = Players.LocalPlayer
	if hold.triggered then
		fire(hold.prompt, "TriggerEnded", player)
		remote("prompt_trigger_end", hold.prompt)
	end
	if hold.prompt.HoldDuration > 0 then
		fire(hold.prompt, "PromptButtonHoldEnded", player)
		remote("prompt_hold_end", hold.prompt)
	end
	local ui = shown[hold.prompt]
	local bar = ui and ui:FindFirstChild("Progress", true)
	if bar then
		bar.Size = UDim2.fromOffset(0, 3)
	end
end

local function trigger(hold)
	hold.triggered = true
	fire(hold.prompt, "Triggered", Players.LocalPlayer)
	remote("prompt_trigger", hold.prompt)
end

local function beginHold(prompt)
	if holding or not shown[prompt] or not prompt.Enabled then
		return
	end
	holding = { prompt = prompt, start = host.clock(), triggered = false }
	if prompt.HoldDuration > 0 then
		fire(prompt, "PromptButtonHoldBegan", Players.LocalPlayer)
		remote("prompt_hold_begin", prompt)
	else
		trigger(holding)
	end
end

local function hide(prompt)
	if holding and holding.prompt == prompt then
		endHold()
	end
	local ui = shown[prompt]
	shown[prompt] = nil
	if typeof(ui) == "Instance" then
		ui:Destroy()
	end
	host.fire(host.id(prompt), "PromptHidden")
	host.fire(host.id(service()), "PromptHidden", prompt)
end

local function candidates()
	local player = Players.LocalPlayer
	local character = player and player.Character
	local root = character and character:FindFirstChild("HumanoidRootPart")
	local camera = workspace.CurrentCamera
	if not root or not camera then
		return {}
	end
	local list = {}
	for prompt in known do
		local at, parent = nil, nil
		if prompt.Enabled and prompt:IsDescendantOf(workspace) then
			at, parent = position(prompt)
		end
		local distance = at and (at - root.Position).Magnitude
		if distance and distance <= prompt.MaxActivationDistance then
			local _, onScreen = camera:WorldToViewportPoint(at)
			local visible = onScreen
			if visible and prompt.RequiresLineOfSight then
				local params = RaycastParams.new()
				params.FilterDescendantsInstances = { character, parent }
				local from = camera.CFrame.Position
				visible = workspace:Raycast(from, at - from, params) == nil
			end
			if visible then
				table.insert(list, { prompt = prompt, distance = distance })
			end
		end
	end
	table.sort(list, function(a, b)
		return a.distance < b.distance
	end)
	local chosen, keys, global = {}, {}, false
	local most = service().MaxPromptsVisible
	for _, c in list do
		local exclusivity = c.prompt.Exclusivity
		local key = c.prompt.KeyboardKeyCode
		local take = if exclusivity == Enum.ProximityPromptExclusivity.AlwaysShow
			then true
			elseif exclusivity == Enum.ProximityPromptExclusivity.OneGlobally then not global and next(keys) == nil
			else not keys[key] and not global
		if take and #chosen < most then
			table.insert(chosen, c.prompt)
			keys[key] = true
			if exclusivity == Enum.ProximityPromptExclusivity.OneGlobally then
				global = true
			end
		end
	end
	return chosen
end

-- Once a frame on a client.
function M.step()
	if host.server() or not Players.LocalPlayer then
		return
	end
	if not watching and game:IsLoaded() then
		watch()
	end
	if next(known) == nil and next(shown) == nil then
		return
	end
	local wanted = {}
	if service().Enabled then
		for _, prompt in candidates() do
			wanted[prompt] = true
		end
	end
	for prompt in shown do
		if not wanted[prompt] then
			hide(prompt)
		end
	end
	for prompt in wanted do
		if not shown[prompt] then
			local ui = prompt.Style == Enum.ProximityPromptStyle.Default and billboard(prompt, prompt.Parent)
			shown[prompt] = ui or true
			local kind = UserInputService:GetLastInputType()
			host.fire(host.id(prompt), "PromptShown", kind)
			host.fire(host.id(service()), "PromptShown", prompt, kind)
		end
	end
	local hold = holding
	if hold and not hold.triggered and hold.prompt.HoldDuration > 0 then
		local done = (host.clock() - hold.start) / hold.prompt.HoldDuration
		local ui = shown[hold.prompt]
		local bar = typeof(ui) == "Instance" and ui:FindFirstChild("Progress", true)
		if bar then
			bar.Size = UDim2.fromOffset(40 * math.min(done, 1), 3)
		end
		if done >= 1 then
			trigger(hold)
		end
	end
end

function M.input(input, processed)
	if host.server() or input.UserInputType ~= Enum.UserInputType.Keyboard then
		return
	end
	if input.UserInputState == Enum.UserInputState.Begin then
		if processed then
			return
		end
		for prompt in shown do
			if prompt.KeyboardKeyCode == input.KeyCode then
				beginHold(prompt)
				return
			end
		end
	elseif input.UserInputState == Enum.UserInputState.End then
		if holding and holding.prompt.KeyboardKeyCode == input.KeyCode then
			endHold()
		end
	end
end

-- A client's hold or trigger arriving at the server.
function M.remote(player, phase, prompt)
	local name = ({
		prompt_hold_begin = "PromptButtonHoldBegan",
		prompt_hold_end = "PromptButtonHoldEnded",
		prompt_trigger = "Triggered",
		prompt_trigger_end = "TriggerEnded",
	})[phase]
	if name then
		fire(prompt, name, player)
	end
end

local METHODS = {
	InputHoldBegin = function(self)
		beginHold(self)
	end,
	InputHoldEnd = function(self)
		if holding and holding.prompt == self then
			endHold()
		end
	end,
}

local SERVICE_SIGNALS = {
	PromptShown = true,
	PromptHidden = true,
	PromptTriggered = true,
	PromptTriggerEnded = true,
	PromptButtonHoldBegan = true,
	PromptButtonHoldEnded = true,
}

function M.member(class, id, key)
	if class == "ProximityPromptService" then
		if SERVICE_SIGNALS[key] then
			return host.signal(id, key), true
		end
		return nil, false
	end
	if PROMPT_SIGNALS[key] then
		return host.signal(id, key), true
	end
	local f = METHODS[key]
	return f, f ~= nil
end

return M
