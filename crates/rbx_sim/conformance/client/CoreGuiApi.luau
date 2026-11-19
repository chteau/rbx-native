-- CoreGuiApi on the client.
local StarterGui = game:GetService("StarterGui")
local GuiService = game:GetService("GuiService")

return function(ch)
	-- "ok:<value>", the error, or "yields" when it hasn't returned after
	-- 30 beats.
	local function try(f)
		local result
		task.spawn(function()
			local ok, got = pcall(f)
			result = if ok then "ok:" .. tostring(got) else (tostring(got):gsub("^.-:%d+: ", ""))
		end)
		for _ = 1, 30 do
			if result then
				break
			end
			ch.beat()
		end
		return result or "yields"
	end
	local err, value = try, try
	local r = {}
	local function enabled(kind)
		return tostring(StarterGui:GetCoreGuiEnabled(kind))
	end
	local types = {}
	for _, kind in { "Backpack", "Health", "PlayerList", "Chat", "EmotesMenu", "All" } do
		table.insert(types, kind .. "=" .. enabled(Enum.CoreGuiType[kind]))
	end
	r.defaults = table.concat(types, " ")
	StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.Backpack, false)
	r.backpack_off = enabled(Enum.CoreGuiType.Backpack) .. " all=" .. enabled(Enum.CoreGuiType.All)
	StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.All, false)
	r.all_off = enabled(Enum.CoreGuiType.Health) .. " " .. enabled(Enum.CoreGuiType.All)
	StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.All, true)
	r.all_on = enabled(Enum.CoreGuiType.Backpack) .. " " .. enabled(Enum.CoreGuiType.All)
	r.set_bad_type = err(function()
		StarterGui:SetCoreGuiEnabled("Backpack", false)
	end)
	StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.All, true)
	r.notify = err(function()
		StarterGui:SetCore("SendNotification", { Title = "Conformance", Text = "notify", Duration = 0.5 })
	end)
	r.set_unknown = err(function()
		StarterGui:SetCore("ConformanceBogus", 1)
	end)
	r.get_unknown = err(function()
		return StarterGui:GetCore("ConformanceBogus")
	end)
	r.topbar = value(function()
		return StarterGui:GetCore("TopbarEnabled")
	end)
	r.reset_callback = err(function()
		StarterGui:SetCore("ResetButtonCallback", true)
	end)
	r.get_reset_callback = value(function()
		return StarterGui:GetCore("ResetButtonCallback")
	end)
	local gui = game:GetService("Players").LocalPlayer.PlayerGui
	r.topbar_transparency = value(function()
		return gui:GetTopbarTransparency()
	end)
	r.menu_open = tostring(GuiService.MenuIsOpen)
	r.selected = tostring(GuiService.SelectedObject)
	r.ten_foot = tostring(GuiService:IsTenFootInterface())
	return r
end
