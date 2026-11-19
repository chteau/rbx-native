-- Tools: StarterPack copied into the Backpack when a character spawns, the
-- server equipping and unequipping through the Humanoid (the events' order,
-- the RightGrip weld), one tool at a time, a tool without its handle, then
-- the client equipping, activating (Activate and a real click), number keys
-- and dropping with Backspace (gestures). Needs a player.
return function(h, origin)
	local StarterPack = game:GetService("StarterPack")
	local player = h.player()
	local result = {}
	local function tool(name, handle)
		local t = Instance.new("Tool")
		t.Name = name
		if handle then
			local part = Instance.new("Part")
			part.Name = "Handle"
			part.Size = Vector3.new(1, 4, 1)
			part.Parent = t
		end
		return t
	end
	local function cf(c)
		return h.vec(c.Position) .. "|" .. h.vec(c.LookVector) .. "|" .. h.vec(c.UpVector)
	end
	local function where(t)
		local parent = t.Parent
		if parent == nil then
			return "nil"
		elseif parent == player.Character then
			return "character"
		elseif parent == player:FindFirstChildOfClass("Backpack") then
			return "backpack"
		end
		return parent.ClassName
	end
	local function gesture(text)
		print("CONFORMANCE_GESTURE " .. text)
		h.beats(30)
	end

	-- The sandbox player's own gear (StarterGear) would take hotbar slots.
	local gear = player:FindFirstChildOfClass("StarterGear")
	if gear then
		gear:ClearAllChildren()
	end
	local starter = tool("ConfStarter", true)
	starter.Parent = StarterPack
	local oldBackpack = player:FindFirstChildOfClass("Backpack")
	player:LoadCharacter()
	h.beats(30)
	local backpack = player:FindFirstChildOfClass("Backpack")
	result.backpack_new = backpack ~= oldBackpack
	local firstCopy = backpack:FindFirstChild("ConfStarter")
	result.copied = firstCopy ~= nil
	player:LoadCharacter()
	h.beats(30)
	backpack = player:FindFirstChildOfClass("Backpack")
	result.copy_replaced = backpack:FindFirstChild("ConfStarter") ~= firstCopy
	local count = 0
	for _, child in backpack:GetChildren() do
		if child.Name == "ConfStarter" then
			count += 1
		end
	end
	result.starter_count = count

	local character = player.Character
	local humanoid = character:WaitForChild("Humanoid")
	local log = {}
	local connections = {}
	local function watch(signal, text)
		table.insert(
			connections,
			signal:Connect(function(...)
				local extra = select("#", ...) > 0 and typeof((...)) or ""
				table.insert(log, text .. (extra ~= "" and ":" .. extra or ""))
			end)
		)
	end
	local a = tool("ConfA", true)
	a.Parent = backpack
	local b = tool("ConfB", true)
	b.Parent = backpack
	h.beats(2)
	for _, t in { a, b } do
		watch(t.Equipped, t.Name .. ".Equipped")
		watch(t.Unequipped, t.Name .. ".Unequipped")
		table.insert(
			connections,
			t.AncestryChanged:Connect(function(_, parent)
				table.insert(log, t.Name .. ".Ancestry:" .. (parent and parent.ClassName or "nil"))
			end)
		)
	end
	table.insert(
		connections,
		character.ChildAdded:Connect(function(c)
			table.insert(log, "character+" .. c.Name)
		end)
	)
	table.insert(
		connections,
		character.ChildRemoved:Connect(function(c)
			table.insert(log, "character-" .. c.Name)
		end)
	)
	humanoid:EquipTool(a)
	table.insert(log, "returned")
	h.beats(30)
	local hand = character:FindFirstChild("RightHand") or character:FindFirstChild("Right Arm")
	local grip = hand and hand:FindFirstChild("RightGrip")
	result.grip = grip and (grip.ClassName .. ":" .. grip.Part0.Name .. ">" .. grip.Part1.Name) or "none"
	result.handle_in_hand = cf(hand.CFrame:ToObjectSpace(a.Handle.CFrame))
	for key, value in h.client("ToolEquip", "grip", a) do
		result["client_" .. key] = value
	end
	table.insert(log, "|")
	humanoid:EquipTool(b)
	table.insert(log, "returned")
	h.beats(2)
	table.insert(log, "|")
	humanoid:UnequipTools()
	table.insert(log, "returned")
	h.beats(2)
	for _, c in connections do
		c:Disconnect()
	end
	result.server_log = table.concat(log, " ")
	result.after_unequip = where(a) .. "," .. where(b)

	local bare = tool("ConfBare", false)
	bare.Parent = backpack
	local bareEquipped = false
	bare.Equipped:Connect(function()
		bareEquipped = true
	end)
	humanoid:EquipTool(bare)
	h.beats(2)
	result.bare = where(bare) .. ":" .. tostring(bareEquipped)
	humanoid:UnequipTools()
	bare:Destroy()
	h.beats(2)

	local activations = {}
	a.Activated:Connect(function()
		table.insert(activations, "Activated")
	end)
	a.Deactivated:Connect(function()
		table.insert(activations, "Deactivated")
	end)
	local equipped = h.client("ToolEquip", "equip", a)
	h.beats(10)
	result.client_equip = equipped.parent
	result.client_equip_on_server = where(a)
	gesture(string.format("click %d %d", equipped.centre_x, equipped.centre_y))
	result.server_activations = table.concat(activations, " ")
	result.client_log = h.client("ToolEquip", "log").log
	h.client("ToolEquip", "unequip")
	h.beats(10)
	result.client_unequip_on_server = where(a)

	gesture("key One")
	local held = character:FindFirstChildOfClass("Tool")
	result.key_one = held and held.Name or "none"
	gesture("key One")
	result.key_one_again = where(held or a)
	gesture("key Two")
	held = character:FindFirstChildOfClass("Tool")
	result.key_two = held and held.Name or "none"
	result.can_be_dropped = tostring(held and held.CanBeDropped)
	gesture("key Backspace")
	result.dropped = held and where(held) or "none"

	for _, t in { a, b, held } do
		if t then
			t:Destroy()
		end
	end
	starter:Destroy()
	player:LoadCharacter()
	h.beats(30)
	return result
end
