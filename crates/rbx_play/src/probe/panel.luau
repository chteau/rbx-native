-- The in-game dev panel: the editor's Output, playtest stats and debugger
-- controls drawn inside the Roblox client, for when the game can't be
-- shown in the editor's viewport (macOS runs Roblox in its own window,
-- often full screen, with the editor out of sight).
--
-- Off unless the module carries a THEME attribute: the editor's theme
-- colours as JSON, so the panel looks like the editor whatever theme is on.
-- The server half relays what the probe already collects (logs from both
-- sides, stats, debugger pauses) to the tester; the client half draws it.

local HttpService = game:GetService("HttpService")
local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local UserInputService = game:GetService("UserInputService")

local M = {}

local MAX_ROWS = 500
local TOGGLE_KEY = Enum.KeyCode.F8

local function theme()
	local raw = script:GetAttribute("THEME")
	if type(raw) ~= "string" or raw == "" then
		return nil
	end
	local ok, decoded = pcall(HttpService.JSONDecode, HttpService, raw)
	if ok and type(decoded) == "table" then
		return decoded
	end
	return nil
end

function M.enabled()
	return theme() ~= nil
end

-- Server: forwards the probe's own messages to the tester, batched once a
-- frame so a print loop is one remote call per frame, not one per line.
function M.server(remote, getTester)
	if not M.enabled() then
		return nil
	end
	local pending = {}
	RunService.Heartbeat:Connect(function()
		if #pending == 0 then
			return
		end
		local tester = getTester()
		if tester then
			remote:FireClient(tester, { kind = "panel", items = pending })
		end
		pending = {}
	end)
	return function(message)
		local kind = message.type
		if kind == "log" or kind == "stats" or kind == "paused" then
			table.insert(pending, message)
		end
	end
end

-- "#rrggbb" or "#rrggbbaa" -> Color3, transparency.
local function parse(hex, fallback)
	if type(hex) ~= "string" then
		return fallback, 0
	end
	local r, g, b = tonumber(hex:sub(2, 3), 16), tonumber(hex:sub(4, 5), 16), tonumber(hex:sub(6, 7), 16)
	if not (r and g and b) then
		return fallback, 0
	end
	local a = tonumber(hex:sub(8, 9), 16) or 255
	return Color3.fromRGB(r, g, b), 1 - a / 255
end

function M.client(remote, Debug)
	local colors = theme()
	if not colors then
		return
	end
	local player = Players.LocalPlayer
	local function c(name)
		local color = parse(colors[name], Color3.new(1, 0, 1))
		return color
	end
	local function alpha(name)
		local _, transparency = parse(colors[name], Color3.new())
		return transparency
	end

	-- Roblox can't load the editor's Manrope / JetBrains Mono: its own
	-- neutral sans and Roboto Mono are the closest it ships.
	local SANS = "rbxasset://fonts/families/BuilderSans.json"
	local MONO = "rbxasset://fonts/families/RobotoMono.json"
	local function font(family, weight)
		return Font.new(family, weight or Enum.FontWeight.Regular)
	end

	local function new(class, props, children)
		local instance = Instance.new(class)
		for key, value in props do
			instance[key] = value
		end
		for _, child in children or {} do
			child.Parent = instance
		end
		return instance
	end
	local function corner(radius)
		return new("UICorner", { CornerRadius = UDim.new(0, radius) })
	end
	local function stroke(name)
		return new("UIStroke", {
			Color = c(name),
			Transparency = alpha(name),
			Thickness = 1,
			ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
		})
	end
	local function pad(x, y)
		return new("UIPadding", {
			PaddingLeft = UDim.new(0, x),
			PaddingRight = UDim.new(0, x),
			PaddingTop = UDim.new(0, y),
			PaddingBottom = UDim.new(0, y),
		})
	end
	local function label(text, size, color, weight, family)
		return new("TextLabel", {
			BackgroundTransparency = 1,
			Text = text,
			TextSize = size,
			TextColor3 = c(color),
			FontFace = font(family or SANS, weight),
			AutomaticSize = Enum.AutomaticSize.XY,
			Size = UDim2.new(),
		})
	end

	local gui = new("ScreenGui", {
		Name = "__rbxnative_panel",
		ResetOnSpawn = false,
		DisplayOrder = 1000,
		ZIndexBehavior = Enum.ZIndexBehavior.Sibling,
		ScreenInsets = Enum.ScreenInsets.DeviceSafeInsets,
	})

	-- The chip that opens it, bottom left, away from Roblox's own buttons.
	local errorsBadge = new("TextLabel", {
		Name = "Badge",
		Visible = false,
		BackgroundColor3 = c("error"),
		Size = UDim2.fromOffset(0, 16),
		AutomaticSize = Enum.AutomaticSize.X,
		Text = "0",
		TextSize = 11,
		TextColor3 = Color3.new(1, 1, 1),
		FontFace = font(SANS, Enum.FontWeight.Bold),
		LayoutOrder = 3,
	}, { corner(8), pad(5, 0) })
	local chip = new("TextButton", {
		Name = "Chip",
		AnchorPoint = Vector2.new(0, 1),
		Position = UDim2.new(0, 10, 1, -10),
		Size = UDim2.fromOffset(0, 30),
		AutomaticSize = Enum.AutomaticSize.X,
		BackgroundColor3 = c("dock"),
		BackgroundTransparency = alpha("dock"),
		Text = "",
		AutoButtonColor = false,
		Parent = gui,
	}, {
		corner(8),
		stroke("border2"),
		pad(10, 0),
		new("UIListLayout", {
			FillDirection = Enum.FillDirection.Horizontal,
			VerticalAlignment = Enum.VerticalAlignment.Center,
			Padding = UDim.new(0, 7),
			SortOrder = Enum.SortOrder.LayoutOrder,
		}),
		new("Frame", {
			Size = UDim2.fromOffset(7, 7),
			BackgroundColor3 = c("accent"),
			LayoutOrder = 1,
		}, { corner(4) }),
		new("TextLabel", {
			BackgroundTransparency = 1,
			AutomaticSize = Enum.AutomaticSize.XY,
			Size = UDim2.new(),
			Text = "RbxNative",
			TextSize = 13,
			TextColor3 = c("text"),
			FontFace = font(SANS, Enum.FontWeight.SemiBold),
			LayoutOrder = 2,
		}),
		errorsBadge,
		new("TextLabel", {
			BackgroundTransparency = 1,
			AutomaticSize = Enum.AutomaticSize.XY,
			Size = UDim2.new(),
			Text = "F8",
			TextSize = 11,
			TextColor3 = c("text3"),
			FontFace = font(MONO),
			LayoutOrder = 4,
		}),
	})
	chip.MouseEnter:Connect(function()
		chip.BackgroundColor3 = c("hover")
	end)
	chip.MouseLeave:Connect(function()
		chip.BackgroundColor3 = c("dock")
	end)

	-- The panel: a dock on the right, the editor's Output dock in look.
	local panel = new("Frame", {
		Name = "Panel",
		Visible = false,
		AnchorPoint = Vector2.new(1, 0),
		Position = UDim2.new(1, -10, 0, 10),
		Size = UDim2.new(0.42, 0, 1, -20),
		BackgroundColor3 = c("dock"),
		BackgroundTransparency = alpha("dock"),
		ClipsDescendants = true,
		Parent = gui,
	}, {
		corner(10),
		stroke("border2"),
		new("UISizeConstraint", { MinSize = Vector2.new(320, 240), MaxSize = Vector2.new(520, math.huge) }),
		new("UIListLayout", { SortOrder = Enum.SortOrder.LayoutOrder }),
	})

	-- Header: tabs, then Clear and close at the right, over a hairline.
	local header = new("Frame", {
		Name = "Header",
		Size = UDim2.new(1, 0, 0, 38),
		BackgroundColor3 = c("chrome"),
		BackgroundTransparency = alpha("chrome"),
		LayoutOrder = 1,
		Parent = panel,
	}, {
		new("Frame", {
			Name = "Hairline",
			AnchorPoint = Vector2.new(0, 1),
			Position = UDim2.new(0, 0, 1, 0),
			Size = UDim2.new(1, 0, 0, 1),
			BackgroundColor3 = c("border"),
			BackgroundTransparency = alpha("border"),
			BorderSizePixel = 0,
		}),
	})
	local tabs = new("Frame", {
		BackgroundTransparency = 1,
		Size = UDim2.new(1, -120, 1, 0),
		Position = UDim2.fromOffset(6, 0),
		Parent = header,
	}, {
		new("UIListLayout", {
			FillDirection = Enum.FillDirection.Horizontal,
			SortOrder = Enum.SortOrder.LayoutOrder,
		}),
	})
	local actions = new("Frame", {
		BackgroundTransparency = 1,
		AnchorPoint = Vector2.new(1, 0),
		Position = UDim2.new(1, -8, 0, 0),
		Size = UDim2.new(0, 120, 1, 0),
		Parent = header,
	}, {
		new("UIListLayout", {
			FillDirection = Enum.FillDirection.Horizontal,
			HorizontalAlignment = Enum.HorizontalAlignment.Right,
			VerticalAlignment = Enum.VerticalAlignment.Center,
			Padding = UDim.new(0, 4),
			SortOrder = Enum.SortOrder.LayoutOrder,
		}),
	})

	local function ghost(text, order)
		local button = new("TextButton", {
			Size = UDim2.fromOffset(0, 24),
			AutomaticSize = Enum.AutomaticSize.X,
			BackgroundColor3 = c("hover"),
			BackgroundTransparency = 1,
			AutoButtonColor = false,
			Text = text,
			TextSize = 12,
			TextColor3 = c("text2"),
			FontFace = font(SANS, Enum.FontWeight.Medium),
			LayoutOrder = order,
		}, { corner(6), pad(8, 0) })
		button.MouseEnter:Connect(function()
			button.BackgroundTransparency = alpha("hover")
		end)
		button.MouseLeave:Connect(function()
			button.BackgroundTransparency = 1
		end)
		return button
	end
	local clear = ghost("Clear", 1)
	clear.Parent = actions
	local close = ghost("\u{2715}", 2)
	close.Parent = actions

	-- The debugger's pause, with the editor's stepping buttons: with Roblox
	-- full screen the editor is out of reach, and a paused game is frozen.
	local pauseBar = new("Frame", {
		Name = "Paused",
		Visible = false,
		Size = UDim2.new(1, 0, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
		BackgroundColor3 = c("warning"),
		BackgroundTransparency = 0.82,
		LayoutOrder = 2,
		Parent = panel,
	}, {
		pad(12, 8),
		new("UIListLayout", {
			SortOrder = Enum.SortOrder.LayoutOrder,
			Padding = UDim.new(0, 6),
		}),
	})
	local pauseText = label("", 12.5, "text", Enum.FontWeight.SemiBold)
	pauseText.LayoutOrder = 1
	pauseText.Parent = pauseBar
	local stepRow = new("Frame", {
		BackgroundTransparency = 1,
		Size = UDim2.new(1, 0, 0, 24),
		LayoutOrder = 2,
		Parent = pauseBar,
	}, {
		new("UIListLayout", {
			FillDirection = Enum.FillDirection.Horizontal,
			Padding = UDim.new(0, 6),
			SortOrder = Enum.SortOrder.LayoutOrder,
		}),
	})

	-- Output filters: kind chips, then context chips, then search.
	local filters = new("Frame", {
		Name = "Filters",
		BackgroundTransparency = 1,
		Size = UDim2.new(1, 0, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
		LayoutOrder = 3,
		Parent = panel,
	}, {
		pad(10, 8),
		new("UIListLayout", {
			FillDirection = Enum.FillDirection.Horizontal,
			Wraps = true,
			Padding = UDim.new(0, 4),
			VerticalAlignment = Enum.VerticalAlignment.Center,
			SortOrder = Enum.SortOrder.LayoutOrder,
		}),
	})

	local body = new("Frame", {
		Name = "Body",
		BackgroundTransparency = 1,
		Size = UDim2.new(1, 0, 1, -120),
		LayoutOrder = 4,
		Parent = panel,
	}, {
		new("UIFlexItem", { FlexMode = Enum.UIFlexMode.Fill }),
	})

	local list = new("ScrollingFrame", {
		Name = "Output",
		BackgroundTransparency = 1,
		BorderSizePixel = 0,
		Size = UDim2.fromScale(1, 1),
		CanvasSize = UDim2.new(),
		AutomaticCanvasSize = Enum.AutomaticSize.Y,
		ScrollBarThickness = 6,
		ScrollBarImageColor3 = c("text3"),
		ScrollingDirection = Enum.ScrollingDirection.Y,
		Parent = body,
	}, {
		pad(10, 4),
		new("UIListLayout", { SortOrder = Enum.SortOrder.LayoutOrder, Padding = UDim.new(0, 2) }),
	})
	local empty = label("Nothing printed yet.", 12, "text3")
	empty.LayoutOrder = -1
	empty.Parent = list

	local statsView = new("Frame", {
		Name = "Stats",
		Visible = false,
		BackgroundTransparency = 1,
		Size = UDim2.fromScale(1, 1),
		Parent = body,
	}, {
		pad(10, 10),
		new("UIListLayout", {
			FillDirection = Enum.FillDirection.Horizontal,
			Padding = UDim.new(0, 8),
			SortOrder = Enum.SortOrder.LayoutOrder,
		}),
	})

	-- State.
	local rows = {}
	local order = 0
	local errors = 0
	local kindFilter = "all"
	local sideFilter = "all"
	local query = ""
	local tab = "output"

	local levelColor = {
		output = "text",
		info = "info",
		warning = "warning",
		error = "error",
	}

	local function shown(row)
		if kindFilter ~= "all" and row.level ~= kindFilter then
			return false
		end
		if sideFilter ~= "all" and row.side ~= sideFilter then
			return false
		end
		if query ~= "" and not string.find(string.lower(row.message), query, 1, true) then
			return false
		end
		return true
	end
	local function refilter()
		for _, row in rows do
			row.frame.Visible = shown(row)
		end
		empty.Visible = #rows == 0
	end

	local function stamp(seconds)
		seconds = tonumber(seconds) or 0
		return string.format("%02d:%05.2f", math.floor(seconds / 60), seconds % 60)
	end

	local function addRow(log)
		local message = tostring(log.message or "")
		local level = levelColor[log.level] and log.level or "output"
		local side = log.side == "client" and "client" or "server"
		order += 1
		local atBottom = list.CanvasPosition.Y >= list.AbsoluteCanvasSize.Y - list.AbsoluteWindowSize.Y - 4
		local frame = new("Frame", {
			BackgroundTransparency = 1,
			Size = UDim2.new(1, 0, 0, 0),
			AutomaticSize = Enum.AutomaticSize.Y,
			LayoutOrder = order,
			Parent = list,
		}, {
			new("UIListLayout", {
				FillDirection = Enum.FillDirection.Horizontal,
				Padding = UDim.new(0, 8),
				SortOrder = Enum.SortOrder.LayoutOrder,
			}),
			new("TextLabel", {
				BackgroundTransparency = 1,
				Size = UDim2.fromOffset(62, 18),
				Text = stamp(log.time),
				TextSize = 11,
				TextColor3 = c("text3"),
				FontFace = font(MONO),
				TextXAlignment = Enum.TextXAlignment.Left,
				LayoutOrder = 1,
			}),
			new("Frame", {
				Size = UDim2.fromOffset(6, 18),
				BackgroundTransparency = 1,
				LayoutOrder = 2,
			}, {
				new("Frame", {
					AnchorPoint = Vector2.new(0.5, 0.5),
					Position = UDim2.fromScale(0.5, 0.5),
					Size = UDim2.fromOffset(6, 6),
					BackgroundColor3 = c(side),
				}, { corner(3) }),
			}),
			new("TextLabel", {
				BackgroundTransparency = 1,
				Size = UDim2.new(1, -84, 0, 0),
				AutomaticSize = Enum.AutomaticSize.Y,
				Text = message,
				TextSize = 12,
				TextColor3 = c(levelColor[level]),
				FontFace = font(MONO),
				TextWrapped = true,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextYAlignment = Enum.TextYAlignment.Top,
				RichText = false,
				LayoutOrder = 3,
			}, { new("UIPadding", { PaddingTop = UDim.new(0, 2) }) }),
		})
		local row = { frame = frame, level = level, side = side, message = message }
		table.insert(rows, row)
		frame.Visible = shown(row)
		if #rows > MAX_ROWS then
			table.remove(rows, 1).frame:Destroy()
		end
		empty.Visible = false
		if level == "error" then
			errors += 1
			errorsBadge.Text = tostring(errors)
			errorsBadge.Visible = not panel.Visible
		end
		if atBottom then
			task.defer(function()
				list.CanvasPosition = Vector2.new(0, math.max(0, list.AbsoluteCanvasSize.Y - list.AbsoluteWindowSize.Y))
			end)
		end
	end

	-- Chips: one group shares a selection, the editor's filter look.
	local function chipGroup(items, initial, onPick, startOrder)
		local buttons = {}
		local function paint(value)
			for key, button in buttons do
				local on = key == value
				button.BackgroundColor3 = on and c("accent_soft") or c("field")
				button.BackgroundTransparency = on and alpha("accent_soft") or alpha("field")
				button.TextColor3 = on and c("accent") or c("text2")
			end
		end
		for i, item in items do
			local button = new("TextButton", {
				Size = UDim2.fromOffset(0, 24),
				AutomaticSize = Enum.AutomaticSize.X,
				AutoButtonColor = false,
				Text = item[2],
				TextSize = 12,
				FontFace = font(SANS, Enum.FontWeight.Medium),
				LayoutOrder = startOrder + i,
				Parent = filters,
			}, { corner(6), pad(9, 0) })
			buttons[item[1]] = button
			button.Activated:Connect(function()
				paint(item[1])
				onPick(item[1])
			end)
		end
		paint(initial)
		return buttons
	end
	chipGroup({ { "all", "All" }, { "output", "Output" }, { "info", "Info" }, { "warning", "Warnings" }, { "error", "Errors" } }, "all", function(value)
		kindFilter = value
		refilter()
	end, 0)
	new("Frame", { Size = UDim2.fromOffset(1, 16), BackgroundColor3 = c("border2"), LayoutOrder = 10, Parent = filters })
	chipGroup({ { "all", "All contexts" }, { "client", "Client" }, { "server", "Server" } }, "all", function(value)
		sideFilter = value
		refilter()
	end, 10)
	local search = new("TextBox", {
		Size = UDim2.new(1, 0, 0, 26),
		BackgroundColor3 = c("field"),
		BackgroundTransparency = alpha("field"),
		PlaceholderText = "Search",
		PlaceholderColor3 = c("text3"),
		Text = "",
		TextSize = 12,
		TextColor3 = c("text"),
		FontFace = font(SANS),
		TextXAlignment = Enum.TextXAlignment.Left,
		ClearTextOnFocus = false,
		LayoutOrder = 30,
		Parent = filters,
	}, { corner(6), pad(9, 0) })
	local searchStroke = stroke("border2")
	searchStroke.Parent = search
	search.Focused:Connect(function()
		searchStroke.Color = c("accent")
	end)
	search.FocusLost:Connect(function()
		searchStroke.Color = c("border2")
	end)
	search:GetPropertyChangedSignal("Text"):Connect(function()
		query = string.lower(search.Text)
		refilter()
	end)

	-- Stats: client and server cards.
	local statRows = { client = {}, server = {} }
	local statNames = {
		{ "fps", "FPS", "%.0f" },
		{ "ping_ms", "Ping", "%.0f ms" },
		{ "frame_ms", "Frame", "%.1f ms" },
		{ "physics_fps", "Physics", "%.0f FPS" },
		{ "memory_mb", "Memory", "%.0f MB" },
		{ "instances", "Instances", "%d" },
		{ "send_kbps", "Send", "%.1f KB/s" },
		{ "receive_kbps", "Receive", "%.1f KB/s" },
		{ "players", "Players", "%d" },
	}
	for i, side in { "client", "server" } do
		local card = new("Frame", {
			Size = UDim2.new(0.5, -4, 0, 0),
			AutomaticSize = Enum.AutomaticSize.Y,
			BackgroundColor3 = c("field"),
			BackgroundTransparency = alpha("field"),
			LayoutOrder = i,
			Parent = statsView,
		}, {
			corner(8),
			pad(12, 10),
			new("UIListLayout", { SortOrder = Enum.SortOrder.LayoutOrder, Padding = UDim.new(0, 6) }),
		})
		local title = label(side == "client" and "Client" or "Server", 12.5, "text", Enum.FontWeight.SemiBold)
		title.LayoutOrder = 0
		title.Parent = card
		for j, stat in statNames do
			local line = new("Frame", {
				BackgroundTransparency = 1,
				Size = UDim2.new(1, 0, 0, 16),
				LayoutOrder = j,
				Visible = false,
				Parent = card,
			})
			local name = label(stat[2], 12, "text2")
			name.Parent = line
			local value = label("", 12, "text", Enum.FontWeight.Medium, MONO)
			value.AnchorPoint = Vector2.new(1, 0)
			value.Position = UDim2.fromScale(1, 0)
			value.Parent = line
			statRows[side][stat[1]] = { line = line, value = value, format = stat[3] }
		end
	end
	local function setStats(side, metrics)
		local cards = statRows[side]
		if not cards or type(metrics) ~= "table" then
			return
		end
		for key, entry in cards do
			local v = metrics[key]
			if type(v) == "number" then
				entry.line.Visible = true
				entry.value.Text = string.format(entry.format, v)
			end
		end
	end

	-- Tabs.
	local tabButtons = {}
	local function selectTab(name)
		tab = name
		list.Visible = name == "output"
		filters.Visible = name == "output"
		statsView.Visible = name == "stats"
		clear.Visible = name == "output"
		for key, button in tabButtons do
			local on = key == name
			button.TextColor3 = on and c("text") or c("text2")
			button.Underline.Visible = on
		end
	end
	for i, item in { { "output", "Output" }, { "stats", "Stats" } } do
		local button = new("TextButton", {
			Size = UDim2.new(0, 0, 1, 0),
			AutomaticSize = Enum.AutomaticSize.X,
			BackgroundTransparency = 1,
			AutoButtonColor = false,
			Text = item[2],
			TextSize = 13,
			FontFace = font(SANS, Enum.FontWeight.SemiBold),
			LayoutOrder = i,
			Parent = tabs,
		}, {
			pad(10, 0),
			new("Frame", {
				Name = "Underline",
				AnchorPoint = Vector2.new(0.5, 1),
				Position = UDim2.new(0.5, 0, 1, 0),
				Size = UDim2.new(1, -12, 0, 2),
				BackgroundColor3 = c("accent"),
				BorderSizePixel = 0,
			}, { corner(1) }),
		})
		tabButtons[item[1]] = button
		button.Activated:Connect(function()
			selectTab(item[1])
		end)
	end
	selectTab("output")

	local function setOpen(open)
		panel.Visible = open
		chip.Visible = not open
		if open then
			errorsBadge.Visible = false
		else
			errorsBadge.Visible = errors > 0
		end
	end
	chip.Activated:Connect(function()
		setOpen(true)
	end)
	close.Activated:Connect(function()
		setOpen(false)
	end)
	clear.Activated:Connect(function()
		for _, row in rows do
			row.frame:Destroy()
		end
		rows = {}
		errors = 0
		errorsBadge.Text = "0"
		refilter()
	end)
	UserInputService.InputBegan:Connect(function(input, processed)
		if not processed and input.KeyCode == TOGGLE_KEY then
			setOpen(not panel.Visible)
		end
	end)

	-- Debugger pause: stepping from here, as the editor's toolbar does.
	local current = nil
	local function resume(action)
		if not current then
			return
		end
		local cmd = { type = "debug_resume", pause = current.pause, action = action }
		if current.side == "client" then
			Debug.command(cmd)
		else
			remote:FireServer({ kind = "panel_resume", command = cmd })
		end
		current = nil
		pauseBar.Visible = false
	end
	for i, item in {
		{ "continue", "Continue" },
		{ "step_over", "Step over" },
		{ "step_into", "Step into" },
		{ "step_out", "Step out" },
	} do
		local button = new("TextButton", {
			Size = UDim2.fromOffset(0, 24),
			AutomaticSize = Enum.AutomaticSize.X,
			BackgroundColor3 = i == 1 and c("accent") or c("field"),
			AutoButtonColor = false,
			Text = item[2],
			TextSize = 12,
			TextColor3 = i == 1 and Color3.new(1, 1, 1) or c("text"),
			FontFace = font(SANS, Enum.FontWeight.SemiBold),
			LayoutOrder = i,
			Parent = stepRow,
		}, { corner(6), pad(10, 0) })
		button.Activated:Connect(function()
			resume(item[1])
		end)
	end
	local function paused(message)
		current = { pause = message.pause, side = message.side == "client" and "client" or "server" }
		pauseText.Text = string.format(
			"Paused on line %s (%s). Resume here or in the editor.",
			tostring(message.line or "?"),
			current.side
		)
		pauseBar.Visible = true
		setOpen(true)
	end

	remote.OnClientEvent:Connect(function(payload)
		if type(payload) ~= "table" or payload.kind ~= "panel" or type(payload.items) ~= "table" then
			return
		end
		for _, item in payload.items do
			if type(item) == "table" then
				if item.type == "log" then
					addRow(item)
				elseif item.type == "stats" then
					setStats(item.side, item.metrics)
				elseif item.type == "paused" then
					paused(item)
				end
			end
		end
	end)

	gui.Parent = player:WaitForChild("PlayerGui")
end

return M
