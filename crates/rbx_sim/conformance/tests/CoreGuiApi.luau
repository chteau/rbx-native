-- StarterGui's core GUI switches and SetCore/GetCore, PlayerGui's top bar
-- and GuiService basics, on the server and on the client. Needs a player.
return function(h, origin)
	local StarterGui = game:GetService("StarterGui")
	local GuiService = game:GetService("GuiService")
	local shared = require(game:GetService("ReplicatedStorage").ConformanceClient.Shared)
	local result = {}
	result.server_get_enabled = shared.errOrYield(function()
		return StarterGui:GetCoreGuiEnabled(Enum.CoreGuiType.Backpack)
	end, h.beat)
	result.server_set_enabled = shared.errOrYield(function()
		StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.Backpack, true)
	end, h.beat)
	result.server_set_core = shared.errOrYield(function()
		StarterGui:SetCore("TopbarEnabled", true)
	end, h.beat)
	result.server_get_core = shared.errOrYield(function()
		return StarterGui:GetCore("TopbarEnabled")
	end, h.beat)
	result.server_inset = shared.errOrYield(function()
		return GuiService:GetGuiInset()
	end, h.beat)
	for key, value in h.client("CoreGuiApi") do
		result["client_" .. key] = value
	end
	return result
end
