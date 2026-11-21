--!nonstrict
-- MemoryStoreService in Local Run, our own, written from the creator docs
-- and what the MemoryStore golden measured on a live server: sorted maps
-- (ordered by numeric sort key, then string sort key, then none, each by
-- key), hash maps, queues (priority first, then first in, with read
-- invisibility), item expiry on the game clock, distributed counters, and
-- the errors with their exact text. With the experience's live data on
-- (see DataStoreService), sorted maps and queues are the experience's
-- real ones through Open Cloud; hash maps and counters, which Open Cloud
-- can't reach, stay in this server's memory, and so do queues under the
-- read-only guard (reading a queue hides its items: a write).
local host = ...

local M = {}

local objects = {} -- instance id -> structure
local byName = {} -- kind .. name -> instance

local MAX_EXPIRATION = 3888000
local KEY_LIMIT = 128
local VALUE_LIMIT = 32 * 1024

local function fail(message)
	error(message, 0)
end

local function now()
	return host.clock()
end

local function request()
	task.wait()
end

local function expiration(seconds)
	if type(seconds) ~= "number" or seconds < 0 or seconds > MAX_EXPIRATION then
		fail("InvalidExpirationTime: The field 'expiration' time must be between 0 and 3,888,000.")
	end
	return now() + seconds
end

local function checkKey(key)
	if key == nil or key == "" then
		fail("InvalidParameter: The key field is required.")
	end
	key = tostring(key)
	if #key > KEY_LIMIT then
		fail("InvalidParameter: The provided key is not valid.")
	end
	return key
end

local function encode(value)
	local ok, text = pcall(host.encode, value)
	if not ok or type(value) == "function" or typeof(value) == "Instance" then
		fail("InvalidRequest: Unable to convert value to json.")
	end
	if #text > VALUE_LIMIT then
		fail("ItemValueSizeTooLarge: The item value size is too large.")
	end
	return text
end

local function live(items)
	local t = now()
	for key, item in items do
		if item.expires <= t then
			items[key] = nil
		end
	end
	return items
end

local function sortKeyOf(value)
	if type(value) == "number" or type(value) == "string" then
		return value
	end
	return nil
end

-- Whether `object` goes to the experience's real memory store.
local function remote(object)
	if not host.live() then
		return false
	end
	return object.kind == "map" or (object.kind == "queue" and not host.live_read_only())
end

-- A live sorted map's items, as the local ones are kept.
local function remoteItems(self)
	local items = {}
	for _, item in host.ds_live("map_list", self.name) do
		items[item.key] = { key = item.key, value = item.value, expires = math.huge, sortKey = sortKeyOf(item.sortKey) }
	end
	return items
end

-- Sorted map order: numeric sort keys, string sort keys, then none.
local function rank(sortKey)
	return type(sortKey) == "number" and 0 or type(sortKey) == "string" and 1 or 2
end

local function before(a, b)
	local ra, rb = rank(a.sortKey), rank(b.sortKey)
	if ra ~= rb then
		return ra < rb
	end
	if ra < 2 and a.sortKey ~= b.sortKey then
		return a.sortKey < b.sortKey
	end
	return a.key < b.key
end

-- Whether `item` lies past `bound` (`side` 1: a lower bound, -1: upper).
-- A bound with only a sort key leaves out every item with that sort key.
local function past(item, bound, side)
	if bound == nil then
		return true
	end
	local edge = { key = bound.key, sortKey = sortKeyOf(bound.sortKey) }
	if edge.key == nil then
		if rank(item.sortKey) ~= rank(edge.sortKey) or item.sortKey ~= edge.sortKey then
			return side == 1 and before(edge, item) or before(item, edge)
		end
		return false
	end
	if side == 1 then
		return before(edge, item)
	end
	return before(item, edge)
end

local MAP = {}

function MAP.SetAsync(self, key, value, seconds, sortKey)
	key = checkKey(key)
	local text = encode(value)
	local expires = expiration(seconds)
	request()
	if remote(self) then
		return host.ds_live("map_set", self.name, key, text, seconds, sortKeyOf(sortKey))
	end
	local items = live(self.items)
	local existed = items[key] ~= nil
	items[key] = { key = key, value = text, expires = expires, sortKey = sortKeyOf(sortKey) }
	return existed
end

local function itemOf(self, key)
	if remote(self) then
		local found = host.ds_live("map_get", self.name, key)
		return found and { key = key, value = found.value, sortKey = sortKeyOf(found.sortKey) }
	end
	return live(self.items)[key]
end

function MAP.GetAsync(self, key)
	key = checkKey(key)
	request()
	local item = itemOf(self, key)
	if not item then
		return nil
	end
	return host.decode(item.value), item.sortKey
end

function MAP.UpdateAsync(self, key, transform, seconds)
	key = checkKey(key)
	local expires = expiration(seconds)
	request()
	local item = itemOf(self, key)
	local value, sortKey = transform(item and host.decode(item.value), item and item.sortKey)
	if value == nil then
		return nil
	end
	local text = encode(value)
	if not self.sorted then
		sortKey = nil
	elseif sortKey == nil and item then
		sortKey = item.sortKey
	end
	if remote(self) then
		host.ds_live("map_set", self.name, key, text, seconds, sortKeyOf(sortKey))
		return host.decode(text)
	end
	self.items[key] = { key = key, value = text, expires = expires, sortKey = sortKeyOf(sortKey) }
	return host.decode(text)
end

function MAP.RemoveAsync(self, key)
	key = checkKey(key)
	request()
	if remote(self) then
		host.ds_live("map_remove", self.name, key)
		return
	end
	self.items[key] = nil
end

function MAP.GetSizeAsync(self)
	request()
	local n = 0
	for _ in if remote(self) then remoteItems(self) else live(self.items) do
		n += 1
	end
	return n
end

function MAP.GetRangeAsync(self, direction, count, lower, upper)
	if type(count) ~= "number" or count < 1 or count > 200 then
		fail("InvalidRequest: Invalid value for max number of items to get.")
	end
	request()
	local list = {}
	for _, item in if remote(self) then remoteItems(self) else live(self.items) do
		if past(item, lower, 1) and past(item, upper, -1) then
			table.insert(list, item)
		end
	end
	table.sort(list, before)
	local descending = direction == Enum.SortDirection.Descending or direction == 1
	local out = {}
	for i = 1, math.min(count, #list) do
		local item = descending and list[#list + 1 - i] or list[i]
		table.insert(out, { key = item.key, value = host.decode(item.value), sortKey = item.sortKey })
	end
	return out
end

function MAP.ListItemsAsync(self, count)
	if type(count) ~= "number" or count < 1 or count > 200 then
		-- What a live server says, wrong field and all.
		fail("InvalidExpirationTime: The field 'expiration' time must be between 0 and 3,888,000.")
	end
	request()
	local list = {}
	for _, item in live(self.items) do
		table.insert(list, item)
	end
	table.sort(list, before)
	local instance, id = host.new("MemoryStoreHashMapPages", "MemoryStoreHashMapPages")
	objects[id] = { kind = "pages", list = list, at = 0, size = count }
	return instance
end

local QUEUE = {}

function QUEUE.AddAsync(self, value, seconds, priority)
	local text = encode(value)
	local expires = expiration(seconds)
	request()
	if remote(self) then
		host.ds_live("queue_add", self.name, text, seconds, type(priority) == "number" and priority or 0)
		return
	end
	self.serial += 1
	table.insert(self.items, {
		value = text,
		expires = expires,
		priority = type(priority) == "number" and priority or 0,
		serial = self.serial,
		hidden = 0,
	})
end

local function visible(self)
	local t = now()
	local list = {}
	for i = #self.items, 1, -1 do
		if self.items[i].expires <= t then
			table.remove(self.items, i)
		end
	end
	for _, item in self.items do
		if item.hidden <= t then
			table.insert(list, item)
		end
	end
	table.sort(list, function(a, b)
		if a.priority ~= b.priority then
			return a.priority > b.priority
		end
		return a.serial < b.serial
	end)
	return list
end

function QUEUE.ReadAsync(self, count, allOrNothing, waitTimeout)
	if type(count) ~= "number" or count < 1 or count > 100 then
		fail("InvalidParameter: The field count must be between 1 and 100.")
	end
	waitTimeout = type(waitTimeout) == "number" and waitTimeout or -1
	local deadline = now() + (waitTimeout < 0 and 30 or waitTimeout)
	request()
	if remote(self) then
		-- Open Cloud reads don't wait: ask again each second until the
		-- timeout.
		while true do
			local read = host.ds_live("queue_read", self.name, count, self.timeout)
			local enough = #read.values > 0 and (not allOrNothing or #read.values >= count)
			if enough then
				local values = {}
				for _, text in read.values do
					table.insert(values, host.decode(text))
				end
				return values, read.id
			end
			if now() >= deadline then
				return nil
			end
			task.wait(1)
		end
	end
	local list = visible(self)
	while (#list == 0 or (allOrNothing and #list < count)) and now() < deadline do
		task.wait()
		list = visible(self)
	end
	if #list == 0 or (allOrNothing and #list < count) then
		return nil
	end
	local id = host.guid()
	local values = {}
	local taken = {}
	for i = 1, math.min(count, #list) do
		local item = list[i]
		item.hidden = now() + self.timeout
		table.insert(values, host.decode(item.value))
		taken[item] = true
	end
	self.reads[id] = taken
	return values, id
end

function QUEUE.RemoveAsync(self, id)
	request()
	if remote(self) then
		host.ds_live("queue_remove", self.name, tostring(id))
		return
	end
	local taken = self.reads[tostring(id)]
	if not taken then
		return
	end
	self.reads[tostring(id)] = nil
	for i = #self.items, 1, -1 do
		if taken[self.items[i]] then
			table.remove(self.items, i)
		end
	end
end

function QUEUE.GetSizeAsync(self, excludeInvisible)
	request()
	if remote(self) then
		fail("GetSizeAsync isn't available for the experience's live queues: Open Cloud can't count them")
	end
	if excludeInvisible then
		return #visible(self)
	end
	visible(self)
	return #self.items
end

local function structure(class, kind, name, extra)
	local key = kind .. "\1" .. name
	local found = byName[key]
	if found then
		return found
	end
	local instance, id = host.new(class, name)
	local object = extra or {}
	object.kind = kind
	object.name = name
	object.items = object.items or {}
	objects[id] = object
	byName[key] = instance
	return instance
end

-- Distributed counters: kept in this server's memory (Open Cloud has
-- none), each expiring `expiration` seconds after its last increment.
local COUNTER = {}

function COUNTER.IncrementAsync(self, delta, seconds)
	if type(delta) == "string" and tonumber(delta) then
		delta = tonumber(delta)
	end
	if type(delta) ~= "number" then
		fail("Unable to cast " .. typeof(delta) .. " to int64")
	end
	local expires = expiration(seconds)
	request()
	if self.expires <= now() then
		self.value = 0
	end
	self.value += math.floor(delta)
	self.expires = expires
	return self.value
end

function COUNTER.GetAsync(self)
	request()
	if self.expires <= now() then
		self.value = 0
	end
	return self.value
end

local SERVICE = {}

function SERVICE.GetDistributedCounter(_, name)
	return structure("MemoryStoreDistributedCounter", "counter", tostring(name), { value = 0, expires = 0 })
end

function SERVICE.GetSortedMap(_, name)
	return structure("MemoryStoreSortedMap", "map", tostring(name), { sorted = true })
end

function SERVICE.GetHashMap(_, name)
	return structure("MemoryStoreHashMap", "hash", tostring(name), { sorted = false })
end

function SERVICE.GetQueue(_, name, timeout)
	return structure("MemoryStoreQueue", "queue", tostring(name), {
		timeout = type(timeout) == "number" and timeout >= 0 and timeout or 30,
		serial = 0,
		reads = {},
	})
end

local SORTED_ONLY = { GetRangeAsync = true }
local HASH_ONLY = { ListItemsAsync = true }

function M.member(class, id, key)
	if class == "MemoryStoreService" then
		return SERVICE[key], SERVICE[key] ~= nil
	end
	local object = objects[id]
	if not object then
		return nil, false
	end
	if object.kind == "pages" then
		if key == "IsFinished" then
			return object.at + object.size >= #object.list, true
		elseif key == "GetCurrentPage" then
			return function()
				local out = {}
				for i = object.at + 1, math.min(object.at + object.size, #object.list) do
					local item = object.list[i]
					table.insert(out, { key = item.key, value = host.decode(item.value) })
				end
				return out
			end, true
		elseif key == "AdvanceToNextPageAsync" then
			return function()
				if object.at + object.size >= #object.list then
					fail("No pages to advance to")
				end
				task.wait()
				object.at += object.size
			end, true
		end
		return nil, false
	end
	local methods = if object.kind == "queue" then QUEUE elseif object.kind == "counter" then COUNTER else MAP
	local f = methods[key]
	if not f or (SORTED_ONLY[key] and not object.sorted) or (HASH_ONLY[key] and object.sorted) then
		return nil, false
	end
	return function(self, ...)
		return f(objects[host.id(self)], ...)
	end, true
end

return M
