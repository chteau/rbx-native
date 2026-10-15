--!strict
--// "Changes while you were away", drawn like rbx-native's diff window: a
--// header with the change counts as filters and a search, the changed
--// instances on the left (their Explorer icons, grouped as rbx-native
--// groups them), and the selected one on the right — its class, its path,
--// and a table of the properties collaborators edited, with each one's
--// current value, who edited it and when.
--//
--// The journal holds no values (by design: nothing anyone typed leaves their
--// machine through the relay), so "Now" is read off this place, and an
--// instance that no longer resolves (renamed or deleted since) is listed as
--// "Not found".

local Changes = require(script.Parent.Changes)
local Paths = require(script.Parent.Paths)
local Ui = require(script.Parent.Ui)

local DiffWindow = {}
DiffWindow.__index = DiffWindow

local LIST_WIDTH = 300
local FOOTER_NOTE = "Values are this place's current ones; who edited what comes from the presence journal."

type Filter = "all" | "updated" | "missing"

export type DiffWindow = typeof(setmetatable({} :: {
	gui: DockWidgetPluginGui,
	changes: { Changes.Change },
	found: { [string]: Instance },
	filter: Filter,
	query: string,
	selected: string?,
	subtitle: TextLabel,
	segments: { [Filter]: TextButton },
	counts: { [Filter]: TextLabel },
	searchBox: TextBox,
	list: ScrollingFrame,
	detail: Frame,
}, DiffWindow))

-- A value as the Properties panel words it, and a swatch for a colour.
local function format(value: any): (string, Color3?)
	local kind = typeof(value)
	if value == nil then
		return "—", nil
	elseif kind == "boolean" then
		return tostring(value), nil
	elseif kind == "number" then
		local text = string.format("%.3f", value)
		text = string.gsub(text, "0+$", "")
		text = string.gsub(text, "%.$", "")
		return text, nil
	elseif kind == "string" then
		local first = string.match(value, "^[^\n]*") or ""
		if #first > 60 or #first < #value then
			return `"{string.sub(first, 1, 60)}…"`, nil
		end
		return `"{value}"`, nil
	elseif kind == "Vector3" then
		local v = value :: Vector3
		return `({format(v.X)}, {format(v.Y)}, {format(v.Z)})`, nil
	elseif kind == "Vector2" then
		local v = value :: Vector2
		return `({format(v.X)}, {format(v.Y)})`, nil
	elseif kind == "CFrame" then
		local p = (value :: CFrame).Position
		return `pos=({format(p.X)}, {format(p.Y)}, {format(p.Z)})`, nil
	elseif kind == "Color3" then
		local c = value :: Color3
		return `({math.round(c.R * 255)}, {math.round(c.G * 255)}, {math.round(c.B * 255)})`, c
	elseif kind == "BrickColor" then
		return (value :: BrickColor).Name, (value :: BrickColor).Color
	elseif kind == "EnumItem" then
		return (value :: EnumItem).Name, nil
	elseif kind == "Instance" then
		return (value :: Instance).Name, nil
	end
	return tostring(value), nil
end

local function read(instance: Instance?, property: string): (boolean, any)
	if not instance then
		return false, nil
	end
	return pcall(function()
		return (instance :: any)[property]
	end)
end

local function sectionLabel(parent: Instance, text: string, count: number?, order: number, ink: string?)
	local row = Ui.frame(parent, order)
	row.Size = UDim2.new(1, 0, 0, 14)
	row.AutomaticSize = Enum.AutomaticSize.None
	Ui.list(row, 6, true)
	if ink then
		Ui.dot(row, Ui.color(ink)).LayoutOrder = 0
	end
	local label = Ui.text(row, text, { size = Ui.TEXT_XXS, color = "text3", font = Ui.FONT_BOLD, wrap = false, order = 1 })
	label.AutomaticSize = Enum.AutomaticSize.X
	label.Size = UDim2.fromOffset(0, 14)
	if count then
		local n = Ui.text(row, tostring(count), { size = Ui.TEXT_XXS, color = "text3", font = Ui.FONT_MONO, wrap = false, order = 2 })
		n.AutomaticSize = Enum.AutomaticSize.X
		n.Size = UDim2.fromOffset(0, 14)
	end
end

local function hairline(parent: Instance, vertical: boolean, position: UDim2, inset: number?)
	local line = Instance.new("Frame")
	line.Size = if vertical then UDim2.new(0, 1, 1, -(inset or 0)) else UDim2.new(1, 0, 0, 1)
	line.Position = position
	Ui.fill(line, "border")
	line.Parent = parent
end

function DiffWindow.new(plugin: Plugin): DiffWindow
	local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Float, false, true, 1000, 640, 560, 420)
	local gui = plugin:CreateDockWidgetPluginGuiAsync("RbxNativePresenceChanges", info)
	gui.Title = "Changes while you were away"
	gui.ZIndexBehavior = Enum.ZIndexBehavior.Sibling

	local root = Instance.new("Frame")
	root.Size = UDim2.fromScale(1, 1)
	Ui.fill(root, "dock")
	root.Parent = gui

	-- Header: tile, title and subtitle, the filters, the search. 61 tall.
	local header = Instance.new("Frame")
	header.Size = UDim2.new(1, 0, 0, 60)
	header.BackgroundTransparency = 1
	header.Parent = root
	hairline(root, false, UDim2.fromOffset(0, 60))
	Ui.pad(header, 12, 16)

	local tile = Instance.new("TextLabel")
	tile.Size = UDim2.fromOffset(28, 28)
	tile.Position = UDim2.new(0, 0, 0.5, -14)
	Ui.fill(tile, "accent_soft")
	Ui.corner(tile, Ui.RADIUS_TILE)
	tile.FontFace = Ui.FONT_BOLD
	tile.TextSize = 15
	tile.TextColor3 = Ui.color("check_on")
	tile.Text = "Δ"
	tile.Parent = header

	local titles = Instance.new("Frame")
	titles.BackgroundTransparency = 1
	titles.Position = UDim2.fromOffset(38, 0)
	titles.Size = UDim2.new(1, -38 - 470, 1, 0)
	titles.Parent = header
	Ui.list(titles, 1)
	local titleList = titles:FindFirstChildOfClass("UIListLayout") :: UIListLayout
	titleList.VerticalAlignment = Enum.VerticalAlignment.Center
	Ui.text(titles, "Review changes", { size = Ui.TEXT_LG, font = Ui.FONT_BOLD, wrap = false, order = 1 })
	local subtitle = Ui.text(titles, "", { size = Ui.TEXT_SM, color = "text2", wrap = false, order = 2 })

	local controls = Instance.new("Frame")
	controls.BackgroundTransparency = 1
	controls.AnchorPoint = Vector2.new(1, 0.5)
	controls.Position = UDim2.fromScale(1, 0.5)
	controls.Size = UDim2.fromOffset(460, 28)
	controls.Parent = header
	local controlsList = Ui.list(controls, 12, true)
	controlsList.HorizontalAlignment = Enum.HorizontalAlignment.Right

	local segmentsFrame = Instance.new("Frame")
	segmentsFrame.LayoutOrder = 1
	segmentsFrame.AutomaticSize = Enum.AutomaticSize.X
	segmentsFrame.Size = UDim2.fromOffset(0, 26)
	Ui.fill(segmentsFrame, "field_select")
	Ui.corner(segmentsFrame)
	Ui.pad(segmentsFrame, 2)
	Ui.list(segmentsFrame, 2, true)
	segmentsFrame.Parent = controls

	local search = Instance.new("Frame")
	search.LayoutOrder = 2
	search.Size = UDim2.fromOffset(200, 28)
	Ui.fill(search, "field_select")
	Ui.corner(search)
	Ui.stroke(search, "border")
	search.Parent = controls
	local searchBox = Instance.new("TextBox")
	searchBox.BackgroundTransparency = 1
	searchBox.Position = UDim2.fromOffset(10, 0)
	searchBox.Size = UDim2.new(1, -20, 1, 0)
	searchBox.ClearTextOnFocus = false
	searchBox.PlaceholderText = "Filter changes"
	searchBox.PlaceholderColor3 = Ui.color("text3")
	searchBox.Text = ""
	searchBox.TextXAlignment = Enum.TextXAlignment.Left
	searchBox.FontFace = Ui.FONT
	searchBox.TextSize = Ui.TEXT_SM
	searchBox.TextColor3 = Ui.color("text")
	searchBox.Parent = search

	-- Body: the list pane and the detail.
	local list = Instance.new("ScrollingFrame")
	list.Position = UDim2.fromOffset(0, 61)
	list.Size = UDim2.new(0, LIST_WIDTH, 1, -61 - 57)
	list.BackgroundTransparency = 1
	list.BorderSizePixel = 0
	list.CanvasSize = UDim2.new()
	list.AutomaticCanvasSize = Enum.AutomaticSize.Y
	list.ScrollBarThickness = 6
	list.ScrollBarImageColor3 = Ui.color("text3")
	list.ScrollingDirection = Enum.ScrollingDirection.Y
	list.Parent = root
	-- Between the header's hairline and the footer's.
	hairline(root, true, UDim2.new(0, LIST_WIDTH, 0, 61), 61 + 57)

	local detail = Instance.new("Frame")
	detail.Position = UDim2.fromOffset(LIST_WIDTH + 1, 61)
	detail.Size = UDim2.new(1, -LIST_WIDTH - 1, 1, -61 - 57)
	detail.BackgroundTransparency = 1
	detail.Parent = root

	-- Footer: the note and Close. 57 tall with its hairline.
	local footer = Instance.new("Frame")
	footer.AnchorPoint = Vector2.new(0, 1)
	footer.Position = UDim2.fromScale(0, 1)
	footer.Size = UDim2.new(1, 0, 0, 56)
	footer.BackgroundTransparency = 1
	footer.Parent = root
	hairline(root, false, UDim2.new(0, 0, 1, -57))
	Ui.pad(footer, 0, 16)
	local note = Ui.text(footer, FOOTER_NOTE, { size = Ui.TEXT_SM, color = "text3", wrap = false })
	note.Size = UDim2.new(1, -120, 1, 0)
	local close = Ui.button(footer, "Close", true)
	close.Size = UDim2.fromOffset(96, 34)
	close.AnchorPoint = Vector2.new(1, 0.5)
	close.Position = UDim2.fromScale(1, 0.5)
	close.Activated:Connect(function()
		gui.Enabled = false
	end)

	local self: DiffWindow = setmetatable({
		gui = gui,
		changes = {},
		found = {},
		filter = "all" :: Filter,
		query = "",
		selected = nil,
		subtitle = subtitle,
		segments = {},
		counts = {},
		searchBox = searchBox,
		list = list,
		detail = detail,
	}, DiffWindow)

	for index, pair in { { "all", "All" }, { "updated", "Updated" }, { "missing", "Not found" } } do
		local filter, label = pair[1] :: Filter, pair[2]
		local button = Instance.new("TextButton")
		button.AutoButtonColor = false
		button.LayoutOrder = index
		button.AutomaticSize = Enum.AutomaticSize.X
		button.Size = UDim2.fromOffset(0, 22)
		button.Text = ""
		Ui.corner(button, 3)
		Ui.pad(button, 0, 10, 0, 10)
		Ui.list(button, 5, true)
		local name = Ui.text(button, label, { size = Ui.TEXT_XS, wrap = false, order = 1 })
		name.AutomaticSize = Enum.AutomaticSize.X
		name.Size = UDim2.fromOffset(0, 22)
		name.Name = "Label"
		local count = Ui.text(button, "0", { size = Ui.TEXT_XXS, font = Ui.FONT_MONO, color = "text3", wrap = false, order = 2 })
		count.AutomaticSize = Enum.AutomaticSize.X
		count.Size = UDim2.fromOffset(0, 22)
		button.Parent = segmentsFrame
		button.Activated:Connect(function()
			self.filter = filter
			self:render()
		end)
		self.segments[filter] = button
		self.counts[filter] = count
	end

	searchBox:GetPropertyChangedSignal("Text"):Connect(function()
		self.query = string.lower(searchBox.Text)
		self:render()
	end)
	return self
end

-- Shows `entries` (a journal) and raises the window.
function DiffWindow.show(self: DiffWindow, entries: { any })
	self.changes = Changes.fromJournal(entries)
	table.clear(self.found)
	for _, change in self.changes do
		local instance = Paths.resolve(game, change.path)
		if instance then
			self.found[change.path] = instance
		end
	end
	self.selected = nil
	self.gui.Enabled = true
	self:render()
end

function DiffWindow.visible(self: DiffWindow): { Changes.Change }
	local out = {}
	for _, change in self.changes do
		local missing = self.found[change.path] == nil
		local passes = self.filter == "all"
			or (self.filter == "updated" and not missing)
			or (self.filter == "missing" and missing)
		local matches = self.query == ""
			or string.find(string.lower(change.path), self.query, 1, true) ~= nil
		if passes and matches then
			table.insert(out, change)
		end
	end
	return out
end

function DiffWindow.render(self: DiffWindow)
	local updated, missing = 0, 0
	for _, change in self.changes do
		if self.found[change.path] then
			updated += 1
		else
			missing += 1
		end
	end
	local total = updated + missing
	self.subtitle.Text = `{total} {if total == 1 then "change" else "changes"} by collaborators since you were last here`

	local counts: { [Filter]: number } = { all = total, updated = updated, missing = missing }
	for filter, button in self.segments do
		local active = self.filter == filter
		local enabled = counts[filter] > 0
		Ui.fill(button, if active then "accent_soft" else "field_select")
		local label = button:FindFirstChild("Label") :: TextLabel
		label.TextColor3 = Ui.color(if active then "check_on" elseif enabled then "text2" else "text3")
		label.FontFace = if active then Ui.FONT_SEMIBOLD else Ui.FONT
		self.counts[filter].Text = tostring(counts[filter])
		self.counts[filter].TextColor3 = Ui.color(if active then "check_on" else "text3")
	end

	local visible = self:visible()
	local stillVisible = false
	for _, change in visible do
		if change.path == self.selected then
			stillVisible = true
		end
	end
	if not stillVisible then
		self.selected = if #visible > 0 then visible[1].path else nil
	end
	self:renderList(visible)
	self:renderDetail()
end

function DiffWindow.renderList(self: DiffWindow, visible: { Changes.Change })
	for _, child in self.list:GetChildren() do
		child:Destroy()
	end
	Ui.pad(self.list, 10, 8, 12, 8)
	Ui.list(self.list, 2)

	local groups = {
		{ title = "UPDATES", ink = "check_on", marker = "~", missing = false },
		{ title = "NOT FOUND", ink = "text_error", marker = "−", missing = true },
	}
	local order = 0
	for _, group in groups do
		local members = {}
		for _, change in visible do
			if (self.found[change.path] == nil) == group.missing then
				table.insert(members, change)
			end
		end
		if #members > 0 then
			order += 1
			local head = Ui.frame(self.list, order)
			Ui.pad(head, 6, 6, 4, 8)
			sectionLabel(head, group.title, #members, 1, group.ink)
			for _, change in members do
				order += 1
				self:listRow(change, group.marker, order)
			end
		end
	end
	if order == 0 then
		Ui.text(self.list, "No changes match.", { size = Ui.TEXT_SM, color = "text3" })
	end
end

function DiffWindow.listRow(self: DiffWindow, change: Changes.Change, marker: string, order: number)
	local selected = self.selected == change.path
	local row = Instance.new("TextButton")
	row.AutoButtonColor = false
	row.Text = ""
	row.LayoutOrder = order
	row.Size = UDim2.new(1, 0, 0, 40)
	Ui.fill(row, if selected then "ribbon_tab_active" else "dock")
	if not selected then
		row.BackgroundTransparency = 1
	end
	Ui.corner(row)
	row.Parent = self.list

	local mark = Ui.text(row, marker, { size = Ui.TEXT_SM, color = "text3", font = Ui.FONT_MONO, wrap = false })
	mark.Position = UDim2.fromOffset(10, 0)
	mark.Size = UDim2.new(0, 10, 1, 0)
	local instance = self.found[change.path]
	local icon = Ui.classIcon(row, if instance then instance.ClassName else "Folder", 16)
	icon.Position = UDim2.new(0, 26, 0.5, -8)

	local name = Ui.text(row, change.name, { size = Ui.TEXT_SM, font = if selected then Ui.FONT_SEMIBOLD else Ui.FONT, wrap = false })
	name.Position = UDim2.fromOffset(50, 4)
	name.Size = UDim2.new(1, -120, 0, 16)
	local path = Ui.text(row, change.parent, { size = Ui.TEXT_XS, color = "text3", wrap = false })
	path.Position = UDim2.fromOffset(50, 21)
	path.Size = UDim2.new(1, -120, 0, 14)
	local n = #change.properties
	local count = Ui.text(row, `{n} {if n == 1 then "prop" else "props"}`, { size = Ui.TEXT_XS, color = "text3", font = Ui.FONT_MONO, wrap = false })
	count.AnchorPoint = Vector2.new(1, 0)
	count.Position = UDim2.new(1, -10, 0, 0)
	count.Size = UDim2.new(0, 60, 1, 0)
	count.TextXAlignment = Enum.TextXAlignment.Right

	row.MouseEnter:Connect(function()
		if self.selected ~= change.path then
			Ui.fill(row, "hover")
		end
	end)
	row.MouseLeave:Connect(function()
		if self.selected ~= change.path then
			row.BackgroundTransparency = 1
		end
	end)
	row.Activated:Connect(function()
		self.selected = change.path
		self:render()
	end)
end

function DiffWindow.renderDetail(self: DiffWindow)
	for _, child in self.detail:GetChildren() do
		child:Destroy()
	end
	local change: Changes.Change? = nil
	for _, c in self.changes do
		if c.path == self.selected then
			change = c
		end
	end
	if not change then
		local empty = Ui.text(self.detail, "No change selected", { size = Ui.TEXT_MD, color = "text3", wrap = false })
		empty.Size = UDim2.fromScale(1, 1)
		empty.TextXAlignment = Enum.TextXAlignment.Center
		return
	end
	local c = change :: Changes.Change
	local instance = self.found[c.path]

	-- Header: icon tile, name, class badge, kind pill, stats; the path under it.
	local head = Instance.new("Frame")
	head.Size = UDim2.new(1, 0, 0, 74)
	head.BackgroundTransparency = 1
	head.Parent = self.detail
	hairline(self.detail, false, UDim2.fromOffset(0, 74))
	Ui.pad(head, 16, 20, 14, 20)

	local top = Instance.new("Frame")
	top.Size = UDim2.new(1, 0, 0, 28)
	top.BackgroundTransparency = 1
	top.Parent = head
	Ui.list(top, 10, true)
	local tile = Instance.new("Frame")
	tile.Size = UDim2.fromOffset(28, 28)
	tile.LayoutOrder = 1
	Ui.fill(tile, "field_select")
	Ui.corner(tile, Ui.RADIUS_TILE)
	tile.Parent = top
	local icon = Ui.classIcon(tile, if instance then instance.ClassName else "Folder", 16)
	icon.Position = UDim2.fromOffset(6, 6)
	local name = Ui.text(top, c.name, { size = 15, font = Ui.FONT_BOLD, wrap = false, order = 2 })
	name.AutomaticSize = Enum.AutomaticSize.X
	name.Size = UDim2.fromOffset(0, 28)
	if instance then
		local badge = Instance.new("Frame")
		badge.LayoutOrder = 3
		badge.AutomaticSize = Enum.AutomaticSize.X
		badge.Size = UDim2.fromOffset(0, 20)
		badge.BackgroundTransparency = 1
		Ui.corner(badge, Ui.RADIUS_BADGE)
		Ui.stroke(badge, "border2")
		Ui.pad(badge, 0, 6, 0, 6)
		local cls = Ui.text(badge, instance.ClassName, { size = Ui.TEXT_XS, color = "text2", font = Ui.FONT_MONO, wrap = false })
		cls.AutomaticSize = Enum.AutomaticSize.X
		cls.Size = UDim2.fromOffset(0, 20)
		badge.Parent = top
		Ui.pill(top, "Updated", "accent_soft", "check_on", 4)
	else
		Ui.pill(top, "Not found", "diff_remove_pill", "text_error", 4)
	end

	local n = #c.properties
	local stats = Ui.text(head, `{n} {if n == 1 then "property" else "properties"}`, { size = Ui.TEXT_XS, color = "text3", font = Ui.FONT_MONO, wrap = false })
	stats.AnchorPoint = Vector2.new(1, 0)
	stats.Position = UDim2.fromScale(1, 0)
	stats.Size = UDim2.fromOffset(120, 28)
	stats.TextXAlignment = Enum.TextXAlignment.Right

	local path = Ui.text(head, c.path, { size = Ui.TEXT_XS, color = "text3", font = Ui.FONT_MONO, wrap = false })
	path.Position = UDim2.fromOffset(38, 34)
	path.Size = UDim2.new(1, -38 - 80, 0, 16)
	if instance then
		local select = Ui.link(head, "Select")
		select.AnchorPoint = Vector2.new(1, 0)
		select.Position = UDim2.new(1, 0, 0, 34)
		select.Activated:Connect(function()
			game:GetService("Selection"):Set({ instance :: Instance })
		end)
	end

	-- Body: PROPERTIES, then the table, scrolling when long.
	local body = Instance.new("ScrollingFrame")
	body.Position = UDim2.fromOffset(0, 75)
	body.Size = UDim2.new(1, 0, 1, -75)
	body.BackgroundTransparency = 1
	body.BorderSizePixel = 0
	body.CanvasSize = UDim2.new()
	body.AutomaticCanvasSize = Enum.AutomaticSize.Y
	body.ScrollBarThickness = 6
	body.ScrollBarImageColor3 = Ui.color("text3")
	body.ScrollingDirection = Enum.ScrollingDirection.Y
	body.Parent = self.detail
	Ui.pad(body, 16, 20, 20, 20)
	Ui.list(body, 10)
	sectionLabel(body, "PROPERTIES", nil, 1)

	local tableFrame = Ui.frame(body, 2)
	Ui.corner(tableFrame)
	Ui.stroke(tableFrame, "border")
	Ui.list(tableFrame, 0)

	local function cells(row: Frame, height: number)
		local columns = {
			{ UDim2.new(0, 170, 0, height), 0 },
			{ UDim2.new(1, -170 - 150 - 90, 0, height), 1 },
			{ UDim2.new(0, 150, 0, height), 2 },
			{ UDim2.new(0, 90, 0, height), 3 },
		}
		local out = {}
		for _, col in columns do
			local cell = Instance.new("Frame")
			cell.BackgroundTransparency = 1
			cell.Size = col[1]
			cell.LayoutOrder = col[2]
			cell.Parent = row
			local layout = Ui.list(cell, 0, true)
			layout.VerticalAlignment = Enum.VerticalAlignment.Center
			table.insert(out, cell)
		end
		return out
	end

	local headRow = Instance.new("Frame")
	headRow.Size = UDim2.new(1, 0, 0, 28)
	headRow.BackgroundTransparency = 1
	headRow.LayoutOrder = 0
	headRow.Parent = tableFrame
	Ui.pad(headRow, 0, 12)
	Ui.list(headRow, 0, true)
	local headCells = cells(headRow, 28)
	for i, title in { "PROPERTY", "NOW", "EDITED BY", "WHEN" } do
		Ui.text(headCells[i], title, { size = Ui.TEXT_XXS, color = "text3", font = Ui.FONT_BOLD, wrap = false })
	end

	local now = DateTime.now().UnixTimestampMillis
	for index, edit in c.properties do
		local line = Instance.new("Frame")
		line.Size = UDim2.new(1, 0, 0, 1)
		line.LayoutOrder = index * 2 - 1
		Ui.fill(line, "border")
		line.Parent = tableFrame

		local row = Instance.new("Frame")
		row.Size = UDim2.new(1, 0, 0, 36)
		row.BackgroundTransparency = 1
		row.LayoutOrder = index * 2
		row.Parent = tableFrame
		Ui.pad(row, 0, 12)
		Ui.list(row, 0, true)
		local cols = cells(row, 36)
		Ui.text(cols[1], edit.property, { size = Ui.TEXT_SM, wrap = false })
		local ok, value = read(instance, edit.property)
		if ok then
			local text, swatch = format(value)
			Ui.chip(cols[2], text, swatch)
		else
			Ui.text(cols[2], "—", { size = Ui.TEXT_SM, color = "text3", wrap = false })
		end
		local by = table.concat(edit.authors, ", ")
		if edit.edits > 1 then
			by ..= ` ({edit.edits} edits)`
		end
		Ui.text(cols[3], by, { size = Ui.TEXT_XS, color = "text2", wrap = false })
		Ui.text(cols[4], Changes.ago(edit.at, now), { size = Ui.TEXT_XS, color = "text3", font = Ui.FONT_MONO, wrap = false })
	end
end

function DiffWindow.destroy(self: DiffWindow)
	self.gui:Destroy()
end

return DiffWindow
