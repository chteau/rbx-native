-- ToolEquip on the client: equips a tool through the Humanoid, activates
-- and deactivates it, and logs what its events did.
local log = {}

return function(ch, step, tool)
	local player = game:GetService("Players").LocalPlayer
	local humanoid = player.Character:FindFirstChildOfClass("Humanoid")
	if step == "equip" then
		log = {}
		tool.Equipped:Connect(function(mouse)
			table.insert(log, "Equipped:" .. typeof(mouse))
		end)
		tool.Unequipped:Connect(function()
			table.insert(log, "Unequipped")
		end)
		tool.Activated:Connect(function()
			table.insert(log, "Activated")
		end)
		tool.Deactivated:Connect(function()
			table.insert(log, "Deactivated")
		end)
		humanoid:EquipTool(tool)
		table.insert(log, "returned")
		ch.beats(10)
		tool:Activate()
		table.insert(log, "activate")
		ch.beats(2)
		tool:Deactivate()
		table.insert(log, "deactivate")
		ch.beats(2)
		table.insert(log, "|")
		local size = workspace.CurrentCamera.ViewportSize
		return {
			parent = tostring(tool.Parent == player.Character),
			centre_x = math.floor(size.X / 2),
			centre_y = math.floor(size.Y / 2),
		}
	elseif step == "grip" then
		local function vec(v)
			local function r(x)
				return math.floor(x * 100 + 0.5) / 100
			end
			return string.format("%s,%s,%s", r(v.X), r(v.Y), r(v.Z))
		end
		local function cf(c)
			return vec(c.Position) .. "|" .. vec(c.LookVector) .. "|" .. vec(c.UpVector)
		end
		local character = player.Character
		local hand = character:FindFirstChild("RightHand") or character:FindFirstChild("Right Arm")
		local grip = hand:FindFirstChild("RightGrip")
		return {
			grip = grip and (grip.ClassName .. ":" .. grip.Part0.Name .. ">" .. grip.Part1.Name) or "none",
			grip_c0 = grip and cf(grip.C0) or "none",
			grip_c1 = grip and cf(grip.C1) or "none",
			handle_in_hand = cf(hand.CFrame:ToObjectSpace(tool.Handle.CFrame)),
		}
	elseif step == "unequip" then
		humanoid:UnequipTools()
		ch.beats(2)
		return {}
	end
	return { log = table.concat(log, " ") }
end
