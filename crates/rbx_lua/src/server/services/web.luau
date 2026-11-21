--!nonstrict
-- The web-backed services a Local Run answers by itself, our own, written
-- from the creator docs and what the WebMarket and WebServices goldens
-- measured on a live server: every listing's Pages object, AssetService's
-- game side and InsertService (Roblox's public web APIs when online, kept
-- for offline runs), AnalyticsService (events kept in a local log, nothing
-- sent), and the services Studio's Play answers without a backend:
-- ExperienceNotificationService, CommerceService, ConfigService and
-- MatchmakingService (Studio's local server attributes).
local host = ...

local M = {}

local function fail(message)
	error(message, 0)
end

local function typeName(value)
	if value == nil then
		return "nil"
	end
	local t = typeof(value)
	return if t == "number" then "double" else t
end

local function int64(value)
	if type(value) == "string" and tonumber(value) then
		value = tonumber(value)
	end
	if type(value) ~= "number" then
		fail("Unable to cast " .. typeName(value) .. " to int64")
	end
	return math.floor(value)
end

local function isPlayer(value)
	return typeof(value) == "Instance" and value:IsA("Player")
end

local function web(url, what)
	return host.web(url, "GET", nil, what)
end

-- Pages ---------------------------------------------------------------

local pages = {} -- instance id -> { page, finished, next }

-- A Pages object of `class` on `first`; `next()` answers the following
-- page and whether it's the last (nil: `first` is the only page).
local function lazyPages(class, first, finished, next)
	local instance, id = host.new(class, class)
	pages[id] = { page = first, finished = finished or next == nil, next = next }
	return instance
end

-- `list` cut into pages of `size`.
function M.pages(class, list, size)
	size = math.max(1, size or 50)
	local function slice(from)
		local page = {}
		for i = from, math.min(from + size - 1, #list) do
			table.insert(page, list[i])
		end
		return page, from + size > #list
	end
	local at = 1
	local first, last = slice(at)
	return lazyPages(class, first, last, function()
		at += size
		return slice(at)
	end)
end

local PAGES = {}

function PAGES.GetCurrentPage(self)
	return table.clone(pages[host.id(self)].page)
end

function PAGES.AdvanceToNextPageAsync(self)
	local object = pages[host.id(self)]
	if object.finished then
		fail("No pages to advance to")
	end
	task.wait()
	object.page, object.finished = object.next()
end

-- AssetService and InsertService ---------------------------------------

local ASSETS = {}

function ASSETS.GetGamePlacesAsync(_)
	task.wait()
	if game.GameId == 0 then
		return M.pages("StandardPages", {}, 50)
	end
	local list = {}
	local cursor = ""
	repeat
		local found = web(
			"https://develop.roblox.com/v1/universes/" .. game.GameId .. "/places?sortOrder=Asc&limit=100" .. cursor,
			"AssetService:GetGamePlacesAsync()"
		)
		for _, place in found.data or {} do
			table.insert(list, { Name = place.name, PlaceId = place.id })
		end
		cursor = if type(found.nextPageCursor) == "string" then "&cursor=" .. found.nextPageCursor else ""
	until cursor == ""
	return M.pages("StandardPages", list, 50)
end

function ASSETS.GetBundleDetailsAsync(_, bundleId)
	bundleId = int64(bundleId)
	local ok, found = pcall(web, "https://catalog.roblox.com/v1/bundles/" .. bundleId .. "/details", "AssetService:GetBundleDetailsAsync()")
	if not ok then
		fail(found)
	end
	local items = {}
	for _, item in found.items or {} do
		table.insert(items, { Id = item.id, Name = item.name, Type = item.type })
	end
	return {
		Id = found.id,
		Name = found.name,
		Description = found.description,
		BundleType = found.bundleType,
		Items = items,
	}
end

local AUDIO_LIMIT = 30

-- What the toolbox says of `ids`, as GetAudioMetadataAsync's entries.
local function audioMetadata(ids, what)
	if #ids == 0 then
		return {}
	end
	local found = web("https://apis.roblox.com/toolbox-service/v1/items/details?assetIds=" .. table.concat(ids, ","), what)
	local out = {}
	for _, entry in found.data or {} do
		local asset = entry.asset or {}
		local audio = asset.audioDetails or {}
		table.insert(out, {
			AssetId = asset.id,
			Title = audio.title or asset.name,
			Artist = audio.artist or (entry.creator and entry.creator.name) or "",
			Duration = asset.duration or 0,
			AudioType = if audio.audioType == "SoundEffect"
				then Enum.AudioSubType.SoundEffect
				else Enum.AudioSubType.Music,
		})
	end
	return out
end

function ASSETS.GetAudioMetadataAsync(_, idList)
	if type(idList) ~= "table" then
		fail("Unable to cast " .. typeName(idList) .. " to Array")
	end
	if #idList > AUDIO_LIMIT then
		fail("AssetService:GetAudioMetadataAsync() only accepts up to " .. AUDIO_LIMIT .. " ids")
	end
	local ids = {}
	for _, id in idList do
		table.insert(ids, tostring(int64(id)))
	end
	task.wait()
	return audioMetadata(ids, "AssetService:GetAudioMetadataAsync()")
end

function ASSETS.SearchAudioAsync(_, params)
	if typeof(params) ~= "Instance" or not params:IsA("AudioSearchParams") then
		fail("Argument 1 missing or nil")
	end
	local size = math.clamp(params.MaxPageSize, 1, 100)
	local keyword = params.SearchKeyword
	local function page(cursor)
		local url = "https://apis.roblox.com/toolbox-service/v1/marketplace/3?limit="
			.. size
			.. "&keyword="
			.. game:GetService("HttpService"):UrlEncode(keyword)
			.. (if cursor then "&cursor=" .. cursor else "")
		local found = web(url, "AssetService:SearchAudioAsync()")
		if type(found.data) ~= "table" then
			fail("Unexpected type for data, expected array got null")
		end
		local ids = {}
		for _, item in found.data do
			table.insert(ids, tostring(item.id))
		end
		local next = if type(found.nextPageCursor) == "string" then found.nextPageCursor else nil
		return audioMetadata(ids, "AssetService:SearchAudioAsync()"), next
	end
	local first, cursor = page(nil)
	return lazyPages("AudioPages", first, cursor == nil, function()
		local list
		list, cursor = page(cursor)
		return list, cursor == nil
	end)
end

-- A model asset, its roots under a Model (as Roblox wraps them).
local function loadAsset(method, assetId)
	assetId = int64(assetId)
	if assetId <= 0 then
		fail(method .. " failed because HTTP 404 (Not Found)")
	end
	local ok, token = pcall(host.download_asset, assetId)
	if not ok then
		fail(method .. " failed because " .. tostring(token))
	end
	return host.model_of_download(token, "Model")
end

function ASSETS.LoadAssetAsync(_, assetId)
	return loadAsset("AssetService:LoadAssetAsync()", assetId)
end

function ASSETS.GetAssetIdsForPackageAsync(_, packageId)
	packageId = int64(packageId)
	local model = loadAsset("AssetService:GetAssetIdsForPackageAsync()", packageId)
	model:Destroy()
	return { packageId }
end

-- The content maps a Create*Async call takes: each must wrap an
-- EditableImage; unknown keys warn.
local function contentMaps(method, content, keys)
	if type(content) ~= "table" then
		fail("Unable to cast " .. typeName(content) .. " to Dictionary")
	end
	local maps = {}
	for key, value in content do
		if not table.find(keys, key) then
			warn(method .. ": unknown key " .. tostring(key) .. " ignored")
		elseif typeof(value) ~= "Content" or typeof(value.Object) ~= "Instance" or not value.Object:IsA("EditableImage") then
			fail(method .. ": " .. key .. " must be a Content wrapping an EditableImage")
		else
			maps[key] = value
		end
	end
	if next(maps) == nil then
		fail(method .. ": no content maps were provided")
	end
	return maps
end

local DECAL_MAPS = { "ColorMap", "MetalnessMap", "NormalMap", "RoughnessMap", "EmissiveMask" }

local function created(class, method, content, keys)
	local maps = contentMaps(method, content, keys)
	task.wait()
	local instance = Instance.new(class)
	for key, value in maps do
		instance[key .. "Content"] = value
	end
	return instance
end

function ASSETS.CreateSurfaceAppearanceAsync(_, content)
	return created("SurfaceAppearance", "AssetService:CreateSurfaceAppearanceAsync", content, DECAL_MAPS)
end

function ASSETS.CreateDecalAsync(_, content)
	return created("Decal", "AssetService:CreateDecalAsync", content, DECAL_MAPS)
end

function ASSETS.CreateTextureAsync(_, content)
	return created("Texture", "AssetService:CreateTextureAsync", content, DECAL_MAPS)
end

local COMPOSE_LIMIT = 8

function ASSETS.ComposeDecalAsync(_, decal, layers)
	if typeof(decal) ~= "Instance" or not decal:IsA("Decal") then
		fail("Argument 1 missing or nil")
	end
	if type(layers) ~= "table" or #layers == 0 or #layers > COMPOSE_LIMIT then
		fail("AssetService:ComposeDecalAsync: layers must hold 1 to " .. COMPOSE_LIMIT .. " layers")
	end
	for _, layer in layers do
		if type(layer) ~= "table" or typeof(layer.ColorMap) ~= "Content" then
			fail("AssetService:ComposeDecalAsync: every layer needs a ColorMap")
		end
	end
	task.wait()
	-- The top layer shows: compositing by alpha is the renderer's job.
	local top = layers[#layers]
	decal.ColorMapContent = top.ColorMap
	for _, key in { "MetalnessMap", "NormalMap", "RoughnessMap" } do
		if typeof(top[key]) == "Content" then
			decal[key .. "Content"] = top[key]
		end
	end
end

-- What Studio says to the calls that publish to the creator's account:
-- a Local Run publishes nothing.
local function noPublishing(method)
	return function()
		task.wait()
		fail(method .. " can't publish from a Local Run: publish the place and use it in a live game")
	end
end

ASSETS.CreateAssetAsync = noPublishing("AssetService:CreateAssetAsync()")
ASSETS.CreateAssetVersionAsync = noPublishing("AssetService:CreateAssetVersionAsync()")
ASSETS.CreatePlaceAsync = noPublishing("AssetService:CreatePlaceAsync()")
ASSETS.CreatePlaceInPlayerInventoryAsync = noPublishing("AssetService:CreatePlaceInPlayerInventoryAsync()")
ASSETS.SavePlaceAsync = noPublishing("AssetService:SavePlaceAsync()")
ASSETS.CreateDataModelContentAsync = noPublishing("AssetService:CreateDataModelContentAsync()")

function ASSETS.PromptCreatePlatformContentAsync(_, player)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	host.notify(player, "Publish (test)", "The game asks to publish a creation. A Local Run publishes nothing.")
	task.wait()
	return Enum.PromptCreateAssetResult.PermissionDenied, 0
end

function ASSETS.PromptImportAnimationClipFromVideoAsync(_, player)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	task.wait()
	return Enum.PromptCreateAssetResult.PermissionDenied, nil
end

function ASSETS.GetLatestVersionUriAsync(_, asset)
	if typeof(asset) ~= "Content" then
		fail("Unable to cast " .. typeName(asset) .. " to Content")
	end
	task.wait()
	return asset
end

local INSERT = {}

function INSERT.LoadAsset(_, assetId)
	return loadAsset("InsertService:LoadAsset()", assetId)
end

function INSERT.LoadAssetVersion(_, versionId)
	return loadAsset("InsertService:LoadAssetVersion()", versionId)
end

function INSERT.GetLatestAssetVersionAsync(_, assetId)
	assetId = int64(assetId)
	local found = web("https://economy.roblox.com/v2/assets/" .. assetId .. "/details", "InsertService:GetLatestAssetVersionAsync()")
	return found.AssetId or assetId
end

local function freeSearch(category, method)
	return function(_, searchText, pageNum)
		if type(searchText) ~= "string" then
			fail("Unable to cast " .. typeName(searchText) .. " to string")
		end
		pageNum = int64(pageNum or 0)
		local url = "https://apis.roblox.com/toolbox-service/v1/marketplace/"
			.. category
			.. "?limit=30&keyword="
			.. game:GetService("HttpService"):UrlEncode(searchText)
		local found = web(url, method)
		local results = {}
		for _, item in found.data or {} do
			table.insert(results, { AssetId = item.id, AssetVersionId = item.id, Name = "", CreatorName = "" })
		end
		return { { CurrentStartIndex = pageNum * 30 + 1, TotalCount = found.totalResults or #results, Results = results } }
	end
end

INSERT.GetFreeModelsAsync = freeSearch(10, "InsertService:GetFreeModelsAsync()")
INSERT.GetFreeDecalsAsync = freeSearch(13, "InsertService:GetFreeDecalsAsync()")

-- AnalyticsService -------------------------------------------------------

local ANALYTICS = {}

local function logEvent(method, player, fields)
	if not isPlayer(player) then
		fail("AnalyticsService:" .. method .. "(): player must be a Player")
	end
	fields.event = method
	fields.userId = player.UserId
	fields.time = host.now_ms()
	host.analytics(host.encode(fields))
end

local function text(value, method, index)
	if value == nil then
		fail("Argument " .. index .. " missing or nil")
	end
	if type(value) ~= "string" and type(value) ~= "number" then
		fail("Unable to cast " .. typeName(value) .. " to string")
	end
	return tostring(value)
end

local function enumOf(enum, value, name)
	if typeof(value) == "EnumItem" and value.EnumType == enum then
		return value.Name
	end
	if type(value) == "string" then
		for _, item in enum:GetEnumItems() do
			if item.Name == value then
				return value
			end
		end
	end
	fail("Unable to cast " .. typeName(value) .. " to " .. name)
end

function ANALYTICS.LogCustomEvent(_, player, eventName, value, customFields)
	logEvent("LogCustomEvent", player, {
		name = text(eventName, "LogCustomEvent", 2),
		value = tonumber(value) or 1,
		customFields = customFields,
	})
end

function ANALYTICS.LogEconomyEvent(_, player, flowType, currencyType, amount, endingBalance, transactionType, itemSku, customFields)
	logEvent("LogEconomyEvent", player, {
		flowType = enumOf(Enum.AnalyticsEconomyFlowType, flowType, "AnalyticsEconomyFlowType"),
		currencyType = text(currencyType, "LogEconomyEvent", 3),
		amount = tonumber(amount) or 0,
		endingBalance = tonumber(endingBalance) or 0,
		transactionType = text(transactionType, "LogEconomyEvent", 6),
		itemSku = itemSku,
		customFields = customFields,
	})
end

function ANALYTICS.LogFunnelStepEvent(_, player, funnelName, sessionId, step, stepName, customFields)
	logEvent("LogFunnelStepEvent", player, {
		funnelName = text(funnelName, "LogFunnelStepEvent", 2),
		sessionId = sessionId,
		step = tonumber(step) or 1,
		stepName = stepName,
		customFields = customFields,
	})
end

function ANALYTICS.LogOnboardingFunnelStepEvent(_, player, step, stepName, customFields)
	logEvent("LogOnboardingFunnelStepEvent", player, {
		step = int64(step),
		stepName = stepName,
		customFields = customFields,
	})
end

function ANALYTICS.LogJourneyEvent(_, player, journeyName, nodeName, sessionId, customFields)
	logEvent("LogJourneyEvent", player, {
		journeyName = text(journeyName, "LogJourneyEvent", 2),
		nodeName = text(nodeName, "LogJourneyEvent", 3),
		sessionId = sessionId,
		customFields = customFields,
	})
end

function ANALYTICS.LogProgressionEvent(_, player, path, status, level, levelName, customFields)
	logEvent("LogProgressionEvent", player, {
		path = text(path, "LogProgressionEvent", 2),
		status = enumOf(Enum.AnalyticsProgressionType, status, "AnalyticsProgressionType"),
		level = int64(level),
		levelName = levelName,
		customFields = customFields,
	})
end

for _, kind in { "Start", "Complete", "Fail" } do
	local method = "LogProgression" .. kind .. "Event"
	ANALYTICS[method] = function(_, player, path, level, levelName, customFields)
		logEvent(method, player, {
			path = text(path, method, 2),
			level = int64(level),
			levelName = levelName,
			customFields = customFields,
		})
	end
end

function ANALYTICS.GetDurationLoggerTimestamp(_)
	return math.floor(host.clock() * 1000)
end

function ANALYTICS.GetPlayerSegmentsAsync(_, player)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	task.wait()
	return {}
end

-- ExperienceNotificationService ----------------------------------------

local NOTIFICATIONS = {}

local function clientOnly(method)
	if host.server() then
		fail(method .. " can only be called from the client")
	end
end

function NOTIFICATIONS.CanPromptOptInAsync(_)
	clientOnly("ExperienceNotificationService:CanPromptOptInAsync()")
	task.wait()
	return false
end

function NOTIFICATIONS.PromptOptIn(self)
	clientOnly("ExperienceNotificationService:PromptOptIn()")
	local id = host.id(self)
	host.notify(game:GetService("Players").LocalPlayer, "Notifications (test)", "The game asks to send experience notifications. A Local Run sends none.")
	task.defer(host.fire, id, "OptInPromptClosed")
end

-- CommerceService --------------------------------------------------------

local COMMERCE = {}

function COMMERCE.GetCommerceProductInfoAsync(_, productId)
	if type(productId) ~= "string" then
		fail("Unable to cast " .. typeName(productId) .. " to string")
	end
	task.wait()
	fail("CommerceService:GetCommerceProductInfoAsync() failed: commerce products need a live game")
end

function COMMERCE.UserEligibleForRealWorldCommerceAsync(_)
	clientOnly("CommerceService:UserEligibleForRealWorldCommerceAsync()")
	task.wait()
	return false
end

function COMMERCE.PromptCommerceProductPurchase(self, player, productId)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	local id = host.id(self)
	host.notify(player, "Commerce (test)", "The game offers a real-world product. A Local Run sells nothing.")
	task.defer(host.fire, id, "PromptCommerceProductPurchaseFinished", player, tostring(productId))
end

function COMMERCE.PromptRealWorldCommerceBrowser(_, player, url)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	host.notify(player, "Commerce (test)", "The game opens " .. tostring(url) .. ". A Local Run opens no browser.")
end

-- ConfigService and ConfigSnapshot --------------------------------------

local CONFIG = {}
local testing = {} -- key -> value SetTestingValue gave
local snapshots = {} -- ConfigSnapshot ids

local function snapshot()
	local instance, id = host.new("ConfigSnapshot", "ConfigSnapshot")
	snapshots[id] = true
	return instance
end

function CONFIG.SetTestingValue(_, key, value)
	testing[text(key, "SetTestingValue", 1)] = value
	for id in snapshots do
		host.fire(id, "Value:" .. key)
	end
end

function CONFIG.ClearTestingValue(self, key)
	CONFIG.SetTestingValue(self, key, nil)
end

function CONFIG.GetConfigAsync(_)
	task.wait()
	return snapshot()
end

function CONFIG.GetConfigForPlayerAsync(_, player)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	task.wait()
	return snapshot()
end

local SNAPSHOT = {}

function SNAPSHOT.GetValue(_, key)
	return testing[text(key, "GetValue", 1)]
end

function SNAPSHOT.GetValueChangedSignal(self, key)
	return host.signal(host.id(self), "Value:" .. text(key, "GetValueChangedSignal", 1))
end

function SNAPSHOT.Refresh(_) end

-- MatchmakingService: Studio's server attributes, set by the game itself.

local MATCHMAKING = {}
local attributes = nil

function MATCHMAKING.InitializeServerAttributesForStudio(_, values)
	if type(values) ~= "table" then
		return false, "Server attributes must be a dictionary"
	end
	attributes = table.clone(values)
	return true, nil
end

function MATCHMAKING.GetServerAttribute(_, name)
	if attributes == nil then
		return nil, "Server attributes have not been initialized"
	end
	return attributes[tostring(name)], nil
end

function MATCHMAKING.SetServerAttribute(_, name, value)
	if attributes == nil then
		return false, "Server attributes have not been initialized"
	end
	attributes[tostring(name)] = value
	return true, nil
end

local TABLES = {
	AssetService = ASSETS,
	InsertService = INSERT,
	AnalyticsService = ANALYTICS,
	ExperienceNotificationService = NOTIFICATIONS,
	CommerceService = COMMERCE,
	ConfigService = CONFIG,
	ConfigSnapshot = SNAPSHOT,
	MatchmakingService = MATCHMAKING,
}

local EVENTS = {
	ExperienceNotificationService = { OptInPromptClosed = true },
	CommerceService = { PromptCommerceProductPurchaseFinished = true },
	ConfigSnapshot = { UpdateAvailable = true },
}

function M.member(class, id, key)
	if pages[id] then
		if key == "IsFinished" then
			return pages[id].finished, true
		end
		local found = PAGES[key]
		return found, found ~= nil
	end
	if class == "ConfigSnapshot" then
		if key == "Outdated" then
			return false, true
		elseif key == "Error" then
			return Enum.ConfigSnapshotErrorState.None, true
		end
	end
	if EVENTS[class] and EVENTS[class][key] then
		return host.signal(id, key), true
	end
	local members = TABLES[class]
	local found = members and members[key]
	return found, found ~= nil
end

return M
