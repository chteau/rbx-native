-- A chat-style dock widget: a row of tab buttons, a scrolling message list
-- (long wrapped RichText lines, UIListLayout, AutomaticCanvasSize), a
-- multi-line note box, an input box with a Send button, and a status line
-- that updates every second.
--
-- It exists to check two things against real Studio: that a plugin which
-- keeps a wait loop alive costs the editor nothing while its dock sits
-- unchanged, and that the dock draws like Studio's (clipping, scrolling,
-- text, list spacing). `build` only touches its `root` argument, so the
-- same function can be pasted into Studio's command bar with a Frame of
-- the dock's size as `root`.

local function build(root)
	local palette = {
		background = Color3.fromRGB(46, 46, 46),
		bar = Color3.fromRGB(37, 37, 37),
		bubble = Color3.fromRGB(60, 60, 60),
		mine = Color3.fromRGB(0, 95, 160),
		text = Color3.fromRGB(230, 230, 230),
		dim = Color3.fromRGB(150, 150, 150),
		accent = Color3.fromRGB(53, 181, 255),
	}

	local body = Instance.new("Frame")
	body.Name = "Body"
	body.Size = UDim2.fromScale(1, 1)
	body.BackgroundColor3 = palette.background
	body.BorderSizePixel = 0
	body.Parent = root

	-- Tabs along the top.
	local tabs = Instance.new("Frame")
	tabs.Name = "Tabs"
	tabs.Size = UDim2.new(1, 0, 0, 32)
	tabs.BackgroundColor3 = palette.bar
	tabs.BorderSizePixel = 0
	tabs.Parent = body
	local tabList = Instance.new("UIListLayout")
	tabList.FillDirection = Enum.FillDirection.Horizontal
	tabList.Padding = UDim.new(0, 4)
	tabList.VerticalAlignment = Enum.VerticalAlignment.Center
	tabList.SortOrder = Enum.SortOrder.LayoutOrder
	tabList.Parent = tabs
	local tabPadding = Instance.new("UIPadding")
	tabPadding.PaddingLeft = UDim.new(0, 6)
	tabPadding.Parent = tabs
	local tabButtons = {}
	for index, title in ipairs({ "General", "Team", "Logs" }) do
		local tab = Instance.new("TextButton")
		tab.Name = title .. "Tab"
		tab.LayoutOrder = index
		tab.AutomaticSize = Enum.AutomaticSize.X
		tab.Size = UDim2.new(0, 0, 0, 24)
		tab.BackgroundColor3 = index == 1 and palette.accent or palette.bubble
		tab.TextColor3 = index == 1 and Color3.new(1, 1, 1) or palette.text
		tab.Font = Enum.Font.GothamBold
		tab.TextSize = 13
		tab.Text = title
		tab.AutoButtonColor = false
		tab.Parent = tabs
		local corner = Instance.new("UICorner")
		corner.CornerRadius = UDim.new(0, 6)
		corner.Parent = tab
		local padding = Instance.new("UIPadding")
		padding.PaddingLeft = UDim.new(0, 10)
		padding.PaddingRight = UDim.new(0, 10)
		padding.Parent = tab
		tabButtons[index] = tab
	end

	-- The message list between the tabs and the input row.
	local chat = Instance.new("ScrollingFrame")
	chat.Name = "Chat"
	chat.Position = UDim2.fromOffset(0, 32)
	chat.Size = UDim2.new(1, 0, 1, -32 - 104 - 22)
	chat.BackgroundColor3 = palette.background
	chat.BorderSizePixel = 0
	chat.CanvasSize = UDim2.new()
	chat.AutomaticCanvasSize = Enum.AutomaticSize.Y
	chat.ScrollBarThickness = 6
	chat.ScrollBarImageColor3 = palette.accent
	chat.ScrollingDirection = Enum.ScrollingDirection.Y
	chat.Parent = body
	local chatList = Instance.new("UIListLayout")
	chatList.Padding = UDim.new(0, 6)
	chatList.SortOrder = Enum.SortOrder.LayoutOrder
	chatList.Parent = chat
	local chatPadding = Instance.new("UIPadding")
	chatPadding.PaddingTop = UDim.new(0, 8)
	chatPadding.PaddingBottom = UDim.new(0, 8)
	chatPadding.PaddingLeft = UDim.new(0, 8)
	chatPadding.PaddingRight = UDim.new(0, 14)
	chatPadding.Parent = chat

	local count = 0
	local function addMessage(author, text, mine)
		count += 1
		local bubble = Instance.new("TextLabel")
		bubble.Name = "Message" .. count
		bubble.LayoutOrder = count
		bubble.Size = UDim2.fromScale(1, 0)
		bubble.AutomaticSize = Enum.AutomaticSize.Y
		bubble.BackgroundColor3 = mine and palette.mine or palette.bubble
		bubble.TextColor3 = palette.text
		bubble.FontFace = Font.new("rbxasset://fonts/families/BuilderSans.json")
		bubble.TextSize = 15
		bubble.RichText = true
		bubble.TextWrapped = true
		bubble.TextXAlignment = Enum.TextXAlignment.Left
		bubble.TextYAlignment = Enum.TextYAlignment.Top
		bubble.Text = string.format("<b>%s</b>  %s", author, text)
		bubble.Parent = chat
		local corner = Instance.new("UICorner")
		corner.CornerRadius = UDim.new(0, 8)
		corner.Parent = bubble
		local padding = Instance.new("UIPadding")
		padding.PaddingTop = UDim.new(0, 6)
		padding.PaddingBottom = UDim.new(0, 6)
		padding.PaddingLeft = UDim.new(0, 8)
		padding.PaddingRight = UDim.new(0, 8)
		padding.Parent = bubble
		return bubble
	end

	local lines = {
		{ "Ana", "hey, did the new lobby build go out?", false },
		{ "Ben", "Yes, about ten minutes ago. The spawn pads moved a bit to the left so the queue doesn't block the shop door any more.", true },
		{ "Ana", "Nice. Players were bunching up there <i>all</i> weekend, so that should help a lot with the complaints we got.", false },
		{ "Cleo", "Heads up: the leaderboard script throws when a player leaves mid-round. I'll push a fix tonight once I've reproduced it on a private server.", false },
		{ "Ben", "Thanks! Ping me when it's up and I'll test it with the four-player setup.", true },
		{ "Ana", "Also the trail colours look washed out on mobile. Might be the transparency curve, might be the texture. Will check.", false },
		{ "Cleo", "<font color=\"#FFB347\">reminder</font>: playtest at 8pm, bring snacks", false },
		{ "Ben", "On it. Last thing: should the chat window remember its scroll position between sessions, or always start at the newest message?", true },
	}
	for _, line in ipairs(lines) do
		addMessage(line[1], line[2], line[3])
	end

	-- The input row.
	local inputRow = Instance.new("Frame")
	inputRow.Name = "InputRow"
	inputRow.AnchorPoint = Vector2.new(0, 1)
	inputRow.Position = UDim2.new(0, 0, 1, -22)
	inputRow.Size = UDim2.new(1, 0, 0, 104)
	inputRow.BackgroundColor3 = palette.bar
	inputRow.BorderSizePixel = 0
	inputRow.Parent = body
	-- A note that wraps, takes Enter as a line break and has more lines
	-- than it shows.
	local note = Instance.new("TextBox")
	note.Name = "Note"
	note.Position = UDim2.fromOffset(8, 8)
	note.Size = UDim2.new(1, -16, 0, 52)
	note.BackgroundColor3 = palette.bubble
	note.TextColor3 = palette.text
	note.FontFace = Font.new("rbxasset://fonts/families/BuilderSans.json")
	note.TextSize = 15
	note.MultiLine = true
	note.TextWrapped = true
	note.ClearTextOnFocus = false
	note.TextXAlignment = Enum.TextXAlignment.Left
	note.TextYAlignment = Enum.TextYAlignment.Top
	note.Text = "Playtest notes: the shop door queue is fixed, the trail colours still look washed out on mobile.\nNext: leaderboard fix, then the four-player run."
	note.Parent = inputRow
	Instance.new("UICorner").Parent = note
	local notePadding = Instance.new("UIPadding")
	notePadding.PaddingLeft = UDim.new(0, 8)
	notePadding.PaddingRight = UDim.new(0, 8)
	notePadding.PaddingTop = UDim.new(0, 2)
	notePadding.Parent = note
	local box = Instance.new("TextBox")
	box.Name = "Input"
	box.Position = UDim2.fromOffset(8, 68)
	box.Size = UDim2.new(1, -88, 0, 28)
	box.BackgroundColor3 = palette.bubble
	box.TextColor3 = palette.text
	box.PlaceholderText = "Message"
	box.PlaceholderColor3 = palette.dim
	box.Font = Enum.Font.SourceSans
	box.TextSize = 16
	box.Text = ""
	box.TextXAlignment = Enum.TextXAlignment.Left
	box.ClearTextOnFocus = false
	box.Parent = inputRow
	Instance.new("UICorner").Parent = box
	local boxPadding = Instance.new("UIPadding")
	boxPadding.PaddingLeft = UDim.new(0, 8)
	boxPadding.Parent = box
	local send = Instance.new("TextButton")
	send.Name = "Send"
	send.AnchorPoint = Vector2.new(1, 0)
	send.Position = UDim2.new(1, -8, 0, 68)
	send.Size = UDim2.fromOffset(64, 28)
	send.BackgroundColor3 = palette.accent
	send.TextColor3 = Color3.new(1, 1, 1)
	send.Font = Enum.Font.GothamMedium
	send.TextSize = 14
	send.Text = "Send"
	send.Parent = inputRow
	Instance.new("UICorner").Parent = send

	-- The status line under it.
	local status = Instance.new("TextLabel")
	status.Name = "Status"
	status.AnchorPoint = Vector2.new(0, 1)
	status.Position = UDim2.fromScale(0, 1)
	status.Size = UDim2.new(1, 0, 0, 22)
	status.BackgroundTransparency = 1
	status.TextColor3 = palette.dim
	status.Font = Enum.Font.SourceSans
	status.TextSize = 14
	status.TextTruncate = Enum.TextTruncate.AtEnd
	status.TextXAlignment = Enum.TextXAlignment.Left
	status.Text = "Connected"
	status.Parent = body
	local statusPadding = Instance.new("UIPadding")
	statusPadding.PaddingLeft = UDim.new(0, 8)
	statusPadding.Parent = status

	local function submit()
		if box.Text ~= "" then
			addMessage("You", box.Text, true)
			box.Text = ""
			chat.CanvasPosition = Vector2.new(0, math.huge)
		end
	end
	send.Activated:Connect(submit)
	box.FocusLost:Connect(function(enter)
		if enter then
			submit()
		end
	end)
	for index, tab in ipairs(tabButtons) do
		tab.Activated:Connect(function()
			for other, button in ipairs(tabButtons) do
				button.BackgroundColor3 = other == index and palette.accent or palette.bubble
				button.TextColor3 = other == index and Color3.new(1, 1, 1) or palette.text
			end
		end)
	end

	return status, chat
end

if not plugin then
	return build
end

local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, true, false, 340, 520, 260, 300)
local widget = plugin:CreateDockWidgetPluginGui("ChatWidget", info)
widget.Title = "Chat"
local status = build(widget)

-- A status line that changes once a second, from a loop that never stops
-- waiting: the dock must not cost a redraw on the ticks in between.
local started = os.clock()
task.spawn(function()
	while true do
		task.wait(1)
		status.Text = string.format("Connected, %ds", math.floor(os.clock() - started))
	end
end)
game:GetService("RunService").Heartbeat:Connect(function() end)
