--!strict
--// The plugin's look: rbx-native's own palette (the generated Theme module,
--// written from assets/themes/default/theme.json) and the few pieces the
--// editor's docks are built from, so the widget and the diff window read as
--// part of rbx-native rather than as stock Studio UI.

local Theme = require(script.Parent.Theme)

local Ui = {}

-- Studio has neither Manrope nor JetBrains Mono; Builder Sans and Builder
-- Mono are its closest.
Ui.FONT = Font.new("rbxasset://fonts/families/BuilderSans.json")
Ui.FONT_SEMIBOLD = Font.new("rbxasset://fonts/families/BuilderSans.json", Enum.FontWeight.SemiBold)
Ui.FONT_BOLD = Font.new("rbxasset://fonts/families/BuilderSans.json", Enum.FontWeight.Bold)
Ui.FONT_MONO = Font.new("rbxasset://fonts/families/BuilderMono.json")

-- Studio sizes text in whole points; these round rbx-native's sizes.
Ui.TEXT_LG = 14
Ui.TEXT_MD = 13
Ui.TEXT_SM = 12
Ui.TEXT_XS = 11
Ui.TEXT_XXS = 10

Ui.RADIUS = Theme.sizes.radius or 5
Ui.RADIUS_BADGE = Theme.sizes.radius_badge or 4
Ui.RADIUS_TILE = Theme.sizes.radius_tile or 6

-- A theme colour as Color3 and as the Transparency that carries its alpha.
function Ui.color(name: string): Color3
	local c = Theme.colors[name]
	if not c then
		return Color3.new(1, 0, 1)
	end
	return Color3.fromRGB(c[1], c[2], c[3])
end

function Ui.alpha(name: string): number
	local c = Theme.colors[name]
	return if c then 1 - c[4] else 0
end

-- `#rrggbb` as Color3 (a collaborator's colour).
function Ui.hex(hex: string): Color3
	local r, g, b = string.match(hex, "^#(%x%x)(%x%x)(%x%x)$")
	if not r then
		return Ui.color("text2")
	end
	return Color3.fromRGB(tonumber(r, 16) :: number, tonumber(g, 16) :: number, tonumber(b, 16) :: number)
end

function Ui.fill(object: GuiObject, name: string)
	object.BackgroundColor3 = Ui.color(name)
	object.BackgroundTransparency = Ui.alpha(name)
	object.BorderSizePixel = 0
end

function Ui.corner(parent: Instance, radius: number?)
	local corner = Instance.new("UICorner")
	corner.CornerRadius = UDim.new(0, radius or Ui.RADIUS)
	corner.Parent = parent
end

function Ui.stroke(parent: Instance, name: string?)
	local stroke = Instance.new("UIStroke")
	stroke.ApplyStrokeMode = Enum.ApplyStrokeMode.Border
	stroke.Color = Ui.color(name or "border")
	stroke.Transparency = Ui.alpha(name or "border")
	stroke.Thickness = 1
	stroke.Parent = parent
	return stroke
end

function Ui.pad(parent: Instance, top: number, right: number?, bottom: number?, left: number?)
	local padding = Instance.new("UIPadding")
	padding.PaddingTop = UDim.new(0, top)
	padding.PaddingRight = UDim.new(0, right or top)
	padding.PaddingBottom = UDim.new(0, bottom or top)
	padding.PaddingLeft = UDim.new(0, left or right or top)
	padding.Parent = parent
end

function Ui.list(parent: Instance, gap: number, horizontal: boolean?): UIListLayout
	local layout = Instance.new("UIListLayout")
	layout.SortOrder = Enum.SortOrder.LayoutOrder
	layout.Padding = UDim.new(0, gap)
	if horizontal then
		layout.FillDirection = Enum.FillDirection.Horizontal
		layout.VerticalAlignment = Enum.VerticalAlignment.Center
	end
	layout.Parent = parent
	return layout
end

-- A transparent box that sizes to its content (vertical stack by default).
function Ui.frame(parent: Instance?, order: number?): Frame
	local frame = Instance.new("Frame")
	frame.BackgroundTransparency = 1
	frame.BorderSizePixel = 0
	frame.LayoutOrder = order or 0
	frame.Size = UDim2.fromScale(1, 0)
	frame.AutomaticSize = Enum.AutomaticSize.Y
	frame.Parent = parent
	return frame
end

export type TextOptions = {
	size: number?,
	color: string?,
	font: Font?,
	order: number?,
	wrap: boolean?,
	width: UDim?,
}

-- A label: full width and wrapping by default, sized to its text.
function Ui.text(parent: Instance?, text: string, options: TextOptions?): TextLabel
	local o: TextOptions = options or {}
	local label = Instance.new("TextLabel")
	label.BackgroundTransparency = 1
	label.BorderSizePixel = 0
	label.LayoutOrder = o.order or 0
	label.FontFace = o.font or Ui.FONT
	label.TextSize = o.size or Ui.TEXT_SM
	label.TextColor3 = Ui.color(o.color or "text")
	label.TextXAlignment = Enum.TextXAlignment.Left
	label.TextYAlignment = Enum.TextYAlignment.Center
	label.RichText = false
	label.Text = text
	if o.wrap == false then
		label.TextWrapped = false
		label.TextTruncate = Enum.TextTruncate.AtEnd
		label.Size = UDim2.new(o.width or UDim.new(1, 0), UDim.new(0, label.TextSize + 4))
	else
		label.TextWrapped = true
		label.Size = UDim2.new(o.width or UDim.new(1, 0), UDim.new(0, 0))
		label.AutomaticSize = Enum.AutomaticSize.Y
	end
	label.Parent = parent
	return label
end

-- A text link: `accent_line` until hovered.
function Ui.link(parent: Instance?, text: string, order: number?): TextButton
	local button = Instance.new("TextButton")
	button.AutoButtonColor = false
	button.BackgroundTransparency = 1
	button.BorderSizePixel = 0
	button.LayoutOrder = order or 0
	button.FontFace = Ui.FONT_SEMIBOLD
	button.TextSize = Ui.TEXT_XS
	button.TextColor3 = Ui.color("check_on")
	button.TextXAlignment = Enum.TextXAlignment.Left
	button.Text = text
	button.AutomaticSize = Enum.AutomaticSize.XY
	button.Size = UDim2.fromOffset(0, 16)
	button.Parent = parent
	button.MouseEnter:Connect(function()
		button.TextColor3 = Ui.color("accent_hover")
	end)
	button.MouseLeave:Connect(function()
		button.TextColor3 = Ui.color("check_on")
	end)
	return button
end

-- rbx-native's buttons: primary is `check_on` with dark bold text, secondary
-- is `field_select` on a `border2` stroke.
function Ui.button(parent: Instance?, text: string, primary: boolean, order: number?): TextButton
	local button = Instance.new("TextButton")
	button.AutoButtonColor = false
	button.LayoutOrder = order or 0
	button.Size = UDim2.new(0, 96, 0, 30)
	button.FontFace = if primary then Ui.FONT_BOLD else Ui.FONT_SEMIBOLD
	button.TextSize = Ui.TEXT_SM
	button.Text = text
	local idle = if primary then "check_on" else "field_select"
	local hover = if primary then "accent_hover" else "secondary_hover"
	Ui.fill(button, idle)
	button.TextColor3 = Ui.color(if primary then "black" else "text")
	Ui.corner(button)
	if not primary then
		Ui.stroke(button, "border2")
	end
	button.MouseEnter:Connect(function()
		Ui.fill(button, hover)
	end)
	button.MouseLeave:Connect(function()
		Ui.fill(button, idle)
	end)
	button.Parent = parent
	return button
end

-- A bordered rounded box with its own vertical stack (`gap` apart).
function Ui.card(parent: Instance?, order: number?, gap: number?): Frame
	local card = Ui.frame(parent, order)
	Ui.corner(card)
	Ui.stroke(card, "border")
	Ui.pad(card, 8, 10)
	Ui.list(card, gap or 4)
	return card
end

-- The 6px (or `size`) round dot a kind or a collaborator is marked with.
function Ui.dot(parent: Instance?, color: Color3, size: number?): Frame
	local dot = Instance.new("Frame")
	dot.Size = UDim2.fromOffset(size or 6, size or 6)
	dot.BackgroundColor3 = color
	dot.BorderSizePixel = 0
	Ui.corner(dot, 999)
	dot.Parent = parent
	return dot
end

export type Switch = {
	frame: TextButton,
	set: (boolean) -> (),
}

-- The dock's on/off switch: a 28x16 track, `check_on` when on, with a white
-- knob; clicking calls `onClick` (the caller sets the state).
function Ui.switch(parent: Instance?, onClick: () -> (), order: number?): Switch
	local track = Instance.new("TextButton")
	track.AutoButtonColor = false
	track.Text = ""
	track.LayoutOrder = order or 0
	track.Size = UDim2.fromOffset(28, 16)
	Ui.corner(track, 999)
	local knob = Instance.new("Frame")
	knob.Size = UDim2.fromOffset(12, 12)
	knob.AnchorPoint = Vector2.new(0, 0.5)
	knob.BorderSizePixel = 0
	knob.BackgroundColor3 = Ui.color("knob")
	Ui.corner(knob, 999)
	knob.Parent = track
	local function set(on: boolean)
		Ui.fill(track, if on then "check_on" else "track")
		knob.Position = if on then UDim2.new(1, -14, 0.5, 0) else UDim2.new(0, 2, 0.5, 0)
	end
	set(false)
	track.Activated:Connect(onClick)
	track.Parent = parent
	return { frame = track, set = set }
end

-- A mono value chip, as the diff table's: `diff_add_soft` behind `text`, a
-- 10x10 swatch first when `swatch` is given.
function Ui.chip(parent: Instance?, text: string, swatch: Color3?, order: number?): Frame
	local chip = Instance.new("Frame")
	chip.LayoutOrder = order or 0
	chip.AutomaticSize = Enum.AutomaticSize.X
	chip.Size = UDim2.fromOffset(0, 22)
	Ui.fill(chip, "diff_add_soft")
	Ui.corner(chip, Ui.RADIUS_BADGE)
	Ui.pad(chip, 0, 7, 0, 7)
	Ui.list(chip, 6, true)
	if swatch then
		local box = Instance.new("Frame")
		box.Size = UDim2.fromOffset(10, 10)
		box.BackgroundColor3 = swatch
		box.BorderSizePixel = 0
		Ui.corner(box, 2)
		Ui.stroke(box, "border2")
		box.Parent = chip
	end
	local label = Ui.text(chip, text, { font = Ui.FONT_MONO, size = Ui.TEXT_SM, wrap = false, order = 1 })
	label.AutomaticSize = Enum.AutomaticSize.X
	label.Size = UDim2.fromOffset(0, 22)
	label.TextTruncate = Enum.TextTruncate.None
	chip.Parent = parent
	return chip
end

-- A small rounded pill: `bg` behind `ink` text, a dot first.
function Ui.pill(parent: Instance?, text: string, bg: string, ink: string, order: number?): Frame
	local pill = Instance.new("Frame")
	pill.LayoutOrder = order or 0
	pill.AutomaticSize = Enum.AutomaticSize.X
	pill.Size = UDim2.fromOffset(0, 20)
	Ui.fill(pill, bg)
	Ui.corner(pill, Ui.RADIUS_BADGE)
	Ui.pad(pill, 0, 8, 0, 8)
	Ui.list(pill, 6, true)
	Ui.dot(pill, Ui.color(ink))
	local label = Ui.text(pill, text, { font = Ui.FONT_SEMIBOLD, size = Ui.TEXT_XS, color = ink, wrap = false, order = 1 })
	label.AutomaticSize = Enum.AutomaticSize.X
	label.Size = UDim2.fromOffset(0, 20)
	label.TextTruncate = Enum.TextTruncate.None
	pill.Parent = parent
	return pill
end

-- A collaborator as Studio shows one in a Team Create session: their round
-- Roblox headshot in a ring of their presence colour (the colour behind it
-- until the headshot loads).
function Ui.avatar(parent: Instance?, userId: number, color: Color3, size: number?, order: number?): ImageLabel
	local s = size or 22
	local image = Instance.new("ImageLabel")
	image.LayoutOrder = order or 0
	image.Size = UDim2.fromOffset(s, s)
	image.BackgroundColor3 = color
	image.BorderSizePixel = 0
	image.Image = string.format("rbxthumb://type=AvatarHeadShot&id=%.0f&w=48&h=48", userId)
	Ui.corner(image, 999)
	local ring = Instance.new("UIStroke")
	ring.Color = color
	ring.Thickness = 2
	ring.ApplyStrokeMode = Enum.ApplyStrokeMode.Border
	ring.Parent = image
	image.Parent = parent
	return image
end

-- The Explorer's own icon for `class`, from StudioService.
function Ui.classIcon(parent: Instance?, class: string, size: number?): ImageLabel
	local image = Instance.new("ImageLabel")
	image.BackgroundTransparency = 1
	image.Size = UDim2.fromOffset(size or 16, size or 16)
	local ok, icon = pcall(function()
		return game:GetService("StudioService"):GetClassIcon(class)
	end)
	if ok and type(icon) == "table" then
		for key, value in icon :: any do
			pcall(function()
				(image :: any)[key] = value
			end)
		end
	end
	image.Parent = parent
	return image
end

return Ui
