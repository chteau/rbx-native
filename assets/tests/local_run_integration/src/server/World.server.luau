-- The floor, a spawn and two coins; a coin goes to whoever touches it.
local Players = game:GetService("Players")

local floor = Instance.new("Part")
floor.Name = "Baseplate"
floor.Anchored = true
floor.Size = Vector3.new(256, 4, 256)
floor.Position = Vector3.new(0, -2, 0)
floor.Parent = workspace

local spawn = Instance.new("SpawnLocation")
spawn.Anchored = true
spawn.Neutral = true
spawn.Size = Vector3.new(8, 1, 8)
spawn.Position = Vector3.new(0, 0.5, 0)
spawn.Parent = workspace

Players.PlayerAdded:Connect(function(player)
	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Parent = stats
	stats.Parent = player
end)

local folder = Instance.new("Folder")
folder.Name = "Coins"
folder.Parent = workspace

for name, position in { CoinA = Vector3.new(0, 3, 24), CoinB = Vector3.new(0, 3, -24) } do
	local coin = Instance.new("Part")
	coin.Name = name
	coin.Anchored = true
	coin.CanCollide = false
	coin.Size = Vector3.new(2, 2, 2)
	coin.Position = position
	coin.Parent = folder
	coin.Touched:Connect(function(hit)
		local player = Players:GetPlayerFromCharacter(hit.Parent)
		if player and coin.Parent then
			coin.Parent = nil
			player.leaderstats.Coins.Value += 1
		end
	end)
end
