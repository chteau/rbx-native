-- DataStoreService's values on a server: what a JSON round trip through
-- SetAsync/GetAsync keeps (numbers, sparse arrays, empty tables, unicode),
-- what Get/Set/Update/Increment/RemoveAsync return (DataStoreKeyInfo,
-- versions, user ids, metadata), UpdateAsync's arguments and a cancelled
-- update, versions and their listing, ListKeysAsync, an ordered store's
-- sorted pages, and AllScopes key names. Everything goes to keys prefixed
-- `localrun_m12_` in a scope of this pass's own and is removed at the end.
return function(h)
	local DataStoreService = game:GetService("DataStoreService")
	local HttpService = game:GetService("HttpService")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	-- A value's shape, the same whatever order pairs walks a table in.
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
			table.insert(parts, type(k):sub(1, 1) .. tostring(k) .. "=" .. show(v[k]))
		end
		return "{" .. table.concat(parts, ",") .. "}"
	end
	local function info(keyInfo)
		if typeof(keyInfo) ~= "Instance" then
			return typeof(keyInfo)
		end
		return string.format(
			"%s users=%s meta=%s created=%s updated=%s",
			keyInfo.ClassName,
			show(keyInfo:GetUserIds()),
			show(keyInfo:GetMetadata()),
			type(keyInfo.CreatedTime),
			type(keyInfo.UpdatedTime)
		)
	end
	-- Whatever an interrupted pass left behind, in any scope.
	pcall(function()
		local leftOptions = Instance.new("DataStoreOptions")
		leftOptions.AllScopes = true
		local left = DataStoreService:GetDataStore("localrun_m12_values", "", leftOptions)
		local leftPages = left:ListKeysAsync("localrun_m12_")
		while true do
			for _, item in leftPages:GetCurrentPage() do
				left:RemoveAsync(item.KeyName)
			end
			if leftPages.IsFinished then
				break
			end
			leftPages:AdvanceToNextPageAsync()
		end
	end)
	local scope = "localrun_m12_" .. HttpService:GenerateGUID(false):sub(1, 8)
	local store = DataStoreService:GetDataStore("localrun_m12_values", scope)
	local touched = {}
	local function k(name)
		local key = "localrun_m12_" .. name
		touched[key] = true
		return key
	end

	-- Round trip of the awkward values, all in one value.
	local value = {
		int = 42,
		frac = 0.1,
		third = 1 / 3,
		big = 2 ^ 53 + 1,
		huge = 1e300,
		tiny = 1e-7,
		negzero = -0,
		neg = -12.5,
		sparse = { 1, nil, 3 },
		holes = { [1] = "a", [2] = "b", [4] = "d" },
		numkeys = { [10] = "x" },
		empty = {},
		nested = { {}, { {} } },
		unicode = "héllo ✓ \u{1F600}",
		control = "a\nb\t\0c",
		bool = false,
	}
	result.roundtrip_set = err(function()
		store:SetAsync(k("roundtrip"), value)
	end)
	local got = store:GetAsync(k("roundtrip"))
	if type(got) == "table" then
		for field in value do
			result["roundtrip_" .. field] = show(got[field])
		end
	else
		result.roundtrip_get = show(got)
	end
	result.top_string = show((function()
		store:SetAsync(k("top"), "plain")
		return store:GetAsync(k("top"))
	end)())
	result.top_number = show((function()
		store:SetAsync(k("top"), 7.25)
		return store:GetAsync(k("top"))
	end)())
	result.top_bool = show((function()
		store:SetAsync(k("top"), true)
		return store:GetAsync(k("top"))
	end)())

	-- What each call returns.
	local options = Instance.new("DataStoreSetOptions")
	options:SetMetadata({ tag = "v1", n = 3 })
	local version = store:SetAsync(k("info"), { coins = 5 }, { 101, 202 }, options)
	result.set_returns = type(version)
	result.version_shape = type(version) == "string" and (version:gsub("%x", "x")) or "none"
	local v, keyInfo = store:GetAsync(k("info"))
	result.get_value = show(v)
	result.get_info = info(keyInfo)
	result.get_version_matches = typeof(keyInfo) == "Instance" and keyInfo.Version == version
	result.get_created_equals_updated = typeof(keyInfo) == "Instance" and keyInfo.CreatedTime == keyInfo.UpdatedTime
	result.get_created_ms = typeof(keyInfo) == "Instance" and math.abs(keyInfo.CreatedTime / 1000 - os.time()) < 120
	local missing, missingInfo = store:GetAsync(k("missing"))
	result.get_missing = show(missing) .. " " .. typeof(missingInfo)

	local seen = {}
	local updated, updatedInfo = store:UpdateAsync(k("info"), function(old, oldInfo)
		table.insert(seen, show(old))
		table.insert(seen, info(oldInfo))
		old.coins += 1
		return old, { 303 }
	end)
	result.update_args = table.concat(seen, " | ")
	result.update_returns = show(updated) .. " | " .. info(updatedInfo)
	result.update_new_version = typeof(updatedInfo) == "Instance" and updatedInfo.Version ~= version
	result.update_kept_created = typeof(updatedInfo) == "Instance"
		and typeof(keyInfo) == "Instance"
		and updatedInfo.CreatedTime == keyInfo.CreatedTime

	local newSeen = {}
	local fresh = store:UpdateAsync(k("fresh"), function(old, oldInfo)
		table.insert(newSeen, show(old))
		table.insert(newSeen, typeof(oldInfo))
		return "made"
	end)
	result.update_missing = table.concat(newSeen, " ") .. " -> " .. show(fresh)
	local cancelled, cancelledInfo = store:UpdateAsync(k("fresh"), function()
		return nil
	end)
	result.update_cancel = show(cancelled) .. " " .. typeof(cancelledInfo)
	result.update_cancel_kept = show(store:GetAsync(k("fresh")))

	local n1, nInfo = store:IncrementAsync(k("count"))
	local n2 = store:IncrementAsync(k("count"), 10)
	local n3 = store:IncrementAsync(k("count"), -3)
	result.increment = show(n1) .. " " .. show(n2) .. " " .. show(n3) .. " " .. info(nInfo)
	store:SetAsync(k("count"), 2.7)
	result.increment_on_float = err(function()
		return store:IncrementAsync(k("count"), 1)
	end)
	result.increment_on_float_value = show(store:GetAsync(k("count")))

	local removed, removedInfo = store:RemoveAsync(k("info"))
	result.remove_returns = show(removed) .. " | " .. info(removedInfo)
	result.remove_then_get = show(store:GetAsync(k("info")))
	local again, againInfo = store:RemoveAsync(k("info"))
	result.remove_again = show(again) .. " " .. typeof(againInfo)

	-- Versions.
	local first = store:SetAsync(k("versioned"), "one")
	local second = store:SetAsync(k("versioned"), "two")
	result.version_differs = first ~= second
	local old, oldInfo = store:GetVersionAsync(k("versioned"), first)
	result.get_version = show(old) .. " " .. info(oldInfo)
	local pages = store:ListVersionsAsync(k("versioned"), Enum.SortDirection.Descending)
	local listed = {}
	for _, item in pages:GetCurrentPage() do
		table.insert(
			listed,
			string.format(
				"%s:%s:%s",
				item.ClassName,
				tostring(item.IsDeleted),
				item.Version == second and "second" or item.Version == first and "first" or "other"
			)
		)
	end
	result.list_versions = table.concat(listed, " ")
	result.list_versions_class = pages.ClassName
	result.list_versions_finished = pages.IsFinished
	result.version_at_now = err(function()
		return assert(store:GetVersionAtTimeAsync(k("versioned"), DateTime.now().UnixTimestampMillis) == "two")
	end)
	result.version_at_past = err(function()
		return store:GetVersionAtTimeAsync(k("versioned"), 0)
	end)
	result.version_at_old = show((store:GetVersionAtTimeAsync(k("versioned"), 1000)))

	-- Keys: two set, one removed.
	store:SetAsync(k("list_a"), 1)
	store:SetAsync(k("list_b"), 2)
	store:RemoveAsync(k("list_b"))
	local keyPages = store:ListKeysAsync("localrun_m12_list")
	local names = {}
	for _, item in keyPages:GetCurrentPage() do
		table.insert(names, item.ClassName .. ":" .. item.KeyName)
	end
	table.sort(names)
	result.list_keys = table.concat(names, " ")
	result.list_keys_class = keyPages.ClassName

	-- AllScopes: keys come back as scope/key.
	local allOptions = Instance.new("DataStoreOptions")
	allOptions.AllScopes = true
	local all = DataStoreService:GetDataStore("localrun_m12_values", "", allOptions)
	local allPages = all:ListKeysAsync(scope .. "/localrun_m12_list")
	local allNames = {}
	for _, item in allPages:GetCurrentPage() do
		table.insert(allNames, (item.KeyName:gsub(scope, "SCOPE")))
	end
	result.all_scopes_keys = table.concat(allNames, " ")
	result.all_scopes_get = show(all:GetAsync(scope .. "/" .. k("list_a")))
	result.all_scopes_bare_key = err(function()
		all:GetAsync(k("list_a"))
	end)

	-- An ordered store's pages.
	local ordered = DataStoreService:GetOrderedDataStore("localrun_m12_values_ordered", scope)
	local scores = { a = 5, b = 1, c = 9, d = 5, e = -2, f = 7 }
	for name, score in scores do
		ordered:SetAsync(k("o_" .. name), score)
	end
	local function page(p)
		local out = {}
		for _, entry in p:GetCurrentPage() do
			table.insert(out, (entry.key:gsub("localrun_m12_o_", "")) .. "=" .. show(entry.value))
		end
		return table.concat(out, " ")
	end
	local sorted = ordered:GetSortedAsync(false, 4)
	result.sorted_class = sorted.ClassName
	result.sorted_page1 = page(sorted)
	result.sorted_finished1 = sorted.IsFinished
	sorted:AdvanceToNextPageAsync()
	result.sorted_page2 = page(sorted)
	result.sorted_finished2 = sorted.IsFinished
	result.sorted_advance_past = err(function()
		sorted:AdvanceToNextPageAsync()
	end)
	result.sorted_asc_range = page(ordered:GetSortedAsync(true, 10, 0, 5))
	result.ordered_get = show(ordered:GetAsync(k("o_c")))
	result.ordered_increment = show(ordered:IncrementAsync(k("o_c"), 2))
	result.ordered_update = show(ordered:UpdateAsync(k("o_e"), function(old)
		return old * 10
	end))
	result.ordered_remove = show(ordered:RemoveAsync(k("o_e")))
	result.ordered_after_remove = page(ordered:GetSortedAsync(true, 10, -100, 0))
	result.ordered_float_value = err(function()
		ordered:SetAsync(k("o_g"), 2.0)
	end)

	for name in scores do
		pcall(ordered.RemoveAsync, ordered, k("o_" .. name))
	end
	pcall(ordered.RemoveAsync, ordered, k("o_g"))
	for key in touched do
		pcall(store.RemoveAsync, store, key)
	end
	return result
end
