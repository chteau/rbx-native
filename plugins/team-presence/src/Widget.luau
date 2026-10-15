--!strict
--// Dock widget, laid out like rbx-native's own Collaborators dock and drawn
--// in its palette (see Ui): the note, a "Share my presence" switch, the
--// status line, an "Advanced" reveal holding the owner's relay setup, the
--// "While you were away" card, then one row per rbx-native collaborator.

local Catchup = require(script.Parent.Catchup)
local Ui = require(script.Parent.Ui)

local Widget = {}
Widget.__index = Widget

local NOTE = "Presence only works for collaborators who have Studio open with the rbx-native "
	.. "presence plugin running, or rbx-native itself."

local HTTP_NOTE = "Presence sends your selection, camera and open script to the relay. "
	.. "Studio asks once whether this plugin may reach that domain; allow it."

type Callbacks = {
	onToggle: () -> (),
	onSaveRelay: (string, string) -> (),
	onViewChanges: () -> (),
}

export type Widget = typeof(setmetatable({} :: {
	gui: DockWidgetPluginGui,
	enabled: boolean,
	relayKind: string,
	switch: Ui.Switch,
	status: TextLabel,
	advanced: TextButton,
	owner: Frame,
	kindWorker: TextButton,
	kindNtfy: TextButton,
	urlBox: TextBox,
	resolved: TextLabel,
	httpNote: Frame,
	catchup: Frame,
	catchupBody: Frame,
	rows: Frame,
	onViewChanges: () -> (),
	connections: { RBXScriptConnection },
}, Widget))

-- One half of the Worker/ntfy segmented control.
local function segment(parent: Instance, text: string, order: number): TextButton
	local button = Instance.new("TextButton")
	button.AutoButtonColor = false
	button.LayoutOrder = order
	button.Size = UDim2.fromOffset(0, 22)
	button.AutomaticSize = Enum.AutomaticSize.X
	button.FontFace = Ui.FONT
	button.TextSize = Ui.TEXT_XS
	button.Text = text
	Ui.pad(button, 0, 10, 0, 10)
	Ui.corner(button, 3)
	button.Parent = parent
	return button
end

function Widget.new(plugin: Plugin, callbacks: Callbacks): Widget
	local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 300, 420, 240, 260)
	local gui = plugin:CreateDockWidgetPluginGuiAsync("RbxNativePresence", info)
	gui.Title = "Collaborators"
	gui.ZIndexBehavior = Enum.ZIndexBehavior.Sibling

	local scroll = Instance.new("ScrollingFrame")
	scroll.Size = UDim2.fromScale(1, 1)
	scroll.CanvasSize = UDim2.new()
	scroll.AutomaticCanvasSize = Enum.AutomaticSize.Y
	scroll.ScrollBarThickness = 6
	scroll.ScrollBarImageColor3 = Ui.color("text3")
	scroll.ScrollingDirection = Enum.ScrollingDirection.Y
	Ui.fill(scroll, "dock")
	scroll.Parent = gui
	Ui.pad(scroll, 8, 10, 12, 8)
	Ui.list(scroll, 10)

	-- The note.
	local noteCard = Ui.card(scroll, 1)
	Ui.text(noteCard, NOTE, { size = Ui.TEXT_XS, color = "text2" })

	-- "Share my presence".
	local toggleRow = Ui.frame(scroll, 2)
	Ui.list(toggleRow, 8, true)
	local self: Widget
	local switch = Ui.switch(toggleRow, function()
		callbacks.onToggle()
	end, 1)
	local toggleLabel = Ui.text(toggleRow, "Share my presence", { size = Ui.TEXT_SM, order = 2, wrap = false })
	toggleLabel.AutomaticSize = Enum.AutomaticSize.X
	toggleLabel.Size = UDim2.fromOffset(0, 18)

	local status = Ui.text(scroll, "Off", { size = Ui.TEXT_XS, color = "text2", order = 3 })

	local advanced = Ui.link(scroll, "Show advanced", 4)
	advanced.TextColor3 = Ui.color("text3")
	advanced.FontFace = Ui.FONT

	-- Owner setup, behind the reveal: the transport, its URL, and saving both
	-- into the place for every collaborator.
	local owner = Ui.frame(scroll, 5)
	owner.Visible = false
	Ui.list(owner, 6)
	Ui.text(owner, "Owner setup — saved into the place for everyone", { size = Ui.TEXT_XS, color = "text3", order = 1 })

	local kinds = Instance.new("Frame")
	kinds.LayoutOrder = 2
	kinds.AutomaticSize = Enum.AutomaticSize.X
	kinds.Size = UDim2.fromOffset(0, 26)
	Ui.fill(kinds, "field_select")
	Ui.corner(kinds)
	Ui.pad(kinds, 2)
	Ui.list(kinds, 2, true)
	kinds.Parent = owner
	local kindWorker = segment(kinds, "Worker", 1)
	local kindNtfy = segment(kinds, "ntfy", 2)

	local urlBox = Instance.new("TextBox")
	urlBox.LayoutOrder = 3
	urlBox.Size = UDim2.new(1, 0, 0, 28)
	urlBox.ClearTextOnFocus = false
	urlBox.PlaceholderText = "Relay URL (blank = community default)"
	urlBox.PlaceholderColor3 = Ui.color("text3")
	urlBox.Text = ""
	urlBox.TextXAlignment = Enum.TextXAlignment.Left
	urlBox.FontFace = Ui.FONT
	urlBox.TextSize = Ui.TEXT_SM
	urlBox.TextColor3 = Ui.color("text")
	urlBox.TextTruncate = Enum.TextTruncate.AtEnd
	Ui.fill(urlBox, "field_select")
	Ui.corner(urlBox)
	local urlStroke = Ui.stroke(urlBox, "border")
	Ui.pad(urlBox, 0, 9, 0, 9)
	urlBox.Parent = owner
	urlBox.Focused:Connect(function()
		urlStroke.Color = Ui.color("check_on")
		urlStroke.Transparency = 0.45
	end)
	urlBox.FocusLost:Connect(function()
		urlStroke.Color = Ui.color("border")
		urlStroke.Transparency = Ui.alpha("border")
	end)

	local saveButton = Ui.button(owner, "Save to place", false, 4)
	saveButton.Size = UDim2.fromOffset(110, 28)
	local resolved = Ui.text(owner, "", { size = Ui.TEXT_XS, color = "text3", order = 5 })

	local httpNote = Ui.card(scroll, 6)
	httpNote.Visible = false
	Ui.text(httpNote, HTTP_NOTE, { size = Ui.TEXT_XS, color = "text2" })

	-- "While you were away", hidden until something changed.
	local catchup = Ui.card(scroll, 7, 4)
	catchup.Name = "Catchup"
	catchup.Visible = false
	local head = Ui.frame(catchup, 1)
	local title = Ui.text(head, "While you were away", { size = Ui.TEXT_XS, font = Ui.FONT_SEMIBOLD, wrap = false })
	title.Size = UDim2.new(1, -60, 0, 16)
	local dismiss = Ui.link(head, "Dismiss")
	dismiss.TextColor3 = Ui.color("text3")
	dismiss.FontFace = Ui.FONT
	dismiss.AnchorPoint = Vector2.new(1, 0)
	dismiss.Position = UDim2.fromScale(1, 0)
	local catchupBody = Ui.frame(catchup, 2)
	Ui.list(catchupBody, 3)

	local rows = Ui.frame(scroll, 8)
	Ui.list(rows, 10)

	self = setmetatable({
		gui = gui,
		enabled = false,
		relayKind = "worker",
		switch = switch,
		status = status,
		advanced = advanced,
		owner = owner,
		kindWorker = kindWorker,
		kindNtfy = kindNtfy,
		urlBox = urlBox,
		resolved = resolved,
		httpNote = httpNote,
		catchup = catchup,
		catchupBody = catchupBody,
		rows = rows,
		onViewChanges = callbacks.onViewChanges,
		connections = {},
	}, Widget)
	self:showKind()

	table.insert(
		self.connections,
		advanced.Activated:Connect(function()
			owner.Visible = not owner.Visible
			advanced.Text = if owner.Visible then "Hide advanced" else "Show advanced"
		end)
	)
	table.insert(
		self.connections,
		kindWorker.Activated:Connect(function()
			self.relayKind = "worker"
			self:showKind()
		end)
	)
	table.insert(
		self.connections,
		kindNtfy.Activated:Connect(function()
			self.relayKind = "ntfy"
			self:showKind()
		end)
	)
	table.insert(
		self.connections,
		saveButton.Activated:Connect(function()
			callbacks.onSaveRelay(self.relayKind, (string.gsub(urlBox.Text, "^%s*(.-)%s*$", "%1")))
		end)
	)
	table.insert(
		self.connections,
		dismiss.Activated:Connect(function()
			self.catchup.Visible = false
		end)
	)
	return self
end

-- The segmented control's active half: `accent_soft` behind `check_on`.
function Widget.showKind(self: Widget)
	for _, pair in { { self.kindWorker, "worker" }, { self.kindNtfy, "ntfy" } } do
		local button, kind = pair[1] :: TextButton, pair[2] :: string
		local active = self.relayKind == kind
		Ui.fill(button, if active then "accent_soft" else "field_select")
		button.TextColor3 = Ui.color(if active then "check_on" else "text2")
		button.FontFace = if active then Ui.FONT_SEMIBOLD else Ui.FONT
	end
end

function Widget.setEnabled(self: Widget, on: boolean)
	self.enabled = on
	self.switch.set(on)
end

function Widget.setStatus(self: Widget, text: string, failed: boolean?)
	self.status.Text = text
	self.status.TextColor3 = Ui.color(if failed then "text_error" else "text2")
end

-- Owner setup: show the kind, prefill the URL, and say which relay is in use.
function Widget.setRelay(self: Widget, kind: string, url: string, isCommunity: boolean)
	self.relayKind = kind
	self:showKind()
	if not isCommunity then
		self.urlBox.Text = url
		self.resolved.Text = `Relay: {url} ({kind})`
	else
		self.resolved.Text = `Relay: {url} (community instance, best effort, no uptime promise)`
	end
end

function Widget.showHttpNote(self: Widget, show: boolean)
	self.httpNote.Visible = show
end

-- "While you were away": one line per author (value-less, as rbx-native's
-- dock words it), then how many instances changed and "View changes" when
-- there is anything to show. `nil` or an empty summary hides the card.
function Widget.setCatchup(self: Widget, summary: any?)
	for _, child in self.catchupBody:GetChildren() do
		if not child:IsA("UIListLayout") then
			child:Destroy()
		end
	end
	if not summary or #summary.authors == 0 then
		self.catchup.Visible = false
		return
	end
	self.catchup.Visible = true
	local order = 0
	for _, author in summary.authors do
		order += 1
		Ui.text(self.catchupBody, Catchup.authorLine(author), { size = Ui.TEXT_XS, color = "text2", order = order })
	end
	local view = Ui.link(self.catchupBody, "View changes", order + 1)
	view.Activated:Connect(function()
		self.onViewChanges()
	end)
end

local function detailLines(user: any): { string }
	local lines: { string } = {}
	if #user.selection > 0 then
		table.insert(lines, `Selected: {#user.selection}`)
	end
	if user.script then
		table.insert(lines, `Has {user.script} open`)
	end
	local newest: any = nil
	for _, edit in user.edits do
		if not newest or edit.at >= newest.at then
			newest = edit
		end
	end
	if newest then
		table.insert(lines, `{newest.property} of {newest.path} -> {tostring(newest.value)}`)
	end
	return lines
end

-- One row per rbx-native collaborator: their colour's dot, the name, "Sending
-- presence", then what they have selected, open and last edited.
function Widget.setUsers(self: Widget, users: { any })
	for _, child in self.rows:GetChildren() do
		if not child:IsA("UIListLayout") then
			child:Destroy()
		end
	end
	for index, user in users do
		if user.client == "rbx-native" then
			local row = Ui.frame(self.rows, index)
			Ui.list(row, 3)
			local line = Ui.frame(row, 1)
			Ui.list(line, 8, true)
			Ui.dot(line, Ui.hex(user.color), 10).LayoutOrder = 1
			local name = Ui.text(line, user.name, { size = Ui.TEXT_XS, order = 2, wrap = false })
			name.AutomaticSize = Enum.AutomaticSize.X
			name.Size = UDim2.fromOffset(0, 16)
			local state = Ui.text(line, "Sending presence", { size = Ui.TEXT_XS, color = "text3", order = 3, wrap = false })
			state.AutomaticSize = Enum.AutomaticSize.X
			state.Size = UDim2.fromOffset(0, 16)
			for i, text in detailLines(user) do
				local detail = Ui.text(row, text, { size = Ui.TEXT_XS, color = "text2", order = 1 + i })
				local pad = Instance.new("UIPadding")
				pad.PaddingLeft = UDim.new(0, 18)
				pad.Parent = detail
			end
		end
	end
end

function Widget.destroy(self: Widget)
	for _, connection in self.connections do
		connection:Disconnect()
	end
	table.clear(self.connections)
	self.gui:Destroy()
end

return Widget
