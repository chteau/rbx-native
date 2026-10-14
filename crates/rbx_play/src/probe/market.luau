-- Test purchases for a Play session. The sandbox is a real published place, so
-- MarketplaceService's own prompts would charge real Robux. The editor rewrites
-- every `game:GetService("MarketplaceService")` in the place's scripts into
-- `Market.wrap(...)` (rbx_play::market); the proxy it returns shows our copy of
-- Roblox's prompt (`__rbxnative_market_ui`), keeps bought passes for the session
-- and calls the game's ProcessReceipt itself with a "DEV-" receipt. Everything
-- else on the service passes through to the real one.
local Market = {}

-- What Studio shows as the balance in a test purchase.
local TEST_BALANCE = 2147483647

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

-- Server -----------------------------------------------------------------------

local function startServer(real, net, signals, store)
	local Players = game:GetService("Players")
	local HttpService = game:GetService("HttpService")
	local owned = {}
	-- userId -> receipts ProcessReceipt has not granted yet (retried on the next purchase).
	local pending = {}
	-- userId -> true while a settle pass runs, so two passes never race on one queue.
	local settling = {}
	local warned = false

	local function info(id, infoType)
		local ok, result = pcall(function()
			return real:GetProductInfo(id, infoType)
		end)
		if ok and type(result) == "table" then
			return result
		end
		return nil
	end

	-- Runs off the Buy invoke: Roblox shows "Purchase completed" without waiting for
	-- ProcessReceipt, which may yield on DataStores for a while.
	local function settle(userId)
		if settling[userId] then
			return
		end
		settling[userId] = true
		task.spawn(function()
			local callback = store.ProcessReceipt
			if not callback and not warned then
				warned = true
				warn("[rbx-native] MarketplaceService.ProcessReceipt is not set; test purchases stay pending")
			end
			local batch = pending[userId] or {}
			pending[userId] = {}
			local remaining = Market.settle(batch, callback, Enum.ProductPurchaseDecision.PurchaseGranted)
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

	net.Buy.OnServerInvoke = function(player, kind, id)
		if type(id) ~= "number" then
			return false
		end
		local userId = player.UserId
		if kind == "GamePass" then
			owned[userId] = owned[userId] or {}
			owned[userId][id] = true
			local pass = info(id, Enum.InfoType.GamePass)
			print(string.format("[rbx-native] Test purchase: %s (GamePass %d)", pass and pass.Name or "?", id))
			return true
		elseif kind == "Product" then
			local product = info(id, Enum.InfoType.Product)
			local receipt = Market.receipt(
				userId,
				id,
				product and product.PriceInRobux or 0,
				game.PlaceId,
				"DEV-" .. HttpService:GenerateGUID(false)
			)
			pending[userId] = pending[userId] or {}
			table.insert(pending[userId], receipt)
			settle(userId)
			print(string.format("[rbx-native] Test purchase: %s (Product %d)", product and product.Name or "?", id))
			return true
		end
		return false
	end

	net.Closed.OnServerEvent:Connect(function(player, kind, id, purchased)
		if type(id) ~= "number" then
			return
		end
		if kind == "Product" then
			signals.product:Fire(player.UserId, id, purchased == true)
		elseif kind == "GamePass" then
			signals.pass:Fire(player, id, purchased == true)
		end
	end)

	net.Owns.OnServerInvoke = function(player, passId)
		local mine = owned[player.UserId]
		if mine and mine[passId] then
			return true
		end
		local ok, result = pcall(function()
			return real:UserOwnsGamePassAsync(player.UserId, passId)
		end)
		return ok and result == true
	end

	Players.PlayerRemoving:Connect(function(player)
		pending[player.UserId] = nil
	end)

	return {
		PromptProductPurchase = function(_, player, productId)
			net.Prompt:FireClient(player, "Product", productId)
		end,
		PromptGamePassPurchase = function(_, player, gamePassId)
			net.Prompt:FireClient(player, "GamePass", gamePassId)
		end,
		UserOwnsGamePassAsync = function(_, userId, gamePassId)
			local mine = owned[userId]
			if mine and mine[gamePassId] then
				return true
			end
			return real:UserOwnsGamePassAsync(userId, gamePassId)
		end,
	}
end

-- Client -----------------------------------------------------------------------

local function startClient(real, net, signals)
	local Players = game:GetService("Players")
	local player = Players.LocalPlayer
	local busy = false

	local function fetchInfo(kind, id)
		local infoType = if kind == "GamePass" then Enum.InfoType.GamePass else Enum.InfoType.Product
		local ok, info = pcall(function()
			return real:GetProductInfo(id, infoType)
		end)
		if not ok or type(info) ~= "table" then
			info = {}
		end
		local iconId = info.IconImageAssetId
		local icon = ""
		if type(iconId) == "number" and iconId ~= 0 then
			icon = string.format("rbxthumb://type=Asset&id=%d&w=150&h=150", iconId)
		end
		return {
			kind = kind,
			name = info.Name or tostring(id),
			price = info.PriceInRobux,
			icon = icon,
			balance = TEST_BALANCE,
		}
	end

	local function prompt(kind, id)
		if busy then
			return
		end
		busy = true
		local info = fetchInfo(kind, id)
		-- Required here so the module still loads where the UI module is absent.
		local MarketUI = require(script.Parent:WaitForChild("__rbxnative_market_ui"))
		MarketUI.prompt(player:WaitForChild("PlayerGui"), info, function()
			return net.Buy:InvokeServer(kind, id) == true
		end, function(purchased)
			busy = false
			if kind == "Product" then
				signals.product:Fire(player.UserId, id, purchased)
			else
				signals.pass:Fire(player, id, purchased)
			end
			net.Closed:FireServer(kind, id, purchased)
		end)
	end

	net.Prompt.OnClientEvent:Connect(prompt)

	return {
		PromptProductPurchase = function(_, target, productId)
			if target == player then
				task.spawn(prompt, "Product", productId)
			end
		end,
		PromptGamePassPurchase = function(_, target, gamePassId)
			if target == player then
				task.spawn(prompt, "GamePass", gamePassId)
			end
		end,
		UserOwnsGamePassAsync = function(_, userId, gamePassId)
			if userId == player.UserId then
				return net.Owns:InvokeServer(gamePassId) == true
			end
			return real:UserOwnsGamePassAsync(userId, gamePassId)
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

	local signals = {
		product = Instance.new("BindableEvent"),
		pass = Instance.new("BindableEvent"),
	}
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

	local store = {}
	local overrides = if isServer then startServer(real, net, signals, store) else startClient(real, net, signals)
	overrides.PromptProductPurchaseFinished = signals.product.Event
	overrides.PromptGamePassPurchaseFinished = signals.pass.Event
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
