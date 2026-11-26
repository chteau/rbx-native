-- MarketplaceService on a server, without buying anything: the argument
-- errors of the prompts and ownership checks, what a game pass check says
-- for a pass that doesn't exist, ProcessReceipt's slot, and the errors
-- GetProductInfo gives for ids that can't be found. Needs a player.
return function(h)
	local MarketplaceService = game:GetService("MarketplaceService")
	local Players = game:GetService("Players")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local player = Players:GetPlayers()[1]
	while not player do
		Players.PlayerAdded:Wait()
		player = Players:GetPlayers()[1]
	end
	result.prompt_nil_player = err(function()
		MarketplaceService:PromptPurchase(nil, 1)
	end)
	result.prompt_product_nil = err(function()
		MarketplaceService:PromptProductPurchase(nil, 1)
	end)
	result.prompt_pass_nil = err(function()
		MarketplaceService:PromptGamePassPurchase(nil, 1)
	end)
	result.prompt_pass_string = err(function()
		MarketplaceService:PromptGamePassPurchase(player, "x")
	end)
	result.prompt_subscription_nil = err(function()
		MarketplaceService:PromptSubscriptionPurchase(nil, "EXP-0")
	end)
	result.owns_pass_zero = err(function()
		return MarketplaceService:UserOwnsGamePassAsync(player.UserId, 0)
	end)
	result.owns_pass_bad_user = err(function()
		return MarketplaceService:UserOwnsGamePassAsync(nil, 1)
	end)
	result.owns_asset_nil = err(function()
		return MarketplaceService:PlayerOwnsAsset(nil, 1)
	end)
	result.owns_asset_zero = err(function()
		return MarketplaceService:PlayerOwnsAsset(player, 0)
	end)
	result.product_info_zero = err(function()
		return MarketplaceService:GetProductInfo(0)
	end)
	result.product_info_negative = err(function()
		return MarketplaceService:GetProductInfo(-5)
	end)
	result.product_info_bad_type = err(function()
		return MarketplaceService:GetProductInfo(1, 99)
	end)
	result.process_receipt_read = err(function()
		return MarketplaceService.ProcessReceipt
	end)
	result.process_receipt_set = err(function()
		MarketplaceService.ProcessReceipt = function()
			return Enum.ProductPurchaseDecision.NotProcessedYet
		end
	end)
	result.process_receipt_set_number = err(function()
		MarketplaceService.ProcessReceipt = 5
	end)
	MarketplaceService.ProcessReceipt = nil
	result.decision_items = #Enum.ProductPurchaseDecision:GetEnumItems()
	return result
end
