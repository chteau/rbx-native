-- TeleportService on a server without teleporting anyone: the argument
-- errors, a reserved server's code and id, TeleportOptions' data, where a
-- player in the game is. Needs a player.
return function(h)
	local TeleportService = game:GetService("TeleportService")
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
	result.async_string_place = err(function()
		TeleportService:TeleportAsync("x", { player })
	end)
	result.async_players_nil = err(function()
		TeleportService:TeleportAsync(game.PlaceId, nil)
	end)
	result.async_players_empty = err(function()
		TeleportService:TeleportAsync(game.PlaceId, {})
	end)
	result.async_bad_options = err(function()
		TeleportService:TeleportAsync(game.PlaceId, { player }, 5)
	end)
	result.teleport_nil_player = err(function()
		TeleportService:Teleport(game.PlaceId, nil)
	end)
	result.private_bad_code = err(function()
		TeleportService:TeleportToPrivateServer(game.PlaceId, "nope", { player })
	end)
	local code, id = TeleportService:ReserveServer(game.PlaceId)
	result.reserve_types = typeof(code) .. " " .. typeof(id)
	result.reserve_distinct = code ~= TeleportService:ReserveServer(game.PlaceId)
	local options = Instance.new("TeleportOptions")
	options:SetTeleportData({ level = 3, name = "x" })
	local data = options:GetTeleportData()
	result.options_data = data and (data.level .. data.name) or "nil"
	result.options_function = err(function()
		options:SetTeleportData({ f = print })
	end)
	local here, message, placeId, jobId = TeleportService:GetPlayerPlaceInstanceAsync(player.UserId)
	result.instance_here = here
	result.instance_place = placeId == game.PlaceId
	result.instance_job = jobId == game.JobId
	result.instance_message = message
	result.local_data_server = err(function()
		return TeleportService:GetLocalPlayerTeleportData()
	end)
	result.join_data_keys = (function()
		local keys = {}
		for key in player:GetJoinData() do
			table.insert(keys, key)
		end
		table.sort(keys)
		return table.concat(keys, ",")
	end)()
	return result
end
