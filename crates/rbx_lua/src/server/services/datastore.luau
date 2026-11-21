--!nonstrict
-- DataStoreService in Local Run, our own, written from the creator docs and
-- what the DataStoreLimits and DataStoreValues goldens measured on a live
-- server: the argument rules and their exact messages, JSON storage rules,
-- versions and key info, ordered stores' pages and the request budgets.
-- The keys and versions live in the host's store (per place, on disk), so
-- what a run saves is there next run; or, for a published place run online
-- with an Open Cloud key, in the experience's real DataStores, as Studio's
-- Play uses them (`host.live`), where every call yields like a web request.
local host = ...

local M = {}

local function ds(op, ...)
	if host.live() then
		return host.ds_live(op, ...)
	end
	return host["ds_" .. op](...)
end

local objects = {} -- instance id -> what it is
local stores = {} -- bucket -> store instance
local budgets = {} -- request type name -> { left, at }
local queued = {} -- request type name -> threads waiting for budget

local NAME_LIMIT = 50
local KEY_LIMIT = 50
local VALUE_LIMIT = 4194304
local METADATA_LIMIT = 300
local USER_ID_LIMIT = 4
local QUEUE_LIMIT = 30

local function fail(message)
	error(message, 0)
end

local function rejected(code, reason)
	fail(string.format("502: API Services rejected request with error. Error code: %d Reason: %s", code, reason))
end

-- A string argument, cast as Roblox casts one.
local function str(value, index)
	if value == nil then
		fail(string.format("Argument %d missing or nil", index))
	end
	if type(value) == "number" then
		return tostring(value)
	end
	if type(value) ~= "string" then
		fail("Unable to cast " .. typeof(value) .. " to string")
	end
	return value
end

local function int(value, what)
	if type(value) == "string" then
		local n = tonumber(value)
		if not n then
			fail("Unable to cast string to " .. what)
		end
		value = n
	end
	if type(value) ~= "number" then
		fail("Unable to cast " .. typeof(value) .. " to " .. what)
	end
	return math.floor(value + 0.5)
end

local function func(value)
	if type(value) ~= "function" then
		fail("Unable to cast " .. (type(value) == "number" and "double" or typeof(value)) .. " to Function")
	end
	return value
end

-- Budgets: what a fresh live server starts with (the DataStoreLimits
-- golden's first pass), refilled per minute as the creator docs give it.
local RATES = {
	GetAsync = { 500, 60, 10 },
	SetIncrementAsync = { 500, 60, 10 },
	UpdateAsync = { 500, 60, 10 },
	GetSortedAsync = { 30, 5, 2 },
	SetIncrementSortedAsync = { 105, 30, 5 },
	OnUpdate = { 35, 30, 5 },
	ListAsync = { 30, 5, 2 },
	GetVersionAsync = { 30, 5, 2 },
	RemoveVersionAsync = { 30, 5, 2 },
	StandardRead = { 500, 60, 10 },
	StandardWrite = { 500, 60, 10 },
	StandardRemove = { 500, 60, 10 },
	StandardList = { 30, 5, 2 },
	OrderedRead = { 500, 60, 10 },
	OrderedWrite = { 105, 30, 5 },
	OrderedRemove = { 105, 30, 5 },
	OrderedList = { 30, 5, 2 },
}

-- The throttling error's code and call name, by the budget that ran out.
local DROPPED = {
	GetAsync = { 301, "GetAsync" },
	SetIncrementAsync = { 302, "SetAsync" },
	UpdateAsync = { 304, "UpdateAsync" },
	GetSortedAsync = { 305, "GetSortedAsync" },
	SetIncrementSortedAsync = { 303, "IncrementAsync" },
	ListAsync = { 305, "ListAsync" },
	GetVersionAsync = { 301, "GetVersionAsync" },
	RemoveVersionAsync = { 306, "RemoveVersionAsync" },
}

local function players()
	local ok, list = pcall(function()
		return #game:GetService("Players"):GetPlayers()
	end)
	return ok and list or 0
end

local function budget(name)
	local rate = RATES[name]
	if not rate then
		return 0
	end
	local now = host.clock()
	local entry = budgets[name]
	local perMinute = rate[2] + rate[3] * players()
	if not entry then
		entry = { left = rate[1], at = now }
		budgets[name] = entry
	end
	entry.left = math.min(math.max(rate[1], perMinute), entry.left + (now - entry.at) * perMinute / 60)
	entry.at = now
	return entry
end

local function spend(kinds, key)
	for _, name in kinds do
		local entry = budget(name)
		if entry.left < 1 then
			local waiting = queued[name] or 0
			if waiting >= QUEUE_LIMIT then
				local dropped = DROPPED[name] or { 301, name }
				fail(
					string.format(
						"%d: %s request dropped. Request was throttled, but throttled request queue was full.",
						dropped[1],
						dropped[2]
					)
				)
			end
			warn(
				"DataStore request was added to queue. If request queue fills, further requests will be dropped. Try sending fewer requests.Key = "
					.. tostring(key)
			)
			queued[name] = waiting + 1
			while budget(name).left < 1 do
				task.wait(0.1)
			end
			queued[name] -= 1
		end
		entry = budget(name)
		entry.left -= 1
	end
end

-- Every request: Studio's API access switch, the budget, then a frame's
-- round trip.
local function request(kinds, key)
	if not host.api_access() then
		fail("StudioAccessToApisNotAllowed: Studio access to APIs is not allowed.")
	end
	spend(kinds, key)
	task.wait()
end

-- Why `value` can't be stored, `nil` when it can.
local function unstorable(value, seen, top)
	local kind = type(value)
	if kind == "nil" or kind == "boolean" or kind == "number" then
		return nil
	end
	if kind == "string" then
		if utf8.len(value) == nil then
			return "104: Cannot store string in data store. Data stores can only accept valid UTF-8 characters."
		end
		return nil
	end
	if kind == "table" then
		if seen[value] then
			return "tables cannot be cyclic"
		end
		seen[value] = true
		local container = #value > 0 and "Array" or "Dictionary"
		for k, v in value do
			if type(k) ~= "string" and type(k) ~= "number" then
				return "Invalid table key type used"
			end
			local why = unstorable(v, seen, false)
			if why then
				if why:sub(1, 4) == "104:" then
					return "104: Cannot store "
						.. container
						.. " in data store. Data stores can only accept valid UTF-8 characters."
				end
				return why
			end
		end
		seen[value] = nil
		return nil
	end
	local name = kind == "function" and "Function" or typeof(value)
	if top == "update" then
		name = ""
	end
	return "104: Cannot store " .. name .. " in data store. Data stores can only accept valid UTF-8 characters."
end

local function encode(value, top)
	local why = unstorable(value, {}, top)
	if why then
		fail(why)
	end
	local text = host.encode(value)
	if #text > VALUE_LIMIT then
		fail("105: Serialized value exceeds 4MB limit.")
	end
	return text
end

local function decode(text)
	if text == nil then
		return nil
	end
	return host.decode(text)
end

local function userIds(list)
	if list == nil then
		return "[]"
	end
	if type(list) ~= "table" then
		fail("Unable to cast " .. typeof(list) .. " to Array")
	end
	if #list > USER_ID_LIMIT then
		fail("512: UserID size exceeds limit of " .. USER_ID_LIMIT)
	end
	for _, id in list do
		if type(id) ~= "number" then
			fail("513: Attribute userId format is invalid")
		end
		if id ~= math.floor(id) then
			rejected(15, "The provided user ids have invalid format.")
		end
	end
	return host.encode(list)
end

local function metadata(options)
	local meta = options and objects[host.id(options)] and objects[host.id(options)].metadata
	if meta == nil then
		return "{}"
	end
	local text = host.encode(meta)
	if text == "[]" then
		return "{}"
	end
	if #text > METADATA_LIMIT then
		fail("511: Metadata attribute size exceeds 300 bytes limit.")
	end
	return text
end

local function keyInfo(entry)
	local info, id = host.new("DataStoreKeyInfo", "DataStoreKeyInfo")
	objects[id] = {
		kind = "info",
		CreatedTime = entry.created,
		UpdatedTime = entry.updated,
		Version = entry.version,
		users = entry.users,
		meta = entry.meta,
	}
	return info
end

-- A store instance and what it reads: bucket, AllScopes, ordered.
local function store(class, name, scope, allScopes, ordered)
	local bucketKey = (ordered and "o" or "d") .. "\1" .. name .. "\1" .. (allScopes and "\2" or scope)
	local found = stores[bucketKey]
	if found then
		return found
	end
	local instance, id = host.new(class, name)
	objects[id] = { kind = "store", name = name, scope = scope, allScopes = allScopes, ordered = ordered }
	stores[bucketKey] = instance
	return instance
end

local function checkName(name, scope, options)
	name = str(name, 1)
	if name == "" then
		fail("DataStore name can't be empty string")
	end
	if #name > NAME_LIMIT then
		fail("DataStore name is too long")
	end
	local allScopes = options ~= nil and options.AllScopes == true
	if scope == nil then
		scope = allScopes and "" or "global"
	else
		scope = str(scope, 2)
	end
	if allScopes then
		if scope ~= "" then
			fail("DataStore scope should be an empty string allScopes is set to true")
		end
	elseif scope == "" then
		fail("DataStore scope can't be empty string")
	end
	if #scope > NAME_LIMIT then
		fail("DataStore scope is too long")
	end
	return name, scope, allScopes
end

-- The bucket and key a store's key names, checking its length.
local function locate(self, key)
	key = str(key, 1)
	local scope = self.scope
	if self.allScopes then
		local at = key:find("/", 1, true)
		if not at then
			fail("518: Invalid key format with allScopes = true.")
		end
		scope, key = key:sub(1, at - 1), key:sub(at + 1)
	end
	if key == "" then
		fail("101: Key name can't be empty.")
	end
	if #key > KEY_LIMIT then
		fail("102: Key name exceeds the 50 character limit.")
	end
	return host.bucket(self.ordered, self.name, scope), key
end

local function kinds(self, standard, ordered)
	return self.ordered and ordered or standard
end

local function pages(class, items, pageSize, makeItem)
	local instance, id = host.new(class, class)
	local size = pageSize > 0 and pageSize or #items
	local object = { kind = "pages", items = items, size = math.max(size, 1), at = 0, makeItem = makeItem }
	objects[id] = object
	object.IsFinished = #items <= object.size
	return instance
end

local function currentPage(object)
	local out = {}
	for i = object.at + 1, math.min(object.at + object.size, #object.items) do
		table.insert(out, object.makeItem(object.items[i]))
	end
	return out
end

local STORE = {}

function STORE.GetAsync(self, key)
	local bucket, name = locate(self, key)
	request(kinds(self, { "GetAsync", "StandardRead" }, { "GetAsync", "OrderedRead" }), name)
	local entry = ds("get", bucket, name)
	if not entry then
		return nil, nil
	end
	if self.ordered then
		return decode(entry.value)
	end
	return decode(entry.value), keyInfo(entry)
end

local BATCH_LIMIT = 100

-- Ordered stores only: each key there maps to { value = ... }, missing
-- keys are left out. Each key spends a read.
function STORE.BatchGetAsync(self, keys)
	if not self.ordered then
		fail("BatchGetAsync is only supported on OrderedDataStore")
	end
	if type(keys) ~= "table" then
		fail("Unable to cast " .. typeof(keys) .. " to Array")
	end
	if #keys == 0 or #keys > BATCH_LIMIT then
		fail("BatchGetAsync takes 1 to " .. BATCH_LIMIT .. " keys")
	end
	local out = {}
	for _, key in keys do
		local bucket, name = locate(self, key)
		request({ "GetAsync", "OrderedRead" }, name)
		local entry = ds("get", bucket, name)
		if entry then
			out[name] = { value = decode(entry.value) }
		end
	end
	return out
end

local function orderedValue(value)
	if type(value) == "number" then
		if value ~= math.floor(value) then
			fail("103: double is not allowed in data stores.")
		end
		return host.encode(value)
	end
	local name = type(value) == "string" and "string"
		or type(value) == "boolean" and "bool"
		or type(value) == "table" and (next(value) == nil or #value > 0) and "Array"
		or type(value) == "table" and "Dictionary"
		or typeof(value)
	fail("103: " .. name .. " is not allowed in data stores.")
end

function STORE.SetAsync(self, key, value, ids, options)
	local bucket, name = locate(self, key)
	local text = self.ordered and orderedValue(value) or encode(value)
	local users = self.ordered and "[]" or userIds(ids)
	local meta = self.ordered and "{}" or metadata(options)
	request(kinds(self, { "SetIncrementAsync", "StandardWrite" }, { "SetIncrementSortedAsync", "OrderedWrite" }), name)
	local entry = ds("put", bucket, name, text, users, meta)
	if self.ordered then
		return nil
	end
	return entry.version
end

function STORE.UpdateAsync(self, key, transform)
	local bucket, name = locate(self, key)
	func(transform)
	request(kinds(self, { "UpdateAsync", "StandardWrite" }, { "UpdateAsync", "OrderedWrite" }), name)
	local entry = ds("get", bucket, name)
	local old = entry and decode(entry.value)
	local value, ids, meta
	if self.ordered then
		value = transform(old)
	else
		value, ids, meta = transform(old, entry and keyInfo(entry) or nil)
	end
	if value == nil then
		return nil, nil
	end
	local text = self.ordered and orderedValue(value) or encode(value, "update")
	local users = self.ordered and "[]" or userIds(ids)
	local metaText = "{}"
	if meta ~= nil and not self.ordered then
		metaText = host.encode(meta)
		if metaText == "[]" then
			metaText = "{}"
		elseif #metaText > METADATA_LIMIT then
			fail("511: Metadata attribute size exceeds 300 bytes limit.")
		end
	end
	local written = ds("put", bucket, name, text, users, metaText)
	if self.ordered then
		return decode(written.value)
	end
	return decode(written.value), keyInfo(written)
end

function STORE.IncrementAsync(self, key, delta, ids, options)
	local bucket, name = locate(self, key)
	delta = delta == nil and 1 or int(delta, "int")
	local users = self.ordered and "[]" or userIds(ids)
	local meta = self.ordered and "{}" or metadata(options)
	request(kinds(self, { "SetIncrementAsync", "StandardWrite" }, { "SetIncrementSortedAsync", "OrderedWrite" }), name)
	local entry = ds("get", bucket, name)
	local old = entry and decode(entry.value) or 0
	if type(old) ~= "number" or old ~= math.floor(old) then
		rejected(24, "Key value is not numeric.")
	end
	local written = ds("put", bucket, name, host.encode(old + delta), users, meta)
	if self.ordered then
		return old + delta
	end
	return old + delta, keyInfo(written)
end

function STORE.RemoveAsync(self, key)
	local bucket, name = locate(self, key)
	request(kinds(self, { "SetIncrementAsync", "StandardRemove" }, { "SetIncrementSortedAsync", "OrderedRemove" }), name)
	local entry = ds("remove", bucket, name)
	if not entry then
		return nil, nil
	end
	if self.ordered then
		return decode(entry.value)
	end
	return decode(entry.value), keyInfo(entry)
end

function STORE.GetSortedAsync(self, ascending, pageSize, minValue, maxValue)
	pageSize = int(pageSize, "int")
	if pageSize < 1 or pageSize > 100 then
		fail("106: PageSize must be within a predefined range")
	end
	for _, bound in { minValue, maxValue } do
		if bound ~= nil and (type(bound) ~= "number" or bound ~= math.floor(bound)) then
			fail("106: MaxValue and MinValue must be integers.")
		end
	end
	if minValue and maxValue and minValue > maxValue then
		fail("107: MaxValue must be greater than or equal to MinValue.")
	end
	request({ "GetSortedAsync", "OrderedList" }, "")
	local entries = {}
	for _, item in ds("entries", host.bucket(true, self.name, self.scope)) do
		local value = decode(item.value)
		if (minValue == nil or value >= minValue) and (maxValue == nil or value <= maxValue) then
			table.insert(entries, { key = item.key, value = value })
		end
	end
	table.sort(entries, function(a, b)
		if a.value ~= b.value then
			return a.value < b.value
		end
		return a.key < b.key
	end)
	if not ascending then
		local n = #entries
		for i = 1, n // 2 do
			entries[i], entries[n + 1 - i] = entries[n + 1 - i], entries[i]
		end
	end
	return pages("DataStorePages", entries, pageSize, function(item)
		return { key = item.key, value = item.value }
	end)
end

function STORE.ListKeysAsync(self, prefix, pageSize, _cursor, excludeDeleted)
	prefix = prefix == nil and "" or str(prefix, 1)
	pageSize = pageSize == nil and 0 or int(pageSize, "int")
	request({ "ListAsync", "StandardList" }, prefix)
	local keys = ds("keys", self.name, not self.allScopes and self.scope or nil, prefix, excludeDeleted ~= true)
	return pages("DataStoreKeyPages", keys, pageSize, function(name)
		local item, id = host.new("DataStoreKey", "DataStoreKey")
		objects[id] = { kind = "key", KeyName = name }
		return item
	end)
end

local function versionInfo(v)
	local item, id = host.new("DataStoreObjectVersionInfo", "DataStoreObjectVersionInfo")
	objects[id] = { kind = "version", CreatedTime = v.created, IsDeleted = v.deleted, Version = v.version }
	return item
end

function STORE.ListVersionsAsync(self, key, direction, minDate, maxDate, pageSize)
	local bucket, name = locate(self, key)
	minDate = minDate and int(minDate, "int64") or 0
	maxDate = maxDate and int(maxDate, "int64") or 0
	pageSize = pageSize and int(pageSize, "int") or 0
	request({ "ListAsync", "StandardList" }, name)
	local list = {}
	for _, v in ds("versions", bucket, name) do
		if v.created >= minDate and (maxDate <= 0 or v.created <= maxDate) then
			table.insert(list, v)
		end
	end
	if direction == Enum.SortDirection.Descending or direction == 1 then
		local n = #list
		for i = 1, n // 2 do
			list[i], list[n + 1 - i] = list[n + 1 - i], list[i]
		end
	end
	return pages("DataStoreVersionPages", list, pageSize, versionInfo)
end

function STORE.GetVersionAsync(self, key, version)
	local bucket, name = locate(self, key)
	version = str(version, 2)
	request({ "GetVersionAsync", "StandardRead" }, name)
	local entry = ds("version", bucket, name, version)
	if not entry then
		rejected(25, "The provided version is not valid.")
	end
	if entry.deleted then
		return nil, nil
	end
	return decode(entry.value), keyInfo(entry)
end

function STORE.GetVersionAtTimeAsync(self, key, timestamp)
	local bucket, name = locate(self, key)
	timestamp = int(timestamp, "int64")
	if timestamp <= 0 or timestamp > host.now_ms() + 600000 then
		fail("InvalidTimestamp: Timestamp must be positive and not more than ten minutes in the future.")
	end
	request({ "GetVersionAsync", "StandardRead" }, name)
	local entry = ds("version_at", bucket, name, timestamp)
	if not entry or entry.deleted then
		return nil, nil
	end
	return decode(entry.value), keyInfo(entry)
end

function STORE.RemoveVersionAsync(self, key, version)
	local bucket, name = locate(self, key)
	version = str(version, 2)
	request({ "RemoveVersionAsync", "StandardRemove" }, name)
	if not ds("remove_version", bucket, name, version) then
		rejected(25, "The provided version is not valid.")
	end
end

local ORDERED_ONLY = { GetSortedAsync = true }
local STANDARD_ONLY = {
	ListKeysAsync = true,
	ListVersionsAsync = true,
	GetVersionAsync = true,
	GetVersionAtTimeAsync = true,
	RemoveVersionAsync = true,
}

local function requestType(kind)
	if typeof(kind) == "EnumItem" then
		return kind.Name
	end
	local value = int(kind, "DataStoreRequestType")
	for _, item in Enum.DataStoreRequestType:GetEnumItems() do
		if item.Value == value then
			return item.Name
		end
	end
	return nil
end

local SERVICE = {}

function SERVICE.GetDataStore(_, name, scope, options)
	local n, s, all = checkName(name, scope, options)
	return store("DataStore", n, s, all, false)
end

function SERVICE.GetGlobalDataStore()
	return store("DataStore", "GlobalDataStore", "global", false, false)
end

function SERVICE.GetOrderedDataStore(_, name, scope)
	local n, s = checkName(name, scope, nil)
	return store("OrderedDataStore", n, s, false, true)
end

function SERVICE.GetRequestBudgetForRequestType(_, kind)
	local name = requestType(kind)
	return name and math.floor(budget(name).left) or 0
end

function SERVICE.ListDataStoresAsync(_, prefix, pageSize)
	prefix = prefix == nil and "" or str(prefix, 1)
	pageSize = pageSize == nil and 0 or int(pageSize, "int")
	request({ "ListAsync", "StandardList" }, prefix)
	return pages("DataStoreListingPages", ds("stores", prefix), pageSize, function(s)
		local item, id = host.new("DataStoreInfo", "DataStoreInfo")
		objects[id] = { kind = "storeInfo", CreatedTime = s.created, DataStoreName = s.name, UpdatedTime = s.updated }
		return item
	end)
end

local function method(f)
	return f, true
end

-- `DataStoreService` and its objects' members, as (value, found).
function M.member(class, id, key)
	if class == "DataStoreService" then
		local f = SERVICE[key]
		if f then
			return method(f)
		end
		return nil, false
	end
	local object = objects[id]
	if class == "DataStoreSetOptions" or class == "DataStoreIncrementOptions" then
		if key == "SetMetadata" then
			return method(function(self, meta)
				if type(meta) ~= "table" then
					fail("Unable to cast " .. typeof(meta) .. " to Dictionary")
				end
				if #meta > 0 then
					fail("Unable to cast Array to Dictionary")
				end
				objects[host.id(self)] = { kind = "options", metadata = table.clone(meta) }
			end)
		elseif key == "GetMetadata" then
			return method(function(self)
				local o = objects[host.id(self)]
				return o and table.clone(o.metadata) or {}
			end)
		end
		return nil, false
	end
	if class == "DataStoreOptions" and key == "SetExperimentalFeatures" then
		return method(function() end)
	end
	if not object then
		return nil, false
	end
	if object.kind == "store" then
		local f = STORE[key]
		if not f or (ORDERED_ONLY[key] and not object.ordered) or (STANDARD_ONLY[key] and object.ordered) then
			return nil, false
		end
		return method(function(self, ...)
			return f(objects[host.id(self)], ...)
		end)
	elseif object.kind == "info" then
		if key == "GetUserIds" then
			return method(function()
				return host.decode(object.users)
			end)
		elseif key == "GetMetadata" then
			return method(function()
				return host.decode(object.meta)
			end)
		end
	elseif object.kind == "pages" then
		if key == "IsFinished" then
			return object.IsFinished, true
		elseif key == "GetCurrentPage" then
			return method(function()
				return currentPage(object)
			end)
		elseif key == "AdvanceToNextPageAsync" then
			return method(function()
				if object.IsFinished then
					fail("No pages to advance to")
				end
				task.wait()
				object.at += object.size
				object.IsFinished = object.at + object.size >= #object.items
			end)
		end
	end
	local value = object[key]
	if value ~= nil and key:sub(1, 1):match("%u") then
		return value, true
	end
	return nil, false
end

return M
