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
	for i = #handlers, 1, -1 do
		local entry = handlers[i]
		if not entry.filter or entry.filter[receipt.ProductId] then
			return entry.handler
		end
	end
	return processReceipt
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
	local entry = { handler = handler, filter = set }
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
	return host.product_info(id, kind)
end
SERVICE.GetProductInfoAsync = SERVICE.GetProductInfo

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
function M.finished(kind, player, id, bought, subscription)
	if not serviceId or not player then
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
