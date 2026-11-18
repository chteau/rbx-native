--!strict
-- The whole place is this screen: what the money is for, one button per
-- developer product, the donor board, and a thank-you after a purchase.
-- Characters don't load (Players.CharacterAutoLoads is off), so there's no
-- world to wander, only this.
local HttpService = game:GetService("HttpService")
local MarketplaceService = game:GetService("MarketplaceService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Config = require(ReplicatedStorage:WaitForChild("Config"))
local remotes = ReplicatedStorage:WaitForChild("Donations")
local thanked = remotes:WaitForChild("Thanked") :: RemoteEvent
local board = remotes:WaitForChild("Board") :: StringValue

local player = Players.LocalPlayer

-- The editor's colours (assets/themes/default/theme.json).
local BLACK = Color3.fromRGB(10, 10, 11)
local PANEL = Color3.fromRGB(18, 18, 19)
local LINE = Color3.fromRGB(44, 44, 46)
local WHITE = Color3.fromRGB(244, 244, 246)
local DIM = Color3.fromRGB(166, 166, 174)
local INDIGO = Color3.fromRGB(108, 127, 219)
local FONT = Font.fromEnum(Enum.Font.BuilderSans)

local function make(class: string, props: { [string]: any }, children: { Instance }?): any
	local object = Instance.new(class)
	for key, value in props do
		(object :: any)[key] = value
	end
	for _, child in (children or {}) :: { Instance } do
		child.Parent = object
	end
	return object
end

local function corner(radius: number)
	return make("UICorner", { CornerRadius = UDim.new(0, radius) })
end

local function text(props: { [string]: any }): TextLabel
	local label = make("TextLabel", {
		BackgroundTransparency = 1,
		FontFace = FONT,
		TextColor3 = WHITE,
		TextXAlignment = Enum.TextXAlignment.Left,
		TextWrapped = true,
		AutomaticSize = Enum.AutomaticSize.Y,
		Size = UDim2.fromScale(1, 0),
	})
	for key, value in props do
		(label :: any)[key] = value
	end
	return label
end

local screen = make("ScreenGui", {
	Name = "Donate",
	IgnoreGuiInset = true,
	ResetOnSpawn = false,
	ZIndexBehavior = Enum.ZIndexBehavior.Sibling,
})
make("Frame", { BackgroundColor3 = BLACK, Size = UDim2.fromScale(1, 1), BorderSizePixel = 0, Parent = screen })

local card = make("Frame", {
	BackgroundColor3 = PANEL,
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.new(1, -32, 0, 0),
	AutomaticSize = Enum.AutomaticSize.Y,
	Parent = screen,
}, {
	corner(10),
	make("UIStroke", { Color = LINE, Thickness = 1 }),
	make("UISizeConstraint", { MaxSize = Vector2.new(560, math.huge) }),
	make("UIPadding", {
		PaddingTop = UDim.new(0, 28),
		PaddingBottom = UDim.new(0, 28),
		PaddingLeft = UDim.new(0, 28),
		PaddingRight = UDim.new(0, 28),
	}),
	make("UIListLayout", { Padding = UDim.new(0, 14), SortOrder = Enum.SortOrder.LayoutOrder }),
})

text({ Text = "Support RbxNative", TextSize = 32, FontFace = Font.fromEnum(Enum.Font.BuilderSansBold), LayoutOrder = 1, Parent = card })
text({
	Text = "RbxNative is a free, open source Roblox editor. Every Robux here goes to its maintainers. A donation doesn't unlock anything, it keeps the project going. Thank you!",
	TextSize = 18,
	TextColor3 = DIM,
	LayoutOrder = 2,
	Parent = card,
})

local buttons = make("Frame", {
	BackgroundTransparency = 1,
	Size = UDim2.fromScale(1, 0),
	AutomaticSize = Enum.AutomaticSize.Y,
	LayoutOrder = 3,
	Parent = card,
}, {
	make("UIListLayout", {
		FillDirection = Enum.FillDirection.Horizontal,
		Wraps = true,
		Padding = UDim.new(0, 10),
		SortOrder = Enum.SortOrder.LayoutOrder,
	}),
})

if #Config.products == 0 then
	text({ Text = "Donations open soon.", TextSize = 18, TextColor3 = DIM, Parent = buttons })
end
for order, id in Config.products do
	local button = make("TextButton", {
		BackgroundColor3 = if order == 1 then WHITE else PANEL,
		TextColor3 = if order == 1 then BLACK else WHITE,
		FontFace = Font.fromEnum(Enum.Font.BuilderSansBold),
		TextSize = 18,
		Text = "…",
		AutoButtonColor = true,
		Size = UDim2.fromOffset(110, 44),
		LayoutOrder = order,
		Parent = buttons,
	}, { corner(6), make("UIStroke", { Color = LINE, ApplyStrokeMode = Enum.ApplyStrokeMode.Border }) })
	task.spawn(function()
		local ok, info = pcall(MarketplaceService.GetProductInfoAsync, MarketplaceService, id, Enum.InfoType.Product)
		if ok and info and info.PriceInRobux then
			button.Text = "R$ " .. tostring(info.PriceInRobux)
		else
			button.Visible = false
		end
	end)
	button.Activated:Connect(function()
		MarketplaceService:PromptProductPurchase(player, id)
	end)
end

text({ Text = "Donors", TextSize = 20, FontFace = Font.fromEnum(Enum.Font.BuilderSansBold), LayoutOrder = 4, Parent = card })
local list = make("Frame", {
	BackgroundTransparency = 1,
	Size = UDim2.fromScale(1, 0),
	AutomaticSize = Enum.AutomaticSize.Y,
	LayoutOrder = 5,
	Parent = card,
}, { make("UIListLayout", { Padding = UDim.new(0, 6), SortOrder = Enum.SortOrder.LayoutOrder }) })

local function showBoard()
	for _, child in list:GetChildren() do
		if child:IsA("TextLabel") then
			child:Destroy()
		end
	end
	local ok, rows = pcall(HttpService.JSONDecode, HttpService, board.Value)
	if not ok or type(rows) ~= "table" or #rows == 0 then
		text({ Text = "Be the first.", TextSize = 16, TextColor3 = DIM, Parent = list })
		return
	end
	for rank, row in rows do
		text({
			Text = string.format("%d.  %s  ·  R$ %d", rank, tostring(row.name), tonumber(row.robux) or 0),
			TextSize = 16,
			TextColor3 = if rank == 1 then INDIGO else WHITE,
			LayoutOrder = rank,
			Parent = list,
		})
	end
end
board.Changed:Connect(showBoard)
showBoard()

local toast = make("TextLabel", {
	BackgroundColor3 = INDIGO,
	TextColor3 = BLACK,
	FontFace = Font.fromEnum(Enum.Font.BuilderSansBold),
	TextSize = 18,
	AnchorPoint = Vector2.new(0.5, 1),
	Position = UDim2.new(0.5, 0, 1, -24),
	Size = UDim2.fromOffset(0, 44),
	AutomaticSize = Enum.AutomaticSize.X,
	Visible = false,
	Parent = screen,
}, { corner(6), make("UIPadding", { PaddingLeft = UDim.new(0, 18), PaddingRight = UDim.new(0, 18) }) })

thanked.OnClientEvent:Connect(function(robux: number)
	toast.Text = string.format("Thank you for the R$ %d!", robux)
	toast.Visible = true
	task.delay(5, function()
		toast.Visible = false
	end)
end)

screen.Parent = player:WaitForChild("PlayerGui")
