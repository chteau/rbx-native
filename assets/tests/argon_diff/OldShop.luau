-- Legacy shop, replaced by ShopClient + ShopUI.
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")

local player = Players.LocalPlayer
local gui = player:WaitForChild("PlayerGui"):WaitForChild("OldShopGui")
local frame = gui.Main
local buyRemote = ReplicatedStorage:WaitForChild("BuyItem")

local ITEMS = {
	{ id = "sword", name = "Sword", price = 100 },
	{ id = "shield", name = "Shield", price = 150 },
	{ id = "boots", name = "Speed Boots", price = 250 },
}

local open = false

local function setOpen(value: boolean)
	open = value
	local goal = { Position = UDim2.fromScale(0.5, value and 0.5 or 1.5) }
	TweenService:Create(frame, TweenInfo.new(0.25), goal):Play()
end

for index, item in ITEMS do
	local button = frame.Template:Clone()
	button.Name = item.id
	button.Title.Text = item.name
	button.Price.Text = tostring(item.price) .. " coins"
	button.LayoutOrder = index
	button.Visible = true
	button.Parent = frame.List
	button.Activated:Connect(function()
		buyRemote:FireServer(item.id)
	end)
end
