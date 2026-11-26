-- GUI at run time: StarterGui copied into PlayerGui when a character spawns
-- (ResetOnSpawn: one that's off only on the first spawn), what the server and the client see of it, the GUI inset,
-- layout, and a real click and typing on the client (gestures): the order
-- of the GUI's and UserInputService's events. Needs a player.
return function(h, origin)
	local StarterGui = game:GetService("StarterGui")
	local player = h.player()
	local result = {}
	local function screen(name, reset)
		local gui = Instance.new("ScreenGui")
		gui.Name = name
		gui.ResetOnSpawn = reset
		gui.IgnoreGuiInset = true
		gui.Parent = StarterGui
		return gui
	end
	local reset = screen("ConfReset", true)
	h.new("TextButton", {
		Name = "Button",
		Position = UDim2.fromOffset(100, 100),
		Size = UDim2.fromOffset(200, 80),
	}, reset)
	h.new("TextBox", {
		Name = "Box",
		Position = UDim2.fromOffset(100, 250),
		Size = UDim2.fromOffset(200, 50),
		Text = "abc",
	}, reset)
	local holder = h.new("Frame", {
		Name = "Holder",
		Position = UDim2.new(0.5, 0, 0, 400),
		AnchorPoint = Vector2.new(0.5, 0),
		Size = UDim2.new(0, 300, 0, 100),
	}, reset)
	h.new("Frame", {
		Name = "Inner",
		Position = UDim2.new(0.5, -10, 1, -30),
		AnchorPoint = Vector2.new(1, 0),
		Size = UDim2.new(0.5, 0, 0, 20),
	}, holder)
	game:GetService("ReplicatedStorage").ConformanceClient.GuiProbe:Clone().Parent = reset
	-- ConfKeep (ResetOnSpawn off) is in StarterGui from the start; one
	-- added now, after the first spawn, is ConfLate.
	local late = screen("ConfLate", false)
	h.beats(5)
	local gui = player:WaitForChild("PlayerGui")
	result.before_spawn = gui:FindFirstChild("ConfReset") ~= nil
	player:LoadCharacter()
	h.beats(10)
	local first = gui:FindFirstChild("ConfReset")
	local keep = gui:FindFirstChild("ConfKeep")
	local names = {}
	for _, child in gui:GetChildren() do
		if string.sub(child.Name, 1, 4) == "Conf" then
			table.insert(names, child.Name)
		end
	end
	table.sort(names)
	result.server_has = table.concat(names, ",")
	player:LoadCharacter()
	h.beats(10)
	result.reset_replaced = gui:FindFirstChild("ConfReset") ~= first and first.Parent == nil
	result.keep_same = gui:FindFirstChild("ConfKeep") == keep
	local keeps = 0
	for _, child in gui:GetChildren() do
		if child.Name == "ConfKeep" then
			keeps += 1
		end
	end
	result.keep_count = keeps
	h.beats(30)
	for key, value in h.client("GuiRuntime", "look") do
		result["client_" .. key] = value
	end
	local at = h.client("GuiRuntime", "watch")
	local function gesture(text)
		print("CONFORMANCE_GESTURE " .. text)
		h.beats(30)
	end
	gesture(string.format("click %d %d", at.button_x, at.button_y))
	gesture(string.format("click %d %d", at.box_x, at.box_y))
	gesture("key H")
	gesture("key I")
	gesture("key Return")
	for key, value in h.client("GuiRuntime", "log") do
		result[key] = value
	end
	reset:Destroy()
	late:Destroy()
	for _, child in gui:GetChildren() do
		if string.sub(child.Name, 1, 4) == "Conf" and child.Name ~= "ConfKeep" then
			child:Destroy()
		end
	end
	h.beats(10)
	return result
end
