--!nonstrict
-- MarketplaceService in Local Run: test purchases with nothing paid. The
-- prompts go to the host, which shows its own purchase dialog; what the
-- player chose comes back through `finished`, which fires the Prompt*
-- events, records game passes and assets in the place's local ownership
-- table and hands developer products to ProcessReceipt, retried until it
-- answers PurchaseGranted (when the callback is set again, when the buyer
-- joins again, and every RETRY seconds). Argument errors are the ones the
-- Marketplace golden measured on a live server.
local host = ...

local M = {}

local RETRY = 10

local serviceId = nil
local processReceipt = nil
local pending = {} -- receipts not granted yet
local handlers = {} -- BindReceiptHandler's: { handler, filter }
local retrying = false

local function fail(message)
	error(message, 0)
end

local function int64(value)
	-- Like any number argument, a numeric string converts ("123").
	if type(value) == "string" and tonumber(value) then
		value = tonumber(value)
	end
	if type(value) == "string" then
		fail("Unable to cast string to int64")
	end
	if type(value) ~= "number" then
		fail("Unable to cast " .. typeof(value) .. " to int64")
	end
	return math.floor(value)
end

local function isPlayer(value)
	return typeof(value) == "Instance" and value:IsA("Player")
end

local function prompt(kind, method)
	return function(self, player, id)
		serviceId = host.id(self)
		if not isPlayer(player) then
			fail("MarketplaceService:" .. method .. "(): player should be of type Player.")
		end
		host.prompt(kind, player, int64(id))
	end
end

local function userId(value)
	if value == nil then
		fail("Argument 1 missing or nil")
	end
	return int64(value)
end

-- The handler for a receipt: the newest bound one whose filter takes its
-- product, else ProcessReceipt.
local function handlerFor(receipt)
	local kind = receipt.ReceiptType or Enum.ReceiptType.DeveloperProduct
	for i = #handlers, 1, -1 do
		local entry = handlers[i]
		if entry.receiptType == kind and (not entry.filter or entry.filter[receipt.ProductId]) then
			return entry.handler
		end
	end
	return if kind == Enum.ReceiptType.DeveloperProduct then processReceipt else nil
end

local function grant(receipt)
	local handler = handlerFor(receipt)
	if not handler then
		return false
	end
	local ok, decision = pcall(handler, receipt)
	if not ok then
		warn("ProcessReceipt failed: " .. tostring(decision))
		return false
	end
	return decision == Enum.ProductPurchaseDecision.PurchaseGranted
end

local function retry(onlyUser)
	if retrying then
		return
	end
	retrying = true
	local players = game:GetService("Players")
	local still = {}
	local list = pending
	pending = {}
	for _, receipt in list do
		local here = players:GetPlayerByUserId(receipt.PlayerId) ~= nil
		if (onlyUser == nil or receipt.PlayerId == onlyUser) and here then
			if not grant(receipt) then
				table.insert(still, receipt)
			end
		else
			table.insert(still, receipt)
		end
	end
	for _, receipt in pending do
		table.insert(still, receipt)
	end
	pending = still
	retrying = false
end

local started = false
local function startRetries()
	if started then
		return
	end
	started = true
	game:GetService("Players").PlayerAdded:Connect(function(player)
		task.spawn(retry, player.UserId)
	end)
	task.spawn(function()
		while true do
			task.wait(RETRY)
			if #pending > 0 then
				retry(nil)
			end
		end
	end)
end

local SERVICE = {}

SERVICE.PromptPurchase = prompt("Asset", "PromptPurchase")
SERVICE.PromptProductPurchase = prompt("Product", "PromptProductPurchase")
SERVICE.PromptGamePassPurchase = prompt("GamePass", "PromptGamePassPurchase")
SERVICE.PromptBundlePurchase = prompt("Bundle", "PromptBundlePurchase")

function SERVICE.PromptSubscriptionPurchase(self, player, subscription)
	serviceId = host.id(self)
	if not isPlayer(player) then
		fail("First argument to PromptSubscriptionPurchase must be a player")
	end
	host.prompt("Subscription", player, 0, tostring(subscription))
end

function SERVICE.BindReceiptHandler(self, receiptType, handler, filter)
	serviceId = host.id(self)
	if typeof(receiptType) ~= "EnumItem" then
		fail("Unable to cast " .. typeof(receiptType) .. " to ReceiptType")
	end
	if type(handler) ~= "function" then
		fail("Argument 2 missing or nil")
	end
	local set = nil
	if type(filter) == "table" then
		set = {}
		for _, productId in filter do
			set[productId] = true
		end
	end
	local entry = { handler = handler, filter = set, receiptType = receiptType }
	table.insert(handlers, entry)
	if host.server() then
		startRetries()
		if #pending > 0 then
			task.spawn(retry, nil)
		end
	end
	local connection = { Connected = true }
	function connection.Disconnect(c)
		c.Connected = false
		local at = table.find(handlers, entry)
		if at then
			table.remove(handlers, at)
		end
	end
	return connection
end

function SERVICE.GetUserSubscriptionStatusAsync(_, player, subscription)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	task.wait()
	return { IsSubscribed = host.owns(player.UserId, "subscription:" .. tostring(subscription)), IsRenewing = false }
end

function SERVICE.GetPassOwnershipAsync(_, user, pass)
	user = userId(user)
	pass = int64(pass)
	task.wait()
	return host.owns(user, "pass:" .. pass) or host.owns_live(user, "pass", pass)
end

function SERVICE.UserOwnsGamePassAsync(_, user, pass)
	user = userId(user)
	pass = int64(pass)
	task.wait()
	return host.owns(user, "pass:" .. pass) or host.owns_live(user, "pass", pass)
end

function SERVICE.PlayerOwnsAsset(_, player, asset)
	if player == nil then
		fail("Argument 1 missing or nil")
	end
	asset = int64(asset)
	if asset <= 0 then
		fail(
			"MarketplaceService:PlayerOwnsAsset() second argument is not a valid assetId (supplied assetId was less than 0)"
		)
	end
	task.wait()
	return host.owns(player.UserId, "asset:" .. asset) or host.owns_live(player.UserId, "asset", asset)
end
SERVICE.PlayerOwnsAssetAsync = SERVICE.PlayerOwnsAsset

function SERVICE.PlayerOwnsBundle(_, player, bundle)
	if player == nil then
		fail("Argument 1 missing or nil")
	end
	task.wait()
	return host.owns(player.UserId, "bundle:" .. int64(bundle))
end
SERVICE.PlayerOwnsBundleAsync = SERVICE.PlayerOwnsBundle

function SERVICE.GetProductInfo(_, id, infoType)
	id = int64(id)
	local kind = "Asset"
	if infoType ~= nil then
		if typeof(infoType) ~= "EnumItem" then
			fail("Unable to cast " .. (type(infoType) == "number" and "double" or typeof(infoType)) .. " to InfoType")
		end
		kind = infoType.Name
	end
	if id <= 0 then
		fail("MarketplaceService:GetProductInfo() " .. id .. " is not a valid assetId")
	end
	-- A bundle's info is its AssetService details (the WebMarket golden).
	if kind == "Bundle" then
		return game:GetService("AssetService"):GetBundleDetailsAsync(id)
	end
	return host.product_info(id, kind)
end
SERVICE.GetProductInfoAsync = SERVICE.GetProductInfo

-- The newer prompts: each goes to the host's test dialog, nothing paid.
local function playerOf(method, player)
	if not isPlayer(player) then
		fail("MarketplaceService:" .. method .. "(): player should be of type Player.")
	end
	return player
end

-- Not released on Roblox yet: a live server refuses these (golden
-- WebMarket).
local function unreleased(method)
	fail("MarketplaceService:" .. method .. "() API is not yet released.")
end

function SERVICE.OpenShop(_, _player)
	unreleased("OpenShop")
end

local BULK_LIMIT = 20

function SERVICE.PromptBulkPurchase(self, player, lineItems, _options)
	serviceId = host.id(self)
	if type(lineItems) ~= "table" then
		fail("Unable to cast " .. typeof(lineItems) .. " to Array")
	end
	-- A live server shrugs off an empty list (even without a player).
	if #lineItems == 0 then
		return
	end
	playerOf("PromptBulkPurchase", player)
	if not host.server() then
		fail("MarketplaceService:PromptBulkPurchase() can only be called from the server")
	end
	if #lineItems > BULK_LIMIT then
		fail("MarketplaceService:PromptBulkPurchase(): lineItems must hold 1 to " .. BULK_LIMIT .. " items")
	end
	local ids = {}
	for _, item in lineItems do
		if type(item) ~= "table" or typeof(item.Type) ~= "EnumItem" or item.Id == nil then
			fail("MarketplaceService:PromptBulkPurchase(): each line item needs a Type and an Id")
		end
		if type(item.PurchaseOptions) == "table" and #item.PurchaseOptions > 1 then
			fail("MarketplaceService:PromptBulkPurchase(): PurchaseOptions supports only one entry")
		end
		local what = if item.Type == Enum.MarketplaceProductType.AvatarBundle then "bundle:" else "asset:"
		table.insert(ids, what .. tostring(item.Id))
	end
	host.prompt("MarketplaceService.PromptBulkPurchase", player, #ids, table.concat(ids, ","))
end

function SERVICE.PromptCancelSubscription(self, player, subscription)
	serviceId = host.id(self)
	if not isPlayer(player) then
		fail("First argument to PromptCancelSubscription must be a player")
	end
	host.prompt("MarketplaceService.PromptCancelSubscription", player, 0, tostring(subscription))
end

function SERVICE.PromptRobloxSubscriptionPurchase(self, player)
	serviceId = host.id(self)
	if not isPlayer(player) then
		fail("MarketplaceService:promptRobloxSubscriptionPurchase() player should be of type Player")
	end
	host.prompt("MarketplaceService.PromptRobloxSubscriptionPurchase", player, 0)
end

function SERVICE.PromptVIPServerPurchase(_, _player)
	unreleased("PromptVIPServerPurchase")
end

local transfers = {} -- transfer id -> { sender, receiver, amount }

function SERVICE.PromptRobuxTransferAsync(self, sender, receiver, amount)
	serviceId = host.id(self)
	if sender == nil then
		fail("Argument 1 missing or nil")
	end
	playerOf("PromptRobuxTransferAsync", sender)
	if not host.server() then
		fail("MarketplaceService:PromptRobuxTransferAsync() can only be called from the server")
	end
	receiver = userId(receiver)
	amount = int64(amount)
	if amount <= 0 then
		fail("amount must be a positive integer")
	end
	local id = host.guid()
	transfers[id] = { sender = sender.UserId, receiver = receiver, amount = amount }
	host.prompt("MarketplaceService.PromptRobuxTransferAsync", sender, amount, id .. "," .. receiver)
	task.wait()
	return id
end

-- Switched off on Roblox for now (golden WebMarket).
function SERVICE.CanUsersTransferRobuxAsync(_, sender, receiver)
	if sender == nil then
		fail("Argument 1 missing or nil")
	end
	playerOf("CanUsersTransferRobuxAsync", sender)
	userId(receiver)
	task.wait()
	fail("CanUsersTransferRobuxAsync is not enabled")
end

-- The experience's developer products, from Roblox's public listing
-- (none for a place that isn't published).
function SERVICE.GetDeveloperProductsAsync(_)
	task.wait()
	local list = {}
	if game.GameId ~= 0 then
		local cursor = ""
		repeat
			local found = host.web(
				"https://apis.roblox.com/developer-products/v2/universes/"
					.. game.GameId
					.. "/developerproducts?limit=50"
					.. cursor,
				"GET",
				nil,
				"MarketplaceService:GetDeveloperProductsAsync()"
			)
			for _, product in found.developerProducts or {} do
				table.insert(list, {
					ProductId = product.productId or product.id,
					DeveloperProductId = product.id or product.productId,
					Name = product.name,
					Description = product.description,
					PriceInRobux = product.priceInRobux,
					IconImageAssetId = product.iconImageAssetId,
				})
			end
			cursor = if type(found.nextPageCursor) == "string" then "&cursor=" .. found.nextPageCursor else ""
		until cursor == ""
	end
	return host.pages("StandardPages", list, 50)
end

-- Roblox refuses a subscription id it doesn't know with HTTP 400; offline
-- an id is known once the ledger has it or when it looks like a real one
-- (EXP- and a long number).
local function subscriptionArgs(method, player, subscription)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	if type(subscription) ~= "string" then
		fail("Unable to cast " .. typeof(subscription) .. " to string")
	end
	if not host.server() then
		fail("MarketplaceService:" .. method .. "() can only be called from the server")
	end
	task.wait()
	if not subscription:match("^EXP%-%d%d%d%d%d+$") and not host.owns(player.UserId, "subscription:" .. subscription) then
		fail("MarketPlaceService::" .. method .. " failed because: HTTP 400")
	end
end

function SERVICE.GetSubscriptionProductInfoAsync(_, subscription)
	if host.server() then
		fail("Can only call GetSubscriptionProductInfoAsync from Script with RunContext = Client")
	end
	if type(subscription) ~= "string" then
		fail("Unable to cast " .. typeof(subscription) .. " to string")
	end
	task.wait()
	fail("MarketplaceService:GetSubscriptionProductInfoAsync() failed: subscription " .. subscription .. " was not found")
end

local MONTH = 30 * 24 * 3600

function SERVICE.GetUserSubscriptionDetailsAsync(_, player, subscription)
	subscriptionArgs("GetUserSubscriptionDetailsAsync", player, subscription)
	if host.owns(player.UserId, "subscription:" .. subscription) then
		return {
			SubscriptionState = Enum.SubscriptionState.SubscribedWillRenew,
			NextRenewTime = DateTime.fromUnixTimestamp(os.time() + MONTH),
		}
	end
	return { SubscriptionState = Enum.SubscriptionState.NeverSubscribed }
end

function SERVICE.GetUserSubscriptionPaymentHistoryAsync(_, player, subscription)
	subscriptionArgs("GetUserSubscriptionPaymentHistoryAsync", player, subscription)
	if host.owns(player.UserId, "subscription:" .. subscription) then
		return {
			{
				CycleStartTime = DateTime.fromUnixTimestamp(os.time()),
				CycleEndTime = DateTime.fromUnixTimestamp(os.time() + MONTH),
				PaymentStatus = Enum.SubscriptionPaymentStatus.Paid,
			},
		}
	end
	return {}
end

function SERVICE.GetRobloxSubscriptionDetailsAsync(_, player)
	if not isPlayer(player) then
		fail("Argument 1 missing or nil")
	end
	task.wait()
	return { IsSubscribed = host.owns(player.UserId, "roblox-subscription"), IsOriginExperience = false }
end

-- Regional price levels: Roblox's base market (100) for everyone here.
function SERVICE.GetUsersPriceLevelsAsync(_, userIds)
	if type(userIds) ~= "table" then
		fail("Unable to cast " .. typeof(userIds) .. " to Array")
	end
	task.wait()
	local out = {}
	for _, id in userIds do
		table.insert(out, { UserId = userId(id), PriceLevel = 100 })
	end
	return out
end

-- No personalisation without Roblox's recommender: the products come
-- back in the order given, recommendations empty.
function SERVICE.RankProductsAsync(_, products)
	if type(products) ~= "table" then
		fail("Unable to cast " .. typeof(products) .. " to Array")
	end
	if #products == 0 then
		fail("MarketplaceService:RankProductsAsync() must have at least one ProductIdentifier.")
	end
	task.wait()
	return table.clone(products)
end

function SERVICE.RecommendTopProductsAsync(_, infoTypes)
	if host.server() then
		fail("MarketplaceService:RecommendTopProductsAsync() Cannot be called from the Server.")
	end
	if type(infoTypes) ~= "table" then
		fail("Unable to cast " .. typeof(infoTypes) .. " to Array")
	end
	task.wait()
	return {}
end

local EVENTS = {
	PromptPurchaseFinished = true,
	PromptProductPurchaseFinished = true,
	PromptGamePassPurchaseFinished = true,
	PromptSubscriptionPurchaseFinished = true,
	PromptBundlePurchaseFinished = true,
	PromptPurchaseRequested = true,
	PromptProductPurchaseRequested = true,
	PromptGamePassPurchaseRequested = true,
	PromptBulkPurchaseFinished = true,
	PromptPremiumPurchaseFinished = true,
	PromptRobloxSubscriptionPurchaseFinished = true,
}

function M.member(_, id, key)
	serviceId = serviceId or id
	if key == "ProcessReceipt" then
		return nil,
			false,
			"ProcessReceipt is a callback member of MarketplaceService; you can only set the callback value, get is not available"
	end
	if EVENTS[key] then
		return host.signal(id, key), true
	end
	return SERVICE[key], SERVICE[key] ~= nil
end

function M.set(_, id, key, value)
	if key ~= "ProcessReceipt" then
		return false
	end
	serviceId = serviceId or id
	processReceipt = type(value) == "function" and value or nil
	if processReceipt and host.server() then
		startRetries()
		if #pending > 0 then
			task.spawn(retry, nil)
		end
	end
	return true
end

-- The host's purchase dialog closed: `bought` when the player confirmed.
-- A receipt for whoever bound a handler of its type.
local function deliver(receipt)
	table.insert(pending, receipt)
	startRetries()
	task.spawn(retry, receipt.PlayerId)
end

-- The host's dialog closed on one of the newer prompts.
local function finishedMore(kind, player, bought, extra)
	local user = player.UserId
	if kind == "MarketplaceService.PromptBulkPurchase" then
		local items = {}
		for entry in string.gmatch(extra, "[^,]+") do
			local what, itemId = string.match(entry, "^(%a+):(.+)$")
			if bought then
				host.grant(user, entry)
			end
			table.insert(items, {
				type = if what == "bundle"
					then Enum.MarketplaceProductType.AvatarBundle
					else Enum.MarketplaceProductType.AvatarAsset,
				id = itemId,
				status = if bought
					then Enum.MarketplaceItemPurchaseStatus.Success
					else Enum.MarketplaceItemPurchaseStatus.SystemError,
			})
		end
		local status = if bought
			then Enum.MarketplaceBulkPurchasePromptStatus.Completed
			else Enum.MarketplaceBulkPurchasePromptStatus.Aborted
		host.fire(serviceId, "PromptBulkPurchaseFinished", player, status, { RobuxSpent = 0, Items = items })
	elseif kind == "MarketplaceService.PromptCancelSubscription" then
		if bought and host.owns(user, "subscription:" .. extra) then
			host.revoke(user, "subscription:" .. extra)
			local players = game:GetService("Players")
			host.fire(host.id(players), "UserSubscriptionStatusChanged", player, extra)
		end
	elseif kind == "MarketplaceService.PromptRobloxSubscriptionPurchase" then
		if bought then
			host.grant(user, "roblox-subscription")
		end
		host.fire(serviceId, "PromptRobloxSubscriptionPurchaseFinished", player, bought)
	elseif kind == "MarketplaceService.PromptRobuxTransferAsync" then
		local transferId = string.match(extra, "^([^,]+)")
		local transfer = transfers[transferId]
		transfers[transferId] = nil
		if bought and transfer and host.server() then
			local function receipt(receiptType, who)
				return {
					ReceiptType = receiptType,
					PlayerId = who,
					TransferRequestId = transferId,
					SenderUserId = transfer.sender,
					ReceiverUserId = transfer.receiver,
					CurrencySpent = transfer.amount,
					CurrencyType = Enum.CurrencyType.Robux,
					PurchaseId = host.guid(),
					PlaceIdWherePurchased = game.PlaceId,
				}
			end
			deliver(receipt(Enum.ReceiptType.RobuxTransferSender, transfer.sender))
			deliver(receipt(Enum.ReceiptType.RobuxTransferReceiver, transfer.receiver))
		end
	end
end

function M.finished(kind, player, id, bought, subscription)
	if not serviceId or not player then
		return
	end
	if string.find(kind, "MarketplaceService.", 1, true) == 1 then
		finishedMore(kind, player, bought, subscription)
		return
	end
	local user = player.UserId

	if bought then
		if kind == "GamePass" then
			host.grant(user, "pass:" .. id)
		elseif kind == "Asset" then
			host.grant(user, "asset:" .. id)
		elseif kind == "Bundle" then
			host.grant(user, "bundle:" .. id)
		elseif kind == "Subscription" then
			host.grant(user, "subscription:" .. subscription)
		end
	end
	if kind == "Asset" then
		host.fire(serviceId, "PromptPurchaseFinished", player, id, bought)
	elseif kind == "GamePass" then
		host.fire(serviceId, "PromptGamePassPurchaseFinished", player, id, bought)
	elseif kind == "Bundle" then
		host.fire(serviceId, "PromptBundlePurchaseFinished", player, id, bought)
	elseif kind == "Subscription" then
		host.fire(serviceId, "PromptSubscriptionPurchaseFinished", player, subscription, bought)
	elseif kind == "Product" then
		host.fire(serviceId, "PromptProductPurchaseFinished", user, id, bought)
		if bought and host.server() then
			table.insert(pending, {
				PlayerId = user,
				PlaceIdWherePurchased = game.PlaceId,
				PurchaseId = host.guid(),
				ProductId = id,
				CurrencyType = Enum.CurrencyType.Robux,
				CurrencySpent = 0,
			})
			startRetries()
			task.spawn(retry, user)
		end
	end
end

return M
