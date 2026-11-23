-- Pre-publish fixture: DataStore and MemoryStore calls outside pcall.
local DataStoreService = game:GetService("DataStoreService")
local HttpService = game:GetService("HttpService")
local store = DataStoreService:GetDataStore("Players")
local ordered = DataStoreService:GetOrderedDataStore("Wins")
local map = game:GetService("MemoryStoreService"):GetSortedMap("Queue")
local Profiles = {}
Profiles.store = DataStoreService:GetDataStore("Profiles")

local data = store:GetAsync("key") -- expect: datastore-without-pcall
store:SetAsync("key", 1) -- expect: datastore-without-pcall
ordered:GetSortedAsync(false, 10) -- expect: datastore-without-pcall
map:SetAsync("k", 1, 60) -- expect: datastore-without-pcall
DataStoreService:GetDataStore("Other"):IncrementAsync("k") -- expect: datastore-without-pcall
Profiles.store:GetAsync("k") -- expect: datastore-without-pcall

local ok, value = pcall(function()
	return store:GetAsync("key")
end)

local function save(key)
	store:UpdateAsync(key, function(old)
		return old
	end)
end
pcall(save, "key")

local function load()
	return store:GetAsync("x")
end
local function safeLoad()
	return load()
end
pcall(safeLoad)

xpcall(function()
	store:RemoveAsync("k")
end, warn)

-- HttpService's GetAsync isn't a data store.
local page = HttpService:GetAsync("https://example.com")
