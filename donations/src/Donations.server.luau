--!strict
-- Takes the Robux. Every product in Config.products is a donation: nothing
-- unlocks, the buyer gets a thank-you and a spot on the donor board. The
-- money goes to the group that owns this place, and the group pays the
-- maintainers out by percentage (donations/README.md).
local DataStoreService = game:GetService("DataStoreService")
local MarketplaceService = game:GetService("MarketplaceService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Config = require(ReplicatedStorage:WaitForChild("Config"))

local remotes = Instance.new("Folder")
remotes.Name = "Donations"
local thanked = Instance.new("RemoteEvent")
thanked.Name = "Thanked"
thanked.Parent = remotes
local board = Instance.new("StringValue")
board.Name = "Board"
board.Value = "[]"
board.Parent = remotes
remotes.Parent = ReplicatedStorage

-- Per donor: their total and the last receipts counted in it, and the same
-- totals sorted for the board. Unpublished (a Studio playtest) Roblox has no
-- DataStores and throws here, so the screen still comes up, just without
-- taking donations.
local storesOk, donors, totals = pcall(function()
	return DataStoreService:GetDataStore("Donors"), DataStoreService:GetOrderedDataStore("DonorTotals")
end)
if not storesOk then
	warn("donations off, no DataStores here:", donors)
end
local KEEP_RECEIPTS = 50

local donation: { [number]: boolean } = {}
for _, id in Config.products do
	donation[id] = true
end

type Donor = { total: number, receipts: { string } }

-- Roblox can hand the same receipt over more than once (a server closing
-- mid-purchase, a retry), so the donor's record keeps the receipts it has
-- counted and skips one it has seen. The purchase is only granted once that
-- record is saved; the board copy is a plain overwrite, safe to repeat.
MarketplaceService.ProcessReceipt = function(receipt)
	if not storesOk or not donation[receipt.ProductId] then
		return Enum.ProductPurchaseDecision.NotProcessedYet
	end
	local key = tostring(receipt.PlayerId)
	local total = 0
	local ok, err = pcall(function()
		donors:UpdateAsync(key, function(saved: Donor?): Donor?
			local donor: Donor = saved or { total = 0, receipts = {} }
			if table.find(donor.receipts, receipt.PurchaseId) then
				total = donor.total
				return nil
			end
			donor.total += receipt.CurrencySpent
			table.insert(donor.receipts, receipt.PurchaseId)
			if #donor.receipts > KEEP_RECEIPTS then
				table.remove(donor.receipts, 1)
			end
			total = donor.total
			return donor
		end)
	end)
	if not ok then
		warn("donation not saved yet, Roblox will retry:", err)
		return Enum.ProductPurchaseDecision.NotProcessedYet
	end
	pcall(function()
		totals:SetAsync(key, total)
	end)
	local player = Players:GetPlayerByUserId(receipt.PlayerId)
	if player then
		thanked:FireClient(player, receipt.CurrencySpent)
	end
	return Enum.ProductPurchaseDecision.PurchaseGranted
end

local names: { [number]: string } = {}
local function nameOf(userId: number): string
	local known = names[userId]
	if known then
		return known
	end
	local ok, name = pcall(Players.GetNameFromUserIdAsync, Players, userId)
	local found = if ok and type(name) == "string" then name else "Someone"
	names[userId] = found
	return found
end

local function refreshBoard()
	if not storesOk then
		return
	end
	local ok, pages = pcall(function()
		return totals:GetSortedAsync(false, Config.boardSize)
	end)
	if not ok then
		return
	end
	local rows = {}
	for _, entry in pages:GetCurrentPage() do
		local userId = tonumber(entry.key)
		if userId then
			table.insert(rows, { name = nameOf(userId), robux = entry.value })
		end
	end
	board.Value = game:GetService("HttpService"):JSONEncode(rows)
end

task.spawn(function()
	while true do
		refreshBoard()
		task.wait(Config.boardEvery)
	end
end)
