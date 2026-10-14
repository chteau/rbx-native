-- Test purchases for a Play session. The sandbox is a real published place, so
-- MarketplaceService's own prompts would charge real Robux. The editor rewrites
-- every `game:GetService("MarketplaceService")` in the place's scripts into
-- `Market.wrap(...)` (rbx_play::market); the proxy it returns shows our copy of
-- Roblox's prompt (`__rbxnative_market_ui`), keeps bought passes, assets, bundles
-- and subscriptions for the session and calls the game's receipt handler itself
-- with a "DEV-" receipt. Everything else on the service passes through to the
-- real one.
local Market = {}

-- What Studio shows as the balance in a test purchase.
local TEST_BALANCE = 2147483647

-- Per kind: the Finished signal, and whether it leads with the user id (Product
-- only; the others lead with the Player). One table keeps firing a single path.
local KINDS = {
	Product = { signal = "PromptProductPurchaseFinished", userIdFirst = true },
	GamePass = { signal = "PromptGamePassPurchaseFinished" },
	Asset = { signal = "PromptPurchaseFinished" },
	Bundle = { signal = "PromptBundlePurchaseFinished" },
	Subscription = { signal = "PromptSubscriptionPurchaseFinished" },
}

local started = false
local proxyCache = nil

-- Pure helpers ---------------------------------------------------------------

function Market.formatRobux(n)
	local s = tostring(math.floor(n))
	local out = s
	repeat
		s = out
		out = string.gsub(s, "^(-?%d+)(%d%d%d)", "%1,%2")
	until out == s
	return out
end

function Market.receipt(userId, productId, price, placeId, purchaseId)
	local enum = Enum
	return {
		PurchaseId = purchaseId,
		PlayerId = userId,
		ProductId = productId,
		PlaceIdWherePurchased = placeId,
		CurrencySpent = price,
		CurrencyType = if enum then enum.CurrencyType.Robux else nil,
		ProductPurchaseChannel = if enum then enum.ProductPurchaseChannel.InExperience else nil,
		ReceiptType = if enum then enum.ReceiptType.DeveloperProduct else nil,
	}
end

-- Roblox retries receipts that errored or were not granted, so those stay pending.
function Market.settle(pending, callback, decisionGranted)
	local remaining = {}
	for _, receipt in ipairs(pending) do
		local keep = true
		if callback then
			local ok, decision = pcall(callback, receipt)
			keep = not ok or decision ~= decisionGranted
		end
		if keep then
			table.insert(remaining, receipt)
		end
	end
	return remaining
end

-- A filtered handler beats a catch-all, which beats ProcessReceipt. Each family
-- reports success with its own enum, folded here into "granted" for settle.
function Market.dispatcher(store, processed, granted)
	return function(receipt)
		local callback, expected
		for _, entry in ipairs(store.handlers) do
			if entry.filter and entry.filter[receipt.ProductId] then
				callback = entry.handler
				break
			end
		end
		if not callback then
			for _, entry in ipairs(store.handlers) do
				if not entry.filter then
					callback = entry.handler
					break
				end
			end
		end
		if callback then
			expected = processed or Enum.ReceiptDecision.Processed
		else
			callback = store.ProcessReceipt
			expected = granted or Enum.ProductPurchaseDecision.PurchaseGranted
		end
		if not callback then
			return nil
		end
		local decision = callback(receipt)
		if decision == expected then
			return "granted"
		end
		return decision
	end
end

-- Proxy ------------------------------------------------------------------------

function Market.proxy(real, overrides, store)
	return setmetatable({}, {
		__index = function(_, k)
			local override = overrides[k]
			if override ~= nil then
				return override
			end
			if k == "ProcessReceipt" then
				return store.ProcessReceipt
			end
			local value = real[k]
			if type(value) == "function" then
				return function(_, ...)
					return real[k](real, ...)
				end
			end
			return value
		end,
		__newindex = function(_, k, v)
			if k == "ProcessReceipt" then
				store.ProcessReceipt = v
			else
				real[k] = v
			end
		end,
		__tostring = function()
			return "MarketplaceService"
		end,
	})
end

-- Shared -----------------------------------------------------------------------

-- Subscription ids are strings ("EXP-..."), everything else is a number.
local function validId(kind, id)
	return KINDS[kind] ~= nil and type(id) == (if kind == "Subscription" then "string" else "number")
end

local function fire(signals, kind, player, id, flag)
	if KINDS[kind].userIdFirst then
		signals[kind]:Fire(player.UserId, id, flag)
	else
		signals[kind]:Fire(player, id, flag)
	end
end

-- Overrides both sides share. `side.prompt(target, kind, id)` shows a prompt;
-- `side.owns(userId, kind, id)` is true for a session purchase, nil when unknown.
local function commonOverrides(real, side, store)
	local overrides = {
		PromptProductPurchase = function(_, player, id)
			side.prompt(player, "Product", id)
		end,
		PromptGamePassPurchase = function(_, player, id)
			side.prompt(player, "GamePass", id)
		end,
		PromptPurchase = function(_, player, id)
			side.prompt(player, "Asset", id)
		end,
		PromptBundlePurchase = function(_, player, id)
			side.prompt(player, "Bundle", id)
		end,
		PromptSubscriptionPurchase = function(_, user, id)
			side.prompt(user, "Subscription", id)
		end,
		UserOwnsGamePassAsync = function(_, userId, id)
			if side.owns(userId, "GamePass", id) == true then
				return true
			end
			return real:UserOwnsGamePassAsync(userId, id)
		end,
		GetUserSubscriptionStatusAsync = function(_, user, id)
			if side.owns(user.UserId, "Subscription", id) == true then
				return { IsSubscribed = true, IsRenewing = true }
			end
			return real:GetUserSubscriptionStatusAsync(user, id)
		end,
		BindReceiptHandler = function(_, receiptType, handler, filter)
			if not Enum or receiptType ~= Enum.ReceiptType.DeveloperProduct then
				return real:BindReceiptHandler(receiptType, handler, filter)
			end
			local set = nil
			if filter then
				set = {}
				for _, productId in ipairs(filter) do
					set[productId] = true
				end
			end
			local entry = { handler = handler, filter = set }
			table.insert(store.handlers, entry)
			local connection = { Connected = true }
			function connection.Disconnect(c)
				local at = table.find(store.handlers, entry)
				if at then
					table.remove(store.handlers, at)
				end
				c.Connected = false
			end
			return connection
		end,
	}
	local byPlayer = {
		PlayerOwnsAsset = "Asset",
		PlayerOwnsAssetAsync = "Asset",
		PlayerOwnsBundle = "Bundle",
		PlayerOwnsBundleAsync = "Bundle",
	}
	for name, kind in pairs(byPlayer) do
		overrides[name] = function(_, player, id)
			if side.owns(player.UserId, kind, id) == true then
				return true
			end
			return real[name](real, player, id)
		end
	end
	return overrides
end

-- Server -----------------------------------------------------------------------

local function startServer(real, net, signals, store)
	local Players = game:GetService("Players")
	local HttpService = game:GetService("HttpService")
	-- owned[userId][kind][id] = true
	local owned = {}
	-- userId -> receipts the receipt handler has not granted yet (retried on the next purchase).
	local pending = {}
	-- userId -> true while a settle pass runs, so two passes never race on one queue.
	local settling = {}
	local warned = false

	local function lookup(kind, id)
		local ok, result = pcall(function()
			return real:GetProductInfo(id, Enum.InfoType[kind])
		end)
		if ok and type(result) == "table" then
			return result
		end
		return nil
	end

	-- Runs off the Buy invoke: Roblox shows "Purchase completed" without waiting for
	-- the receipt handler, which may yield on DataStores for a while.
	local function settle(userId)
		if settling[userId] then
			return
		end
		settling[userId] = true
		task.spawn(function()
			if not store.ProcessReceipt and #store.handlers == 0 and not warned then
				warned = true
				warn("[rbx-native] MarketplaceService.ProcessReceipt is not set; test purchases stay pending")
			end
			local batch = pending[userId] or {}
			pending[userId] = {}
			local remaining = Market.settle(batch, Market.dispatcher(store), "granted")
			-- Receipts bought during this pass go after the ones still owed.
			for _, receipt in ipairs(pending[userId] or {}) do
				table.insert(remaining, receipt)
			end
			if Players:GetPlayerByUserId(userId) then
				pending[userId] = remaining
			else
				pending[userId] = nil
			end
			settling[userId] = nil
		end)
	end

	local function isOwned(userId, kind, id)
		local mine = owned[userId]
		return mine ~= nil and mine[kind] ~= nil and mine[kind][id] == true
	end

	-- `shownName` is what the dialog showed: subscription info is client-only.
	net.Buy.OnServerInvoke = function(player, kind, id, shownName)
		if not validId(kind, id) then
			return false
		end
		local userId = player.UserId
		local data = if kind == "Subscription" then nil else lookup(kind, id)
		if kind == "Product" then
			local receipt = Market.receipt(
				userId,
				id,
				data and data.PriceInRobux or 0,
				game.PlaceId,
				"DEV-" .. HttpService:GenerateGUID(false)
			)
			pending[userId] = pending[userId] or {}
			table.insert(pending[userId], receipt)
			settle(userId)
		else
			owned[userId] = owned[userId] or {}
			owned[userId][kind] = owned[userId][kind] or {}
			owned[userId][kind][id] = true
		end
		local name = if data then data.Name else (if type(shownName) == "string" then shownName else "?")
		print(string.format("[rbx-native] Test purchase: %s (%s %s)", name, kind, tostring(id)))
		return true
	end

	net.Closed.OnServerEvent:Connect(function(player, kind, id, purchased)
		if validId(kind, id) then
			fire(signals, kind, player, id, purchased == true)
		end
	end)

	-- Session purchases only: the asking client falls back to the real
	-- ownership call itself, as it would without us.
	net.Owns.OnServerInvoke = function(player, kind, id)
		return validId(kind, id) and isOwned(player.UserId, kind, id)
	end

	Players.PlayerRemoving:Connect(function(player)
		pending[player.UserId] = nil
	end)

	return {
		prompt = function(player, kind, id)
			net.Prompt:FireClient(player, kind, id)
		end,
		owns = function(userId, kind, id)
			if isOwned(userId, kind, id) then
				return true
			end
			return nil
		end,
	}
end

-- Client -----------------------------------------------------------------------

local function startClient(real, net, signals)
	local Players = game:GetService("Players")
	local player = Players.LocalPlayer
	local busy = false

	-- Roblox's prompt shows a bundle's outfit (the avatar wearing it), not its
	-- catalog card; the card is the fallback.
	local function bundleIcon(id, items)
		for _, item in ipairs(if type(items) == "table" then items else {}) do
			if item.Type == "UserOutfit" and type(item.Id) == "number" then
				return string.format("rbxthumb://type=Outfit&id=%d&w=150&h=150", item.Id)
			end
		end
		return string.format("rbxthumb://type=BundleThumbnail&id=%d&w=150&h=150", id)
	end

	local function fetchInfo(kind, id)
		local ok, data = pcall(function()
			if kind == "Subscription" then
				return real:GetSubscriptionProductInfoAsync(id)
			end
			return real:GetProductInfo(id, Enum.InfoType[kind])
		end)
		local info = {
			kind = kind,
			name = tostring(id),
			icon = "",
			balance = TEST_BALANCE,
		}
		if not ok or type(data) ~= "table" then
			info.failed = true
			return info
		end
		info.name = data.Name or info.name
		if kind == "Asset" then
			info.icon = string.format("rbxthumb://type=Asset&id=%d&w=150&h=150", id)
		elseif kind == "Bundle" then
			info.icon = bundleIcon(id, data.Items)
		else
			local iconId = tonumber(data.IconImageAssetId)
			if iconId and iconId ~= 0 then
				info.icon = string.format("rbxthumb://type=Asset&id=%d&w=150&h=150", iconId)
			end
		end
		if kind == "Subscription" then
			local price = data.PriceInRobux
			if price == nil or price == 0 then
				info.priceText = (data.DisplayPrice or "") .. (data.DisplaySubscriptionPeriod or "")
			else
				info.price = price
				info.period = data.DisplaySubscriptionPeriod
			end
		else
			-- Bundles carry only the base price.
			info.price = data.PriceInRobux or data.UserBasePriceInRobux
		end
		return info
	end

	local function prompt(kind, id)
		if busy or not validId(kind, id) then
			return
		end
		busy = true
		local info = fetchInfo(kind, id)
		-- Required here so the module still loads where the UI module is absent.
		local MarketUI = require(script.Parent:WaitForChild("__rbxnative_market_ui"))
		MarketUI.prompt(player:WaitForChild("PlayerGui"), info, function()
			return net.Buy:InvokeServer(kind, id, info.name) == true
		end, function(purchased)
			busy = false
			fire(signals, kind, player, id, purchased)
			net.Closed:FireServer(kind, id, purchased)
		end)
	end

	net.Prompt.OnClientEvent:Connect(prompt)

	return {
		prompt = function(target, kind, id)
			if target == player then
				task.spawn(prompt, kind, id)
			end
		end,
		owns = function(userId, kind, id)
			if userId == player.UserId then
				return net.Owns:InvokeServer(kind, id) == true
			end
			return nil
		end,
	}
end

-- Public API -------------------------------------------------------------------

function Market.start()
	if started then
		return
	end
	started = true

	local real = game:GetService("MarketplaceService")
	local isServer = game:GetService("RunService"):IsServer()

	local signals = {}
	for kind in pairs(KINDS) do
		signals[kind] = Instance.new("BindableEvent")
	end
	local net = {}
	if isServer then
		net.Prompt = Instance.new("RemoteEvent")
		net.Buy = Instance.new("RemoteFunction")
		net.Closed = Instance.new("RemoteEvent")
		net.Owns = Instance.new("RemoteFunction")
		for name, remote in pairs(net) do
			remote.Name = name
			remote.Parent = script
		end
	else
		for _, name in ipairs({ "Prompt", "Buy", "Closed", "Owns" }) do
			net[name] = script:WaitForChild(name)
		end
	end

	local store = { handlers = {} }
	local side = if isServer then startServer(real, net, signals, store) else startClient(real, net, signals)
	local overrides = commonOverrides(real, side, store)
	for kind, def in pairs(KINDS) do
		overrides[def.signal] = signals[kind].Event
	end
	proxyCache = Market.proxy(real, overrides, store)
end

function Market.wrap(real)
	if real == nil then
		return nil
	end
	Market.start()
	-- The client's start() yields on the server's remotes; a script that wraps
	-- while another call is still starting waits for the same proxy.
	while proxyCache == nil do
		task.wait()
	end
	return proxyCache
end

return Market
