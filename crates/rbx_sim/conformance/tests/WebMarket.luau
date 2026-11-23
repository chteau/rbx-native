-- MarketplaceService's newer members and AssetService's game side on a
-- server, never buying, prompting or creating anything: argument errors,
-- the shapes of what comes back, and what a subscription id that doesn't
-- exist gives. Fields under `_` hold live data (they differ between the
-- sandbox experience and an unpublished Local Run). Needs a player.
return function(h)
	local MarketplaceService = game:GetService("MarketplaceService")
	local AssetService = game:GetService("AssetService")
	local Players = game:GetService("Players")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	-- "ok:<type>" or the error.
	local function kind(f)
		local ok, value = pcall(f)
		return ok and ("ok:" .. typeof(value)) or (tostring(value):gsub("^.-:%d+: ", ""))
	end
	local function keys(t)
		if type(t) ~= "table" then
			return typeof(t)
		end
		local list = {}
		for k, v in t do
			table.insert(list, tostring(k) .. ":" .. typeof(v))
		end
		table.sort(list)
		return table.concat(list, ",")
	end
	local player = Players:GetPlayers()[1]
	while not player do
		Players.PlayerAdded:Wait()
		player = Players:GetPlayers()[1]
	end

	-- Prompts: only the argument errors (a nil or wrong player).
	result.open_shop_nil = err(function()
		MarketplaceService:OpenShop(nil)
	end)
	result.bulk_nil = err(function()
		MarketplaceService:PromptBulkPurchase(nil, {}, {})
	end)
	result.bulk_items_string = err(function()
		MarketplaceService:PromptBulkPurchase(player, "x", {})
	end)
	result.cancel_nil = err(function()
		MarketplaceService:PromptCancelSubscription(nil, "EXP-0")
	end)
	result.roblox_sub_nil = err(function()
		MarketplaceService:PromptRobloxSubscriptionPurchase(nil)
	end)
	result.vip_nil = err(function()
		MarketplaceService:PromptVIPServerPurchase(nil)
	end)
	result.transfer_zero = err(function()
		return MarketplaceService:PromptRobuxTransferAsync(player, 1, 0)
	end)
	result.transfer_nil = err(function()
		return MarketplaceService:PromptRobuxTransferAsync(nil, 1, 1)
	end)

	-- Reads.
	local okEligible, eligible = pcall(function()
		return MarketplaceService:CanUsersTransferRobuxAsync(player, 1)
	end)
	result.can_transfer = okEligible and typeof(eligible) or (tostring(eligible):gsub("^.-:%d+: ", ""))
	result.can_transfer_nil = err(function()
		return MarketplaceService:CanUsersTransferRobuxAsync(nil, 1)
	end)
	local okProducts, products = pcall(function()
		return MarketplaceService:GetDeveloperProductsAsync()
	end)
	result.dev_products = okProducts and typeof(products) or (tostring(products):gsub("^.-:%d+: ", ""))
	if okProducts then
		result.dev_products_class = products.ClassName
		local page = products:GetCurrentPage()
		result.dev_products_page = typeof(page)
		result._dev_products_count = #page
		result._dev_products_finished = products.IsFinished
	end
	result.sub_info = err(function()
		return MarketplaceService:GetSubscriptionProductInfoAsync("EXP-0")
	end)
	result.sub_details = kind(function()
		return MarketplaceService:GetUserSubscriptionDetailsAsync(player, "EXP-0")
	end)
	result.sub_details_nil = err(function()
		return MarketplaceService:GetUserSubscriptionDetailsAsync(nil, "EXP-0")
	end)
	result.sub_history = kind(function()
		return MarketplaceService:GetUserSubscriptionPaymentHistoryAsync(player, "EXP-0")
	end)
	local okRoblox, details = pcall(function()
		return MarketplaceService:GetRobloxSubscriptionDetailsAsync(player)
	end)
	result.roblox_sub = okRoblox and keys(details) or (tostring(details):gsub("^.-:%d+: ", ""))
	local okLevels, levels = pcall(function()
		return MarketplaceService:GetUsersPriceLevelsAsync({ player.UserId })
	end)
	result.price_levels = okLevels and typeof(levels) or (tostring(levels):gsub("^.-:%d+: ", ""))
	if okLevels then
		result.price_levels_count = #levels
		result.price_levels_first = keys(levels[1])
	end
	result.price_levels_string = err(function()
		return MarketplaceService:GetUsersPriceLevelsAsync("x")
	end)
	result.rank_empty = kind(function()
		return MarketplaceService:RankProductsAsync({})
	end)
	result.recommend = kind(function()
		return MarketplaceService:RecommendTopProductsAsync({ Enum.InfoType.GamePass })
	end)

	-- AssetService.
	local okPlaces, places = pcall(function()
		return AssetService:GetGamePlacesAsync()
	end)
	result.places = okPlaces and places.ClassName or (tostring(places):gsub("^.-:%d+: ", ""))
	if okPlaces then
		local page = places:GetCurrentPage()
		result.places_first = keys(page[1])
		result._places_count = #page
	end
	local okBundle, bundle = pcall(function()
		return AssetService:GetBundleDetailsAsync(192)
	end)
	result.bundle = okBundle and keys(bundle) or (tostring(bundle):gsub("^.-:%d+: ", ""))
	if okBundle then
		result.bundle_item = keys(bundle.Items and bundle.Items[1])
		result._bundle_name = bundle.Name
	end
	result.bundle_zero = err(function()
		return AssetService:GetBundleDetailsAsync(0)
	end)
	local okInfo, info = pcall(function()
		return MarketplaceService:GetProductInfo(192, Enum.InfoType.Bundle)
	end)
	result.bundle_info = okInfo and keys(info) or (tostring(info):gsub("^.-:%d+: ", ""))
	if okInfo then
		result.bundle_info_creator = keys(info.Creator)
		result._bundle_info_name = info.Name
	end
	result.bundle_info_zero = err(function()
		return MarketplaceService:GetProductInfo(0, Enum.InfoType.Bundle)
	end)
	result.audio_empty = kind(function()
		return AssetService:GetAudioMetadataAsync({})
	end)
	local okAudio, audio = pcall(function()
		return AssetService:GetAudioMetadataAsync({ 1843463175 })
	end)
	result._audio_one = okAudio and keys(audio[1]) or (tostring(audio):gsub("^.-:%d+: ", ""))
	local okSearch, found = pcall(function()
		local params = Instance.new("AudioSearchParams")
		params.SearchKeyword = "rain"
		params.MaxPageSize = 5
		return AssetService:SearchAudioAsync(params)
	end)
	result.search = okSearch and found.ClassName or (tostring(found):gsub("^.-:%d+: ", ""))
	if okSearch then
		local page = found:GetCurrentPage()
		result._search_first = keys(page[1])
		result._search_count = #page
	end
	result.audio_string = err(function()
		return AssetService:GetAudioMetadataAsync("x")
	end)
	result.load_zero = err(function()
		return AssetService:LoadAssetAsync(0)
	end)
	result.package_zero = err(function()
		return AssetService:GetAssetIdsForPackageAsync(0)
	end)
	result.surface_empty = err(function()
		return AssetService:CreateSurfaceAppearanceAsync({})
	end)
	result.decal_empty = err(function()
		return AssetService:CreateDecalAsync({})
	end)
	result.texture_empty = err(function()
		return AssetService:CreateTextureAsync({})
	end)
	result.compose_nil = err(function()
		return AssetService:ComposeDecalAsync(nil, {})
	end)
	result.search_nil = err(function()
		return AssetService:SearchAudioAsync(nil)
	end)
	return result
end
