-- ClientApis on the client.
local ContextActionService = game:GetService("ContextActionService")
local GuiService = game:GetService("GuiService")
local UserInputService = game:GetService("UserInputService")
local StarterGui = game:GetService("StarterGui")
local TeleportService = game:GetService("TeleportService")

return function(ch)
	local r = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local function noop() end
	ContextActionService:BindActionAtPriority("ConfLow", noop, true, 1000, Enum.KeyCode.F)
	ContextActionService:BindActionAtPriority("ConfHigh", noop, false, 3000, Enum.KeyCode.F)
	local info = ContextActionService:GetBoundActionInfo("ConfLow")
	r.info_priority = info.priorityLevel
	r.info_touch = info.createTouchButton
	r.info_inputs = #info.inputTypes
	r.info_missing = next(ContextActionService:GetBoundActionInfo("ConfNone")) == nil
	r.button_desktop = ContextActionService:GetButton("ConfLow") == nil
	r.set_title = err(function()
		ContextActionService:SetTitle("ConfLow", "Go")
	end)
	r.set_position = err(function()
		ContextActionService:SetPosition("ConfLow", UDim2.new(0.5, 0, 0.5, 0))
	end)
	ContextActionService:UnbindAction("ConfLow")
	ContextActionService:UnbindAction("ConfHigh")
	r.touch = UserInputService.TouchEnabled
	r.keyboard = UserInputService.KeyboardEnabled
	r.mouse = UserInputService.MouseEnabled
	r.gamepad = UserInputService.GamepadEnabled
	r.key_a = UserInputService:GetStringForKeyCode(Enum.KeyCode.A)
	r.key_one = UserInputService:GetStringForKeyCode(Enum.KeyCode.One)
	r.key_semicolon = UserInputService:GetStringForKeyCode(Enum.KeyCode.Semicolon)
	r.key_shift = UserInputService:GetStringForKeyCode(Enum.KeyCode.LeftShift)
	r.focused = UserInputService:GetFocusedTextBox() == nil
	r.keys_pressed = #UserInputService:GetKeysPressed()
	r.selected_nil = GuiService.SelectedObject == nil
	local screen = Instance.new("ScreenGui")
	local first = Instance.new("TextButton")
	first.Name = "First"
	first.Parent = screen
	screen.Parent = game:GetService("Players").LocalPlayer.PlayerGui
	local gained = false
	first.SelectionGained:Connect(function()
		gained = true
	end)
	GuiService:Select(screen)
	r.select_picks = GuiService.SelectedObject and GuiService.SelectedObject.Name or "nil"
	for _ = 1, 3 do
		ch.beat()
	end
	r.selection_gained = gained
	GuiService.SelectedObject = nil
	screen:Destroy()
	r.emotes_default = GuiService:GetEmotesMenuOpen()
	r.inspect_default = GuiService:GetInspectMenuEnabled()
	r.inspect_bad = err(function()
		GuiService:InspectPlayerFromUserId("x")
	end)
	r.shop_enabled = StarterGui:GetCoreGuiEnabled(Enum.CoreGuiType.ExperienceShop)
	StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.AvatarSwitcher, false)
	r.switcher_off = StarterGui:GetCoreGuiEnabled(Enum.CoreGuiType.AvatarSwitcher)
	StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.AvatarSwitcher, true)
	r.system_message = err(function()
		StarterGui:SetCore("ChatMakeSystemMessage", { Text = "conformance" })
	end)
	r.teleport_data = TeleportService:GetLocalPlayerTeleportData() == nil
	r.arriving_gui = TeleportService:GetArrivingTeleportGui() == nil
	r.teleport_gui_bad = err(function()
		TeleportService:SetTeleportGui(5)
	end)
	return r
end
