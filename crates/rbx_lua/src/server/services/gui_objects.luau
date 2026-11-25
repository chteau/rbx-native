--!nonstrict
-- GuiObject methods in Local Run past what the engine answers itself:
-- UIPageLayout's page turning and ScrollingFrame's scroll velocity. Our
-- own, from the creator docs. A page layout's pages are its parent's
-- GuiObject children in its SortOrder; turning a page moves
-- CurrentPage, fires PageLeave on the old one and PageEnter on the new,
-- then Stopped once the turn's tween time is up (at once when not
-- Animated). Where the pages sit on screen is the layout pass's job.
local host = ...

local M = {}

local function fail(message)
	error(message, 0)
end

local function pages(layout)
	local parent = layout.Parent
	local list = {}
	if not parent then
		return list
	end
	for _, child in parent:GetChildren() do
		if child:IsA("GuiObject") then
			table.insert(list, child)
		end
	end
	local byName = layout.SortOrder == Enum.SortOrder.Name
	local order = {}
	for i, page in list do
		order[page] = i
	end
	table.sort(list, function(a, b)
		if byName then
			if a.Name ~= b.Name then
				return a.Name < b.Name
			end
		elseif a.LayoutOrder ~= b.LayoutOrder then
			return a.LayoutOrder < b.LayoutOrder
		end
		return order[a] < order[b]
	end)
	return list
end

-- The page each layout turned to last (CurrentPage reads through here).
local chosen = setmetatable({}, { __mode = "k" })

local function current(layout, list)
	local page = chosen[layout]
	if page and table.find(list, page) then
		return page
	end
	return list[1]
end

-- Turns of each layout, so a later one cancels an earlier one's Stopped.
local turns = setmetatable({}, { __mode = "k" })

local function turnTo(layout, page)
	local list = pages(layout)
	local old = current(layout, list)
	if page == nil or page == old then
		return
	end
	local id = host.id(layout)
	chosen[layout] = page
	host.set_engine(id, "CurrentPage", page)
	if old then
		host.fire(id, "PageLeave", old)
	end
	host.fire(id, "PageEnter", page)
	local turn = (turns[layout] or 0) + 1
	turns[layout] = turn
	local delay = if layout.Animated then layout.TweenTime else 0
	task.delay(delay, function()
		if turns[layout] == turn then
			host.fire(id, "Stopped", page)
		end
	end)
end

local function step(layout, by)
	local list = pages(layout)
	local at = table.find(list, current(layout, list))
	if not at then
		return
	end
	local to = at + by
	if layout.Circular then
		to = (to - 1) % #list + 1
	end
	turnTo(layout, list[to])
end

local PAGE_LAYOUT = {
	JumpTo = function(self, page)
		if typeof(page) ~= "Instance" then
			fail("Unable to cast value to Object")
		end
		if table.find(pages(self), page) then
			turnTo(self, page)
		end
	end,
	JumpToIndex = function(self, index)
		if type(index) ~= "number" then
			fail("Argument 1 missing or nil")
		end
		turnTo(self, pages(self)[math.floor(index) + 1])
	end,
	Next = function(self)
		step(self, 1)
	end,
	Previous = function(self)
		step(self, -1)
	end,
}

local PAGE_SIGNALS = { PageEnter = true, PageLeave = true, Stopped = true }

-- Each ScrollingFrame asked about: its canvas position a frame ago and how
-- fast it has moved since, in pixels a second.
local tracked = setmetatable({}, { __mode = "k" })
local tracking

local function track(frame)
	local entry = tracked[frame]
	if not entry then
		entry = { position = frame.CanvasPosition, velocity = Vector2.zero }
		tracked[frame] = entry
	end
	if not tracking then
		tracking = game:GetService("RunService").Heartbeat:Connect(function(dt)
			for f, e in tracked do
				local position = f.CanvasPosition
				e.velocity = (position - e.position) / math.max(dt, 1e-6)
				e.position = position
			end
		end)
	end
	return entry
end

local SCROLLING = {
	GetScrollVelocity = function(self)
		return track(self).velocity
	end,
	ResetScrollVelocity = function(self)
		track(self).velocity = Vector2.zero
	end,
}

function M.member(class, id, key)
	if class == "UIPageLayout" then
		if PAGE_SIGNALS[key] then
			return host.signal(id, key), true
		end
		if key == "CurrentPage" then
			local layout = host.instance(id)
			return current(layout, pages(layout)), true
		end
		return PAGE_LAYOUT[key], PAGE_LAYOUT[key] ~= nil
	end
	return SCROLLING[key], SCROLLING[key] ~= nil
end

return M
