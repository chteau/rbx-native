-- Purchase dialog that mimics Roblox's own prompt. Layout and motion numbers
-- were measured from Roblox's Foundation Dialog source, live CoreGui dumps and
-- a reference screenshot; see the notes beside each table.
local RunService = game:GetService("RunService")
local Workspace = game:GetService("Workspace")

local MarketUI = {}

local SANS = "rbxasset://fonts/families/BuilderSans.json"
local ICONS = "rbxasset://LuaPackages/Packages/_Index/BuilderIcons/BuilderIcons/BuilderIcons.json"

local COLOR = {
	scrim = "0A0A0E",
	dialog = "191A1F",
	stroke = "D0D9FB",
	title = "D5D7DD",
	text = "F7F7F8",
	body = "D5D7DD",
	closeHover = "292A32",
	buy = "335FFF",
	buyHover = "4069FF",
	banner = "D0D9FB",
	pill = "F7F7F8",
	pillText = "202227",
	shadow = "000000",
}

local FONT = {
	sans = SANS,
	icons = ICONS,
	iconWeight = 400,
	titleSize = 25,
	-- text-heading-small: its LineHeight decides where the glyphs sit in the header.
	titleLineHeight = 0.96,
	-- The balance is lighter than the item price beside the same icon.
	balanceWeight = "Medium",
	itemSize = 18,
	itemLineHeight = 1.111,
	buySize = 18,
	buyLineHeight = 0.778,
	closeIconSize = 24,
	iconLineHeight = 1.1,
	bannerSize = 15,
	bannerLineHeight = 1.2,
	pillSize = 15,
	disclaimerSize = 13,
	disclaimerLineHeight = 1.077,
	completedIconSize = 48,
	bannerIconSize = 20,
}

local SIZE = {
	dialogMaxWidth = 456,
	viewportMargin = 32,
	holderPadding = 16,
	dialogRadius = 16,
	dialogBottomPadding = 20,
	strokeThickness = 1,
	bannerStrokeThickness = 1.5,
	headerHeight = 48,
	-- The reference title sits one pixel lower than plain centring puts it.
	titleNudge = 2,
	headerTop = 8,
	headerLeft = 20,
	headerRight = 12,
	headerGroupHeight = 40,
	headerGroupGap = 8,
	closeButton = 40,
	closeRadius = 8,
	closeIcon = 24,
	itemRowHeight = 81,
	itemTop = 9,
	sideInset = 20,
	itemIcon = 72,
	itemDetailsOffset = 80,
	itemDetailsGap = 8,
	buySectionTop = 40,
	buyHeight = 40,
	buyRadius = 8,
	bannerTop = 12,
	bannerHeight = 44,
	bannerRadius = 8,
	bannerIcon = 20,
	bannerIconInset = 12,
	bannerGap = 8,
	-- text-body-small's own vertical padding.
	bannerTextPad = 1,
	-- Share of the row the banner text may take before wrapping (live dump: 0.7).
	bannerTextWidth = 0.7,
	-- The reference's banner text sits one pixel below the centred row.
	bannerTextNudge = 1,
	pillWidth = 42,
	pillHeight = 24,
	pillRadius = 12,
	pillInset = 12,
	-- "New" sits one pixel below the pill's centre in the reference.
	pillTextNudge = 1,
	disclaimerTop = 12,
	subscriptionDisclaimerTop = 20,
	termsTop = 20,
	completedTop = 28,
	completedGap = 16,
	completedIcon = 48,
	okTop = 20,
	shadowSlice = 16,
	shadowSliceScale = 2,
	shadowRectOffsetX = 305,
	shadowRectOffsetY = 464,
	shadowRectSize = 33,
	displayOrder = 1000000,
}

local IMAGE = {
	shadow = "rbxasset://LuaPackages/Packages/_Index/FoundationImages/FoundationImages/SpriteSheets/img_set_1x_3.png",
}

local TRANSPARENCY = {
	scrimOpen = 0.25,
	stroke = 0.88,
	shadow = 0.85,
	closeButton = 0.08,
	closeButtonHover = 0.0736,
	bannerHover = 0.92,
	buyPressed = 0.5,
	buyGradientRight = 0.5,
}

local MOTION = {
	slideDistance = 32,
	openDuration = 0.3,
	openBezier = { 0.2, 0, 0, 1 },
	closeDuration = 0.2,
	closeBezier = { 0.8, 0, 0.4, 1 },
	scrimDuration = 0.1,
	buyDelay = 0.65,
	buyPressDuration = 0.1,
	gradientEdge = 1e-4,
	newtonIterations = 8,
	bisectIterations = 40,
	solveEpsilon = 1e-6,
}

local TEXT = {
	buyTitle = "Buy item",
	completedTitle = "Purchase completed",
	buy = "Buy",
	ok = "OK",
	banner = "Get 10% off with Roblox Plus",
	pill = "New",
	disclaimer = "This is a test purchase. Your account will not be charged.",
	noPrice = "—",
	subscriptionTitle = "Get subscription",
	subscribe = "Subscribe",
	terms = 'By clicking "Subscribe", you agree to the Terms of Use. You will be automatically charged each month and can cancel at any time.',
	errorTitle = "Error",
	errorMessage = "Your purchase failed because something went wrong. Your account has not been charged. Please try again later.",
	completedMessage = "You have successfully bought %s.",
}

local current = nil

function MarketUI.isOpen()
	return current ~= nil
end

-- CSS cubic-bezier: x(t) is monotonic for the control points used, so Newton
-- converges fast and bisection is a safe fallback for flat spots.
function MarketUI.bezier(x1, y1, x2, y2)
	local function coord(a, b, t)
		local u = 1 - t
		return 3 * u * u * t * a + 3 * u * t * t * b + t * t * t
	end
	local function slope(a, b, t)
		local u = 1 - t
		return 3 * u * u * a + 6 * u * t * (b - a) + 3 * t * t * (1 - b)
	end
	return function(x)
		if x <= 0 then
			return 0
		elseif x >= 1 then
			return 1
		end
		local t = x
		local solved = false
		for _ = 1, MOTION.newtonIterations do
			local err = coord(x1, x2, t) - x
			if math.abs(err) < MOTION.solveEpsilon then
				solved = true
				break
			end
			local d = slope(x1, x2, t)
			if math.abs(d) < MOTION.solveEpsilon then
				break
			end
			t = math.clamp(t - err / d, 0, 1)
		end
		if not solved then
			local lo, hi = 0, 1
			t = x
			for _ = 1, MOTION.bisectIterations do
				if coord(x1, x2, t) < x then
					lo = t
				else
					hi = t
				end
				t = (lo + hi) / 2
			end
		end
		return coord(y1, y2, t)
	end
end

local function linear(a)
	return a
end

local function formatRobux(n)
	local s = tostring(math.floor(n))
	local out = s:reverse():gsub("(%d%d%d)", "%1,"):reverse()
	return (out:gsub("^,", ""))
end

local function hex(code)
	return Color3.fromHex(code)
end

-- Creates an instance, applies props, then children, and parents last so the
-- instance is configured before it replicates into the tree.
local function make(className, props, children)
	local inst = Instance.new(className)
	local parent = nil
	for k, v in props or {} do
		if k == "Parent" then
			parent = v
		else
			inst[k] = v
		end
	end
	for _, child in children or {} do
		child.Parent = inst
	end
	inst.Parent = parent
	return inst
end

local function px(n)
	return UDim.new(0, n)
end

local function pad(top, left, right, bottom)
	return make("UIPadding", {
		PaddingTop = px(top or 0),
		PaddingLeft = px(left or 0),
		PaddingRight = px(right or 0),
		PaddingBottom = px(bottom or 0),
	})
end

local function corner(radius)
	return make("UICorner", { CornerRadius = px(radius) })
end

local function stroke(thickness)
	return make("UIStroke", {
		Color = hex(COLOR.stroke),
		Transparency = TRANSPARENCY.stroke,
		Thickness = thickness,
		ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
	})
end

local function list(props)
	props = props or {}
	props.SortOrder = Enum.SortOrder.LayoutOrder
	return make("UIListLayout", props)
end

local function sansFont(weight)
	return Font.new(FONT.sans, Enum.FontWeight[weight])
end

local function label(props, children)
	local p = {
		BackgroundTransparency = 1,
		BorderSizePixel = 0,
		TextXAlignment = Enum.TextXAlignment.Left,
		TextYAlignment = Enum.TextYAlignment.Center,
	}
	for k, v in props do
		p[k] = v
	end
	return make("TextLabel", p, children)
end

-- Icons are ligatures in a font: the label text is the icon's name.
local function iconLabel(name, size, props)
	local p = {
		Text = name,
		FontFace = Font.new(FONT.icons),
		TextSize = size,
		TextColor3 = hex(COLOR.text),
		LineHeight = FONT.iconLineHeight,
		TextXAlignment = Enum.TextXAlignment.Center,
	}
	for k, v in props or {} do
		p[k] = v
	end
	return label(p)
end

local function robuxText(amount)
	local value = if amount then formatRobux(amount) else TEXT.noPrice
	return '<font family="' .. FONT.icons .. '" weight="' .. FONT.iconWeight .. '">robux</font> ' .. value
end

local function itemLabel(props)
	local p = {
		FontFace = sansFont("Bold"),
		TextSize = FONT.itemSize,
		LineHeight = FONT.itemLineHeight,
		TextColor3 = hex(COLOR.text),
	}
	for k, v in props do
		p[k] = v
	end
	return label(p)
end

-- One of the dialog's stacked sections: a padded, auto-sized frame.
local function section(parent, order, top)
	return make("Frame", {
		Parent = parent,
		BackgroundTransparency = 1,
		LayoutOrder = order,
		Size = UDim2.new(1, 0, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
	}, { pad(top, SIZE.sideInset, SIZE.sideInset, 0) })
end

-- Runs step(ease(alpha)) every RenderStepped for `duration` seconds; the final
-- call always receives exactly 1. Returns a function that stops it early.
local function animate(duration, ease, step, onDone)
	ease = ease or linear
	local start = os.clock()
	local conn = nil
	local function finish()
		if conn then
			conn:Disconnect()
			conn = nil
		end
	end
	step(ease(0))
	conn = RunService.RenderStepped:Connect(function()
		local alpha = math.min(1, (os.clock() - start) / duration)
		step(ease(alpha))
		if alpha >= 1 then
			finish()
			if onDone then
				onDone()
			end
		end
	end)
	return finish
end

local function header(parent, titleText, balance, onClose)
	local bar = make("Frame", {
		Parent = parent,
		BackgroundTransparency = 1,
		LayoutOrder = 1,
		Size = UDim2.new(1, 0, 0, SIZE.headerHeight),
	}, { pad(SIZE.headerTop, SIZE.headerLeft, SIZE.headerRight, 0) })
	label({
		Parent = bar,
		Text = titleText,
		FontFace = sansFont("Bold"),
		TextSize = FONT.titleSize,
		LineHeight = FONT.titleLineHeight,
		TextColor3 = hex(COLOR.title),
		AnchorPoint = Vector2.new(0, 0.5),
		Position = UDim2.new(0, 0, 0.5, 0),
		AutomaticSize = Enum.AutomaticSize.XY,
	}, { pad(SIZE.titleNudge, 0, 0, 0) })
	local group = make("Frame", {
		Parent = bar,
		BackgroundTransparency = 1,
		AnchorPoint = Vector2.new(1, 0.5),
		Position = UDim2.new(1, 0, 0.5, 0),
		Size = UDim2.new(0, 0, 0, SIZE.headerGroupHeight),
		AutomaticSize = Enum.AutomaticSize.X,
	}, {
		list({
			FillDirection = Enum.FillDirection.Horizontal,
			Padding = px(SIZE.headerGroupGap),
			VerticalAlignment = Enum.VerticalAlignment.Center,
			HorizontalAlignment = Enum.HorizontalAlignment.Right,
		}),
	})
	if balance then
		itemLabel({
			Parent = group,
			LayoutOrder = 1,
			FontFace = sansFont(FONT.balanceWeight),
			RichText = true,
			Text = robuxText(balance),
			AutomaticSize = Enum.AutomaticSize.XY,
		})
	end
	local close = make("ImageButton", {
		Parent = group,
		LayoutOrder = 2,
		AutoButtonColor = false,
		Image = "",
		BorderSizePixel = 0,
		Size = UDim2.new(0, SIZE.closeButton, 0, SIZE.closeButton),
		BackgroundColor3 = hex(COLOR.dialog),
		BackgroundTransparency = TRANSPARENCY.closeButton,
	}, {
		corner(SIZE.closeRadius),
		iconLabel("x", FONT.closeIconSize, {
			AnchorPoint = Vector2.new(0.5, 0.5),
			Position = UDim2.new(0.5, 0, 0.5, 0),
			Size = UDim2.new(0, SIZE.closeIcon, 0, SIZE.closeIcon),
		}),
	})
	close.MouseEnter:Connect(function()
		close.BackgroundColor3 = hex(COLOR.closeHover)
		close.BackgroundTransparency = TRANSPARENCY.closeButtonHover
	end)
	close.MouseLeave:Connect(function()
		close.BackgroundColor3 = hex(COLOR.dialog)
		close.BackgroundTransparency = TRANSPARENCY.closeButton
	end)
	close.Activated:Connect(onClose)
end

-- Primary button shared by Buy and OK; returns the button and whether hover is live.
local function primaryButton(parent, text, state)
	local button = make("ImageButton", {
		Parent = parent,
		AutoButtonColor = false,
		Image = "",
		BorderSizePixel = 0,
		Size = UDim2.new(1, 0, 0, SIZE.buyHeight),
		BackgroundColor3 = hex(COLOR.buy),
	}, {
		corner(SIZE.buyRadius),
		label({
			Text = text,
			FontFace = sansFont("Medium"),
			TextSize = FONT.buySize,
			LineHeight = FONT.buyLineHeight,
			TextColor3 = hex(COLOR.text),
			TextXAlignment = Enum.TextXAlignment.Center,
			Size = UDim2.new(1, 0, 1, 0),
		}),
	})
	button.MouseEnter:Connect(function()
		if state.enabled then
			button.BackgroundColor3 = hex(COLOR.buyHover)
		end
	end)
	button.MouseLeave:Connect(function()
		button.BackgroundColor3 = hex(COLOR.buy)
	end)
	return button
end

-- Wipes left to right as the delay elapses, as the real prompt's Buy does.
local function runBuyDelay(button, state)
	state.enabled = false
	local edge = MOTION.gradientEdge
	local gradient = make("UIGradient", { Parent = button })
	local function setProgress(p)
		p = math.clamp(p, 2 * edge, 1 - 2 * edge)
		gradient.Transparency = NumberSequence.new({
			NumberSequenceKeypoint.new(0, 0),
			NumberSequenceKeypoint.new(p - edge, 0),
			NumberSequenceKeypoint.new(p + edge, TRANSPARENCY.buyGradientRight),
			NumberSequenceKeypoint.new(1, TRANSPARENCY.buyGradientRight),
		})
	end
	state.stopDelay = animate(MOTION.buyDelay, function(a)
		return a * a
	end, setProgress, function()
		gradient:Destroy()
		state.enabled = true
	end)
end

local function noteSection(body, order, top, text, align)
	local note = section(body, order, top)
	label({
		Parent = note,
		Text = text,
		FontFace = sansFont("Medium"),
		TextSize = FONT.disclaimerSize,
		LineHeight = FONT.disclaimerLineHeight,
		TextColor3 = hex(COLOR.body),
		TextXAlignment = align,
		TextWrapped = true,
		Size = UDim2.new(1, 0, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
	})
end

local function priceLine(info)
	if info.priceText then
		return info.priceText, false
	end
	return robuxText(info.price) .. (info.period or ""), true
end

local function itemRow(body, info)
	local priceText, rich = priceLine(info)
	local item = make("Frame", {
		Parent = body,
		BackgroundTransparency = 1,
		LayoutOrder = 2,
		Size = UDim2.new(1, 0, 0, SIZE.itemRowHeight),
	}, { pad(SIZE.itemTop, SIZE.sideInset, SIZE.sideInset, 0) })
	make("ImageLabel", {
		Parent = item,
		BackgroundTransparency = 1,
		BorderSizePixel = 0,
		Size = UDim2.new(0, SIZE.itemIcon, 0, SIZE.itemIcon),
		ScaleType = Enum.ScaleType.Fit,
		Image = info.icon or "",
	})
	local details = make("Frame", {
		Parent = item,
		BackgroundTransparency = 1,
		AnchorPoint = Vector2.new(0, 0.5),
		Position = UDim2.new(0, SIZE.itemDetailsOffset, 0.5, 0),
		Size = UDim2.new(1, -SIZE.itemDetailsOffset, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
	}, { list({ Padding = px(SIZE.itemDetailsGap) }) })
	itemLabel({
		Parent = details,
		LayoutOrder = 1,
		Text = info.name,
		TextWrapped = true,
		Size = UDim2.new(1, 0, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
	})
	itemLabel({
		Parent = details,
		LayoutOrder = 2,
		RichText = rich,
		Text = priceText,
		AutomaticSize = Enum.AutomaticSize.XY,
	})
end

local function buyButton(body, text, state, actions)
	local buySection = section(body, 3, SIZE.buySectionTop)
	local buy = primaryButton(buySection, text, state)
	buy.Activated:Connect(function()
		if state.enabled then
			actions.buy(buy)
		end
	end)
	runBuyDelay(buy, state)
	return buy
end

local function buildBuyView(body, info, state, actions)
	header(body, TEXT.buyTitle, info.balance, actions.cancel)
	itemRow(body, info)
	local buy = buyButton(body, TEXT.buy, state, actions)

	local bannerSection = section(body, 4, SIZE.bannerTop)
	local banner = make("ImageButton", {
		Parent = bannerSection,
		AutoButtonColor = false,
		Image = "",
		Size = UDim2.new(1, 0, 0, SIZE.bannerHeight),
		BackgroundColor3 = hex(COLOR.banner),
		BackgroundTransparency = 1,
		BorderSizePixel = 0,
	}, {
		corner(SIZE.bannerRadius),
		stroke(SIZE.bannerStrokeThickness),
		-- Icon and text centred in a list row, as Roblox lays them out: the
		-- text's half-pixel offset then rounds the same way.
		make("Frame", {
			BackgroundTransparency = 1,
			Position = UDim2.new(0, SIZE.bannerIconInset, 0, SIZE.bannerIconInset),
			Size = UDim2.new(1, -2 * SIZE.bannerIconInset, 0, SIZE.bannerIcon),
		}, {
			list({
				FillDirection = Enum.FillDirection.Horizontal,
				Padding = px(SIZE.bannerGap),
				VerticalAlignment = Enum.VerticalAlignment.Center,
			}),
			iconLabel("roblox-plus", FONT.bannerIconSize, {
				LayoutOrder = 1,
				Size = UDim2.new(0, SIZE.bannerIcon, 0, SIZE.bannerIcon),
			}),
			label({
				LayoutOrder = 2,
				Text = TEXT.banner,
				FontFace = sansFont("Regular"),
				TextSize = FONT.bannerSize,
				LineHeight = FONT.bannerLineHeight,
				TextColor3 = hex(COLOR.text),
				TextWrapped = true,
				Size = UDim2.new(SIZE.bannerTextWidth, 0, 0, 0),
				AutomaticSize = Enum.AutomaticSize.Y,
			}, {
				pad(SIZE.bannerTextPad + SIZE.bannerTextNudge, 0, 0, SIZE.bannerTextPad - SIZE.bannerTextNudge),
			}),
		}),
		make("Frame", {
			AnchorPoint = Vector2.new(1, 0.5),
			Position = UDim2.new(1, -SIZE.pillInset, 0.5, 0),
			Size = UDim2.new(0, SIZE.pillWidth, 0, SIZE.pillHeight),
			BackgroundColor3 = hex(COLOR.pill),
			BorderSizePixel = 0,
		}, {
			corner(SIZE.pillRadius),
			label({
				Text = TEXT.pill,
				FontFace = sansFont("Medium"),
				TextSize = FONT.pillSize,
				TextColor3 = hex(COLOR.pillText),
				TextXAlignment = Enum.TextXAlignment.Center,
				Size = UDim2.new(1, 0, 1, 0),
			}, { pad(SIZE.pillTextNudge, 0, 0, -SIZE.pillTextNudge) }),
		}),
	})
	banner.MouseEnter:Connect(function()
		banner.BackgroundTransparency = TRANSPARENCY.bannerHover
	end)
	banner.MouseLeave:Connect(function()
		banner.BackgroundTransparency = 1
	end)

	noteSection(body, 5, SIZE.disclaimerTop, TEXT.disclaimer, Enum.TextXAlignment.Center)
	return buy
end

local function buildSubscriptionView(body, info, state, actions)
	header(body, TEXT.subscriptionTitle, nil, actions.cancel)
	itemRow(body, info)
	local buy = buyButton(body, TEXT.subscribe, state, actions)
	noteSection(body, 4, SIZE.subscriptionDisclaimerTop, TEXT.disclaimer, Enum.TextXAlignment.Center)
	noteSection(body, 5, SIZE.termsTop, TEXT.terms, Enum.TextXAlignment.Left)
	return buy
end

local function buildMessageView(body, title, icon, message, state, onClose)
	header(body, title, nil, onClose)
	local content = section(body, 2, SIZE.completedTop)
	content.AutomaticSize = Enum.AutomaticSize.Y
	make("UIListLayout", {
		Parent = content,
		SortOrder = Enum.SortOrder.LayoutOrder,
		Padding = px(SIZE.completedGap),
		HorizontalAlignment = Enum.HorizontalAlignment.Center,
	})
	iconLabel(icon, FONT.completedIconSize, {
		Parent = content,
		LayoutOrder = 1,
		TextColor3 = hex(COLOR.body),
		Size = UDim2.new(0, SIZE.completedIcon, 0, SIZE.completedIcon),
	})
	label({
		Parent = content,
		LayoutOrder = 2,
		Text = message,
		FontFace = sansFont("Regular"),
		TextSize = FONT.itemSize,
		LineHeight = FONT.itemLineHeight,
		TextColor3 = hex(COLOR.body),
		TextXAlignment = Enum.TextXAlignment.Center,
		TextWrapped = true,
		Size = UDim2.new(1, 0, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
	})
	local okSection = section(body, 3, SIZE.okTop)
	state.enabled = true
	primaryButton(okSection, TEXT.ok, state).Activated:Connect(onClose)
end

local function buildCompletedView(body, info, state, actions)
	buildMessageView(
		body,
		TEXT.completedTitle,
		"circle-check",
		string.format(TEXT.completedMessage, info.name),
		state,
		actions.finish
	)
end

local function buildErrorView(body, state, actions)
	buildMessageView(body, TEXT.errorTitle, "triangle-exclamation", TEXT.errorMessage, state, actions.cancel)
end

-- Shadow and padded content live under whichever of the two containers is
-- active: a CanvasGroup while fading, a plain Frame at rest (a CanvasGroup
-- re-rasterises text, which the real prompt avoids the same way).
local function buildShell(parent, width)
	local gui = make("ScreenGui", {
		Name = "__rbxnative_purchase",
		Parent = parent,
		ResetOnSpawn = false,
		IgnoreGuiInset = true,
		ScreenInsets = Enum.ScreenInsets.None,
		DisplayOrder = SIZE.displayOrder,
		ZIndexBehavior = Enum.ZIndexBehavior.Sibling,
	})
	local scrim = make("TextButton", {
		Parent = gui,
		Text = "",
		AutoButtonColor = false,
		BorderSizePixel = 0,
		Size = UDim2.new(1, 0, 1, 0),
		BackgroundColor3 = hex(COLOR.scrim),
		BackgroundTransparency = 1,
	})
	local holder = make("Frame", {
		Parent = gui,
		BackgroundTransparency = 1,
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.new(0.5, 0, 0.5, 0),
		Size = UDim2.new(0, width + 2 * SIZE.holderPadding, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
	})
	local containerProps = {
		Parent = holder,
		BackgroundTransparency = 1,
		BorderSizePixel = 0,
		Size = UDim2.new(1, 0, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
	}
	local rest = make("Frame", containerProps)
	containerProps.Name = "Fade"
	local fade = make("CanvasGroup", containerProps)
	local shadow = make("ImageLabel", {
		Parent = rest,
		BackgroundTransparency = 1,
		BorderSizePixel = 0,
		ZIndex = 0,
		Size = UDim2.new(1, 0, 1, 0),
		Image = IMAGE.shadow,
		ImageRectOffset = Vector2.new(SIZE.shadowRectOffsetX, SIZE.shadowRectOffsetY),
		ImageRectSize = Vector2.new(SIZE.shadowRectSize, SIZE.shadowRectSize),
		ScaleType = Enum.ScaleType.Slice,
		SliceCenter = Rect.new(SIZE.shadowSlice, SIZE.shadowSlice, SIZE.shadowSlice, SIZE.shadowSlice),
		SliceScale = SIZE.shadowSliceScale,
		ImageColor3 = hex(COLOR.shadow),
		ImageTransparency = TRANSPARENCY.shadow,
	})
	local inset = make("Frame", {
		Parent = rest,
		BackgroundTransparency = 1,
		Size = UDim2.new(1, 0, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
	}, { pad(SIZE.holderPadding, SIZE.holderPadding, SIZE.holderPadding, SIZE.holderPadding) })
	local dialog = make("ImageButton", {
		Parent = inset,
		AutoButtonColor = false,
		Image = "",
		BorderSizePixel = 0,
		Size = UDim2.new(0, width, 0, 0),
		AutomaticSize = Enum.AutomaticSize.Y,
		BackgroundColor3 = hex(COLOR.dialog),
		ClipsDescendants = true,
	}, {
		corner(SIZE.dialogRadius),
		stroke(SIZE.strokeThickness),
		pad(0, 0, 0, SIZE.dialogBottomPadding),
		list(),
	})
	return {
		gui = gui,
		scrim = scrim,
		holder = holder,
		rest = rest,
		fade = fade,
		shadow = shadow,
		inset = inset,
		dialog = dialog,
	}
end

local function setFading(shell, fading)
	local target = if fading then shell.fade else shell.rest
	shell.shadow.Parent = target
	shell.inset.Parent = target
end

-- Slides the holder and fades the content; direction "in" opens, "out" closes.
local function slide(shell, opening, onDone)
	local d = MOTION.slideDistance
	local curve = if opening then MOTION.openBezier else MOTION.closeBezier
	local duration = if opening then MOTION.openDuration else MOTION.closeDuration
	setFading(shell, true)
	animate(duration, MarketUI.bezier(table.unpack(curve)), function(a)
		local offsetY = if opening then d * (1 - a) else d * a
		shell.holder.Position = UDim2.new(0.5, 0, 0.5, offsetY)
		shell.fade.GroupTransparency = offsetY / d
	end, function()
		if opening then
			setFading(shell, false)
		end
		if onDone then
			onDone()
		end
	end)
end

local function fadeScrim(shell, toTransparency)
	local from = shell.scrim.BackgroundTransparency
	animate(MOTION.scrimDuration, linear, function(a)
		shell.scrim.BackgroundTransparency = from + (toTransparency - from) * a
	end)
end

function MarketUI.prompt(parent, info, onBuy, onClosed)
	if current then
		return
	end
	local camera = Workspace.CurrentCamera
	local viewportX = if camera then camera.ViewportSize.X else SIZE.dialogMaxWidth + SIZE.viewportMargin
	local width = math.min(SIZE.dialogMaxWidth, viewportX - SIZE.viewportMargin)
	local shell = buildShell(parent, width)
	local state = { mode = "idle", enabled = false, stopDelay = nil }
	current = state

	local body = nil
	local function replaceBody()
		if body then
			body:Destroy()
		end
		body = make("Frame", {
			Parent = shell.dialog,
			BackgroundTransparency = 1,
			Size = UDim2.new(1, 0, 0, 0),
			AutomaticSize = Enum.AutomaticSize.Y,
		}, { list() })
		return body
	end

	local purchased = false
	local function close(didPurchase)
		if state.mode == "closing" then
			return
		end
		state.mode = "closing"
		purchased = didPurchase
		if state.stopDelay then
			state.stopDelay()
		end
		fadeScrim(shell, 1)
		slide(shell, false, function()
			shell.gui:Destroy()
			current = nil
			onClosed(purchased)
		end)
	end

	local actions = {}
	function actions.cancel()
		if state.mode == "idle" then
			close(false)
		end
	end
	function actions.finish()
		if state.mode == "done" then
			close(true)
		end
	end
	function actions.buy(button)
		state.mode = "busy"
		state.enabled = false
		local from = button.BackgroundTransparency
		animate(MOTION.buyPressDuration, linear, function(a)
			button.BackgroundTransparency = from + (TRANSPARENCY.buyPressed - from) * a
		end)
		task.spawn(function()
			-- A failed purchase (the server call erroring) closes like a cancel
			-- instead of leaving the dialog stuck on a dimmed button.
			local ok, result = pcall(onBuy)
			local bought = ok and result == true
			if not ok then
				warn("[rbx-native] Test purchase failed: " .. tostring(result))
			end
			if state.mode == "closing" then
				return
			end
			if bought then
				state.mode = "done"
				buildCompletedView(replaceBody(), info, state, actions)
				slide(shell, true)
			else
				state.mode = "idle"
				close(false)
			end
		end)
	end

	shell.scrim.Activated:Connect(function()
		actions.cancel()
		actions.finish()
	end)

	if info.failed then
		buildErrorView(replaceBody(), state, actions)
	elseif info.kind == "Subscription" then
		buildSubscriptionView(replaceBody(), info, state, actions)
	else
		buildBuyView(replaceBody(), info, state, actions)
	end
	fadeScrim(shell, TRANSPARENCY.scrimOpen)
	slide(shell, true)
end

return MarketUI
