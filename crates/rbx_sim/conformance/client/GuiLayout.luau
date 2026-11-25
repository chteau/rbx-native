-- GuiLayout on the client: how layouts, constraints and AutomaticSize
-- combine, read back through AbsolutePosition/AbsoluteSize. Every tree
-- hangs off a frame of fixed pixel size, so the window's size doesn't
-- matter; positions are reported from that frame's corner.
local function new(class, props, parent)
	local object = Instance.new(class)
	for key, value in props do
		object[key] = value
	end
	object.Parent = parent
	return object
end

local function frame(props, parent)
	props.BackgroundTransparency = props.BackgroundTransparency or 1
	props.BorderSizePixel = 0
	return new("Frame", props, parent)
end

-- A cell-sized button: square by a UIAspectRatioConstraint, capped at
-- 60x60 by a UISizeConstraint, as a HUD grid's buttons often are.
local function button(parent, order, aspect, cap, anchor)
	local b = new("TextButton", {
		Text = "",
		LayoutOrder = order,
		Size = UDim2.fromScale(0.388, 0.317),
		AnchorPoint = anchor or Vector2.new(0.5, 0.5),
		BorderSizePixel = 0,
	}, parent)
	if aspect then
		new("UIAspectRatioConstraint", { AspectRatio = 1 }, b)
	end
	if cap then
		new("UISizeConstraint", { MaxSize = Vector2.new(60, 60) }, b)
	end
	return b
end

local function grid(parent)
	return new("UIGridLayout", {
		CellSize = UDim2.fromScale(0.45, 0.3),
		CellPadding = UDim2.fromScale(0.05, 0),
		FillDirectionMaxCells = 2,
		HorizontalAlignment = Enum.HorizontalAlignment.Center,
		VerticalAlignment = Enum.VerticalAlignment.Center,
		SortOrder = Enum.SortOrder.LayoutOrder,
	}, parent)
end

return function(ch)
	local gui = game:GetService("Players").LocalPlayer.PlayerGui
	local screen = new("ScreenGui", { Name = "ConfLayout", IgnoreGuiInset = true, ResetOnSpawn = false }, gui)
	local roots = {}
	local function root(name)
		local r = frame({ Name = name, Size = UDim2.fromOffset(166, 276), Position = UDim2.fromOffset(10, 10) }, screen)
		roots[name] = r
		return r
	end

	-- A column of buttons: a frame growing with its grid (AutomaticSize Y),
	-- cell size in scale, and a button under it in the column's list.
	local function column(name, auto, aspect, cap, anchor)
		local col = root(name)
		new("UIListLayout", {
			SortOrder = Enum.SortOrder.LayoutOrder,
			HorizontalAlignment = Enum.HorizontalAlignment.Center,
			VerticalAlignment = Enum.VerticalAlignment.Center,
		}, col)
		local holder = frame({
			Name = "Holder",
			LayoutOrder = 0,
			Size = UDim2.fromScale(1, 0.2756),
			AutomaticSize = if auto then Enum.AutomaticSize.Y else Enum.AutomaticSize.None,
		}, col)
		grid(holder)
		local buttons = {}
		for i = 1, 6 do
			buttons[i] = button(holder, i, aspect, cap, anchor)
		end
		local below = frame({ Name = "Below", LayoutOrder = 1, Size = UDim2.fromScale(1, 0.2) }, col)
		return { holder = holder, first = buttons[1], second = buttons[2], third = buttons[3], below = below }
	end

	local cases = {
		auto_capped = column("AutoCapped", true, true, true),
		auto_square = column("AutoSquare", true, true, false),
		auto_bare = column("AutoBare", true, false, false),
		fixed_capped = column("FixedCapped", false, true, true),
		corner_capped = column("CornerCapped", true, true, true, Vector2.new(0, 0)),
		far_capped = column("FarCapped", true, true, true, Vector2.new(1, 1)),
	}

	-- A list under a frame growing with AutomaticSize Y, its items sized in
	-- scale along that axis.
	local stack = root("AutoList")
	local grown = frame({ Name = "Grown", Size = UDim2.fromScale(1, 0.1), AutomaticSize = Enum.AutomaticSize.Y }, stack)
	new("UIListLayout", { SortOrder = Enum.SortOrder.LayoutOrder }, grown)
	local top = frame({ Size = UDim2.fromScale(1, 0.25), LayoutOrder = 0 }, grown)
	local fixed = frame({ Size = UDim2.fromOffset(50, 30), LayoutOrder = 1 }, grown)
	-- A capped list item: where it sits across the list.
	local lane = root("ListCapped")
	new("UIListLayout", { SortOrder = Enum.SortOrder.LayoutOrder }, lane)
	local capped = frame({ Size = UDim2.fromScale(1, 0.2), AnchorPoint = Vector2.new(0.5, 0.5) }, lane)
	new("UISizeConstraint", { MaxSize = Vector2.new(60, 60) }, capped)
	local next_ = frame({ Size = UDim2.fromOffset(20, 20), LayoutOrder = 1 }, lane)

	-- AutomaticSize Y with a scale-sized child and an offset-sized one.
	local auto = root("AutoScale")
	local grow = frame({ Name = "Grow", Size = UDim2.new(0, 200, 0, 50), AutomaticSize = Enum.AutomaticSize.Y }, auto)
	local half = frame({ Size = UDim2.fromScale(1, 0.5) }, grow)
	local tall = frame({ Size = UDim2.fromOffset(100, 120), Position = UDim2.fromOffset(0, 10) }, grow)

	-- A list item square by its own UIAspectRatioConstraint.
	local list = root("ListAspect")
	new("UIListLayout", { SortOrder = Enum.SortOrder.LayoutOrder }, list)
	local item = frame({ Size = UDim2.fromScale(1, 0.5) }, list)
	new("UIAspectRatioConstraint", { AspectRatio = 1 }, item)
	local after = frame({ Size = UDim2.fromOffset(20, 20), LayoutOrder = 1 }, list)

	ch.beats(5)
	local function rect(object, base)
		local p = object.AbsolutePosition - base.AbsolutePosition
		local s = object.AbsoluteSize
		return p.X, p.Y, s.X, s.Y
	end
	local r = {}
	local function put(key, object, base)
		local x, y, w, h = rect(object, base)
		r[key .. "_x"], r[key .. "_y"], r[key .. "_w"], r[key .. "_h"] = x, y, w, h
	end
	for name, case in cases do
		local base = case.holder.Parent
		put(name .. "_holder", case.holder, base)
		put(name .. "_first", case.first, base)
		put(name .. "_second", case.second, base)
		put(name .. "_third", case.third, base)
		put(name .. "_below", case.below, base)
	end
	put("auto_scale_grow", grow, auto)
	put("auto_scale_half", half, auto)
	put("auto_scale_tall", tall, auto)
	put("list_aspect_item", item, list)
	put("list_aspect_after", after, list)
	put("auto_list_grown", grown, stack)
	put("auto_list_top", top, stack)
	put("auto_list_fixed", fixed, stack)
	put("list_capped_item", capped, lane)
	put("list_capped_next", next_, lane)
	screen:Destroy()
	return r
end
