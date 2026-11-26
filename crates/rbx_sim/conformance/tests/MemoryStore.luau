-- MemoryStoreService on a server: sorted maps (Set/Get/Update/Remove,
-- ranges by key and by sort key, sizes), hash maps (and their pages),
-- queues (priorities, ReadAsync's invisibility, RemoveAsync, a read that
-- waits), item expiry, and the argument errors and limits. Names carry a
-- per-pass suffix; everything expires within a minute.
return function(h)
	local MemoryStoreService = game:GetService("MemoryStoreService")
	local HttpService = game:GetService("HttpService")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""):gsub(" RequestId: .*$", ""))
	end
	local function show(v)
		if type(v) ~= "table" then
			if type(v) == "number" then
				return string.format("%.17g", v)
			end
			return type(v) .. ":" .. tostring(v)
		end
		local keys = {}
		for k in v do
			table.insert(keys, k)
		end
		table.sort(keys, function(a, b)
			return tostring(a) < tostring(b)
		end)
		local parts = {}
		for _, k in keys do
			table.insert(parts, tostring(k) .. "=" .. show(v[k]))
		end
		return "{" .. table.concat(parts, ",") .. "}"
	end
	local suffix = HttpService:GenerateGUID(false):sub(1, 8)
	local map = MemoryStoreService:GetSortedMap("localrun_m12_map_" .. suffix)
	result.map_class = map.ClassName
	result.map_set_new = show(map:SetAsync("b", { x = 1 }, 60))
	result.map_set_again = show(map:SetAsync("b", { x = 2 }, 60))
	map:SetAsync("a", "first", 60)
	map:SetAsync("c", 3, 60, 10)
	map:SetAsync("d", 4, 60, 5)
	map:SetAsync("e", 5, 60, "zeta")
	local value, sortKey = map:GetAsync("c")
	result.map_get = show(value) .. " " .. show(sortKey)
	result.map_get_missing = show((map:GetAsync("nope")))
	local function range(items)
		local out = {}
		for _, item in items do
			table.insert(out, item.key .. ":" .. show(item.value) .. ":" .. show(item.sortKey))
		end
		return table.concat(out, " ")
	end
	result.map_range_asc = range(map:GetRangeAsync(Enum.SortDirection.Ascending, 10))
	result.map_range_desc2 = range(map:GetRangeAsync(Enum.SortDirection.Descending, 2))
	result.map_range_bounds = range(map:GetRangeAsync(Enum.SortDirection.Ascending, 10, { key = "a" }, { key = "e" }))
	result.map_range_sortkey = range(
		map:GetRangeAsync(Enum.SortDirection.Ascending, 10, { sortKey = 5 }, { sortKey = "zeta" })
	)
	result.map_size = show(map:GetSizeAsync())
	local seen = {}
	local updated = map:UpdateAsync("c", function(old, oldSort)
		table.insert(seen, show(old) .. " " .. show(oldSort))
		return old + 1, 20
	end, 60)
	result.map_update = table.concat(seen, ",") .. " -> " .. show(updated)
	result.map_update_cancel = show(map:UpdateAsync("c", function()
		return nil
	end, 60))
	result.map_update_missing = show(map:UpdateAsync("new", function(old)
		return old == nil and "made" or "had"
	end, 60))
	result.map_remove = show(map:RemoveAsync("a"))
	result.map_after_remove = show((map:GetAsync("a")))
	result.map_expiry_0 = err(function()
		map:SetAsync("x", 1, 0)
	end)
	result.map_expiry_long = err(function()
		map:SetAsync("x", 1, 3888001)
	end)
	result.map_key_long = err(function()
		map:SetAsync(string.rep("k", 129), 1, 60)
	end)
	result.map_key_128 = err(function()
		map:SetAsync(string.rep("k", 128), 1, 60)
	end)
	result.map_key_empty = err(function()
		map:SetAsync("", 1, 60)
	end)
	result.map_value_function = err(function()
		map:SetAsync("x", function() end, 60)
	end)
	result.map_value_big = err(function()
		map:SetAsync("x", string.rep("v", 33 * 1024), 60)
	end)
	result.map_range_count_0 = err(function()
		map:GetRangeAsync(Enum.SortDirection.Ascending, 0)
	end)
	result.map_range_count_201 = err(function()
		map:GetRangeAsync(Enum.SortDirection.Ascending, 201)
	end)
	result.map_sortkey_bool = err(function()
		map:SetAsync("x", 1, 60, true)
	end)

	map:SetAsync("short", 1, 1)
	task.wait(2)
	result.map_expired = show((map:GetAsync("short")))

	local hash = MemoryStoreService:GetHashMap("localrun_m12_hash_" .. suffix)
	result.hash_class = hash.ClassName
	result.hash_set = show(hash:SetAsync("k1", { v = 1 }, 60))
	result.hash_set_again = show(hash:SetAsync("k1", { v = 2 }, 60))
	hash:SetAsync("k2", "two", 60)
	hash:SetAsync("k3", 3, 60)
	result.hash_get = show(hash:GetAsync("k1"))
	result.hash_update = show(hash:UpdateAsync("k3", function(old)
		return old * 2
	end, 60))
	result.hash_remove = show(hash:RemoveAsync("k2"))
	local listed = {}
	local pages = hash:ListItemsAsync(1)
	result.hash_pages_class = pages.ClassName
	while true do
		for _, item in pages:GetCurrentPage() do
			table.insert(listed, item.key .. "=" .. show(item.value))
		end
		if pages.IsFinished then
			break
		end
		pages:AdvanceToNextPageAsync()
	end
	table.sort(listed)
	result.hash_list = table.concat(listed, " ")
	result.hash_list_count_0 = err(function()
		hash:ListItemsAsync(0)
	end)

	local queue = MemoryStoreService:GetQueue("localrun_m12_queue_" .. suffix, 2)
	result.queue_class = queue.ClassName
	queue:AddAsync("low", 60, 0)
	queue:AddAsync("high", 60, 5)
	queue:AddAsync("low2", 60, 0)
	queue:AddAsync({ t = 1 }, 60, 5)
	result.queue_size = show(queue:GetSizeAsync())
	local items, id = queue:ReadAsync(3, false, 0)
	local values = {}
	for _, item in items do
		table.insert(values, show(item))
	end
	result.queue_read = table.concat(values, " ")
	result.queue_id = type(id)
	result.queue_size_invisible = show(queue:GetSizeAsync(true))
	local rest = queue:ReadAsync(3, false, 0)
	result.queue_read_rest = #rest
	result.queue_all_or_nothing = show((queue:ReadAsync(5, true, 0)))
	queue:RemoveAsync(id)
	task.wait(2.5)
	local back = queue:ReadAsync(5, false, 0)
	local backValues = {}
	for _, item in back do
		table.insert(backValues, show(item))
	end
	result.queue_after_timeout = table.concat(backValues, " ")
	result.queue_remove_bad = err(function()
		queue:RemoveAsync("bogus")
	end)
	result.queue_count_0 = err(function()
		queue:ReadAsync(0)
	end)
	result.queue_count_101 = err(function()
		queue:ReadAsync(101)
	end)
	local started = os.clock()
	local waited = queue:ReadAsync(1, false, 1)
	result.queue_wait_empty = show(waited)
	result.queue_waited = os.clock() - started > 0.8
	result.queue_expiry_long = err(function()
		queue:AddAsync(1, 3888001)
	end)
	result.queue_invisibility_bad = err(function()
		MemoryStoreService:GetQueue("localrun_m12_q2_" .. suffix, -1)
	end)
	result.name_long = err(function()
		MemoryStoreService:GetSortedMap(string.rep("n", 129))
	end)
	return result
end
