-- GuiRuntime on the client: what PlayerGui holds and how it's laid out,
-- then the events a click on a button and typing in a box fire.
local UserInputService = game:GetService("UserInputService")
local GuiService = game:GetService("GuiService")
local log = {}
local connections = {}

local function v2(v)
	return string.format("%d,%d", math.floor(v.X + 0.5), math.floor(v.Y + 0.5))
end

local function inputName(input)
	if input.KeyCode ~= Enum.KeyCode.Unknown then
		return input.KeyCode.Name
	end
	return input.UserInputType.Name
end

return function(ch, step)
	local gui = game:GetService("Players").LocalPlayer.PlayerGui
	local screen = gui:WaitForChild("ConfReset")
	local button = screen:WaitForChild("Button")
	local box = screen:WaitForChild("Box")
	if step == "look" then
		ch.beats(2)
		local names = {}
		for _, child in gui:GetChildren() do
			if string.sub(child.Name, 1, 4) == "Conf" then
				table.insert(names, child.Name)
			end
		end
		table.sort(names)
		local topLeft, bottomRight = GuiService:GetGuiInset()
		local holder = screen.Holder
		local width = screen.AbsoluteSize.X
		local runs = (_G.confGuiRuns or 0) - (_G.confGuiRunsSeen or 0)
		_G.confGuiRunsSeen = _G.confGuiRuns or 0
		return {
			has = table.concat(names, ","),
			inset = v2(topLeft) .. " " .. v2(bottomRight),
			runs = runs,
			button_pos = v2(button.AbsolutePosition),
			button_size = v2(button.AbsoluteSize),
			holder_from_center = v2(holder.AbsolutePosition - Vector2.new(width / 2, 0)),
			inner = v2(holder.Inner.AbsolutePosition - holder.AbsolutePosition) .. " " .. v2(holder.Inner.AbsoluteSize),
			_screen = v2(screen.AbsoluteSize),
		}
	elseif step == "watch" then
		-- Each pass starts over: the last pass's service connections go.
		log = {}
		for _, c in connections do
			c:Disconnect()
		end
		table.clear(connections)
		local function add(text)
			table.insert(log, text)
		end
		for _, name in { "MouseEnter", "MouseLeave", "MouseButton1Down", "MouseButton1Up", "MouseButton1Click" } do
			button[name]:Connect(function()
				add(name)
			end)
		end
		button.Activated:Connect(function(input, clicks)
			add("Activated:" .. input.UserInputType.Name .. ":" .. tostring(clicks))
		end)
		button.InputBegan:Connect(function(input)
			if input.UserInputType ~= Enum.UserInputType.MouseMovement then
				add("button.InputBegan:" .. inputName(input))
			end
		end)
		button.InputEnded:Connect(function(input)
			if input.UserInputType ~= Enum.UserInputType.MouseMovement then
				add("button.InputEnded:" .. inputName(input))
			end
		end)
		table.insert(connections, UserInputService.InputBegan:Connect(function(input, processed)
			if input.UserInputType ~= Enum.UserInputType.MouseMovement then
				add("uis.InputBegan:" .. inputName(input) .. ":" .. tostring(processed))
			end
		end))
		table.insert(connections, UserInputService.InputEnded:Connect(function(input, processed)
			if input.UserInputType ~= Enum.UserInputType.MouseMovement then
				add("uis.InputEnded:" .. inputName(input) .. ":" .. tostring(processed))
			end
		end))
		table.insert(connections, UserInputService.TextBoxFocused:Connect(function(focused)
			add("uis.TextBoxFocused:" .. focused.Name)
		end))
		table.insert(connections, UserInputService.TextBoxFocusReleased:Connect(function(released)
			add("uis.TextBoxFocusReleased:" .. released.Name)
		end))
		box.Focused:Connect(function()
			add("Focused:" .. box.Text .. ":" .. tostring(UserInputService:GetFocusedTextBox() == box))
		end)
		box.FocusLost:Connect(function(enter, input)
			add("FocusLost:" .. tostring(enter) .. ":" .. (input and inputName(input) or "nil"))
		end)
		box:GetPropertyChangedSignal("Text"):Connect(function()
			add("Text=" .. box.Text)
		end)
		-- AbsolutePosition counts from below the top bar inset, even in a
		-- ScreenGui that ignores it; the window's pixels don't.
		local inset = GuiService:GetGuiInset()
		local function centre(object)
			local c = object.AbsolutePosition + object.AbsoluteSize / 2 + inset
			return math.floor(c.X + 0.5), math.floor(c.Y + 0.5)
		end
		local bx, by = centre(button)
		local tx, ty = centre(box)
		return { button_x = bx, button_y = by, box_x = tx, box_y = ty }
	end
	-- "log"
	ch.beats(2)
	return {
		log = table.concat(log, " "),
		box_text = box.Text,
		focused_after = tostring(box:IsFocused()),
	}
end
