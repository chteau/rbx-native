--!strict
--// Dock widget: on/off switch, relay URL, status and the list of rbx-native collaborators.

local Widget = {}
Widget.__index = Widget

local HTTP_NOTE = "Presence sends your selection, camera and open script to the relay you set. "
	.. "Studio will ask you to allow HTTP requests to that domain."

type Callbacks = {
	onToggle: () -> (),
	onRelayUrl: (string) -> (),
	onCreateKey: () -> (),
}

export type Widget = typeof(setmetatable({} :: {
	gui: DockWidgetPluginGui,
	enabled: boolean,
	toggle: TextButton,
	urlBox: TextBox,
	status: TextLabel,
	keyButton: TextButton,
	note: TextLabel,
	rows: { Frame },
	list: ScrollingFrame,
	users: { any },
	connections: { RBXScriptConnection },
}, Widget))

local function themeColor(color: Enum.StudioStyleGuideColor): Color3
	return (settings() :: any).Studio.Theme:GetColor(color)
end

local function parseColor(hex: string): Color3
	local r, g, b = string.match(hex, "^#(%x%x)(%x%x)(%x%x)$")
	if not r then
		return Color3.new(1, 1, 1)
	end
	return Color3.fromRGB(tonumber(r, 16) :: number, tonumber(g, 16) :: number, tonumber(b, 16) :: number)
end

local function newLabel(parent: Instance, order: number, text: string): TextLabel
	local label = Instance.new("TextLabel")
	label.LayoutOrder = order
	label.Size = UDim2.new(1, -8, 0, 20)
	label.AutomaticSize = Enum.AutomaticSize.Y
	label.BackgroundTransparency = 1
	label.TextXAlignment = Enum.TextXAlignment.Left
	label.TextWrapped = true
	label.Font = Enum.Font.BuilderSans
	label.TextSize = 14
	label.Text = text
	label.Parent = parent
	return label
end

local function newButton(parent: Instance, order: number, text: string): TextButton
	local button = Instance.new("TextButton")
	button.LayoutOrder = order
	button.Size = UDim2.new(1, -8, 0, 28)
	button.AutoButtonColor = true
	button.BorderSizePixel = 1
	button.Font = Enum.Font.BuilderSans
	button.TextSize = 14
	button.Text = text
	button.Parent = parent
	return button
end

function Widget.new(plugin: Plugin, callbacks: Callbacks): Widget
	local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 280, 360, 220, 220)
	local gui = plugin:CreateDockWidgetPluginGuiAsync("RbxNativePresence", info)
	gui.Title = "rbx-native presence"

	local list = Instance.new("ScrollingFrame")
	list.Size = UDim2.fromScale(1, 1)
	list.BorderSizePixel = 0
	list.CanvasSize = UDim2.new()
	list.AutomaticCanvasSize = Enum.AutomaticSize.Y
	list.ScrollBarThickness = 8
	list.Parent = gui

	local layout = Instance.new("UIListLayout")
	layout.SortOrder = Enum.SortOrder.LayoutOrder
	layout.Padding = UDim.new(0, 6)
	layout.Parent = list

	local padding = Instance.new("UIPadding")
	padding.PaddingTop = UDim.new(0, 6)
	padding.PaddingLeft = UDim.new(0, 6)
	padding.PaddingRight = UDim.new(0, 6)
	padding.Parent = list

	local toggle = newButton(list, 1, "Presence: off")

	local urlBox = Instance.new("TextBox")
	urlBox.LayoutOrder = 2
	urlBox.Size = UDim2.new(1, -8, 0, 26)
	urlBox.ClearTextOnFocus = false
	urlBox.PlaceholderText = "Relay URL"
	urlBox.Text = ""
	urlBox.TextXAlignment = Enum.TextXAlignment.Left
	urlBox.Font = Enum.Font.BuilderSans
	urlBox.TextSize = 14
	urlBox.BorderSizePixel = 1
	urlBox.Parent = list

	local status = newLabel(list, 3, "")
	local keyButton = newButton(list, 4, "Create presence key")
	keyButton.Visible = false
	local note = newLabel(list, 5, HTTP_NOTE)
	note.Visible = false

	local self: Widget = setmetatable({
		gui = gui,
		enabled = false,
		toggle = toggle,
		urlBox = urlBox,
		status = status,
		keyButton = keyButton,
		note = note,
		rows = {},
		list = list,
		users = {},
		connections = {},
	}, Widget)

	table.insert(self.connections, toggle.Activated:Connect(callbacks.onToggle))
	table.insert(self.connections, keyButton.Activated:Connect(callbacks.onCreateKey))
	table.insert(
		self.connections,
		urlBox.FocusLost:Connect(function(enterPressed: boolean)
			if enterPressed then
				callbacks.onRelayUrl(urlBox.Text)
			end
		end)
	)
	table.insert(
		self.connections,
		(settings() :: any).Studio.ThemeChanged:Connect(function()
			self:applyTheme()
		end)
	)
	self:applyTheme()
	return self
end

function Widget.applyTheme(self: Widget)
	local main = Enum.StudioStyleGuideColor.MainBackground
	self.list.BackgroundColor3 = themeColor(main)
	self.list.ScrollBarImageColor3 = themeColor(Enum.StudioStyleGuideColor.ScrollBar)
	for _, label in { self.status, self.note } do
		label.TextColor3 = themeColor(Enum.StudioStyleGuideColor.MainText)
	end
	for _, button in { self.toggle, self.keyButton } do
		button.BackgroundColor3 = themeColor(Enum.StudioStyleGuideColor.Button)
		button.BorderColor3 = themeColor(Enum.StudioStyleGuideColor.ButtonBorder)
		button.TextColor3 = themeColor(Enum.StudioStyleGuideColor.ButtonText)
	end
	self.urlBox.BackgroundColor3 = themeColor(Enum.StudioStyleGuideColor.InputFieldBackground)
	self.urlBox.BorderColor3 = themeColor(Enum.StudioStyleGuideColor.InputFieldBorder)
	self.urlBox.TextColor3 = themeColor(Enum.StudioStyleGuideColor.MainText)
	self.urlBox.PlaceholderColor3 = themeColor(Enum.StudioStyleGuideColor.DimmedText)
	for _, row in self.rows do
		for _, child in row:GetChildren() do
			if child:IsA("TextLabel") then
				child.TextColor3 = themeColor(
					if child.Name == "Detail" then Enum.StudioStyleGuideColor.DimmedText else Enum.StudioStyleGuideColor.MainText
				)
			end
		end
	end
end

function Widget.setEnabled(self: Widget, on: boolean)
	self.enabled = on
	self.toggle.Text = if on then "Presence: on" else "Presence: off"
end

function Widget.setStatus(self: Widget, text: string)
	self.status.Text = text
end

function Widget.setRelayUrl(self: Widget, url: string)
	self.urlBox.Text = url
end

function Widget.setNeedsKey(self: Widget, needs: boolean)
	self.keyButton.Visible = needs
end

function Widget.showHttpNote(self: Widget, show: boolean)
	self.note.Visible = show
end

local function detailOf(user: any): string
	local parts: { string } = {}
	if user.script then
		table.insert(parts, `has {user.script} open`)
	end
	local newest: any = nil
	for _, edit in user.edits do
		if not newest or edit.at >= newest.at then
			newest = edit
		end
	end
	if newest then
		table.insert(parts, `{newest.path}.{newest.property} = {tostring(newest.value)}`)
	end
	return table.concat(parts, "\n")
end

function Widget.setUsers(self: Widget, users: { any })
	for _, row in self.rows do
		row:Destroy()
	end
	table.clear(self.rows)
	self.users = users

	local order = 10
	for _, user in users do
		if user.client == "rbx-native" then
			order += 1
			local row = Instance.new("Frame")
			row.LayoutOrder = order
			row.Size = UDim2.new(1, -8, 0, 20)
			row.AutomaticSize = Enum.AutomaticSize.Y
			row.BackgroundTransparency = 1

			local swatch = Instance.new("Frame")
			swatch.Name = "Swatch"
			swatch.Size = UDim2.fromOffset(12, 12)
			swatch.Position = UDim2.fromOffset(0, 4)
			swatch.BorderSizePixel = 0
			swatch.BackgroundColor3 = parseColor(user.color)
			swatch.Parent = row

			local name = newLabel(row, 1, user.name)
			name.Name = "Name"
			name.Position = UDim2.fromOffset(18, 0)
			name.Size = UDim2.new(1, -18, 0, 20)

			local detail = detailOf(user)
			if detail ~= "" then
				local d = newLabel(row, 2, detail)
				d.Name = "Detail"
				d.TextSize = 12
				d.Position = UDim2.fromOffset(18, 20)
				d.Size = UDim2.new(1, -18, 0, 16)
				row.Size = UDim2.new(1, -8, 0, 20)
			end
			row.Parent = self.list
			table.insert(self.rows, row)
		end
	end
	self:applyTheme()
end

function Widget.destroy(self: Widget)
	for _, connection in self.connections do
		connection:Disconnect()
	end
	table.clear(self.connections)
	self.gui:Destroy()
end

return Widget
