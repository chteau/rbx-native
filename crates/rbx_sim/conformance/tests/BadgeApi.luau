-- BadgeService on a server without awarding anything: the argument errors
-- and what a check says for a badge the player can't have. Needs a player.
return function(h)
	local BadgeService = game:GetService("BadgeService")
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
	result.has_string_user = err(function()
		return BadgeService:UserHasBadgeAsync("x", 1)
	end)
	result.has_nil_badge = err(function()
		return BadgeService:UserHasBadgeAsync(player.UserId, nil)
	end)
	result.has_unknown = BadgeService:UserHasBadgeAsync(player.UserId, 1)
	result.check_not_table = err(function()
		return BadgeService:CheckUserBadgesAsync(player.UserId, 5)
	end)
	result.check_too_many = err(function()
		return BadgeService:CheckUserBadgesAsync(player.UserId, { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11 })
	end)
	result.check_none = #BadgeService:CheckUserBadgesAsync(player.UserId, { 1, 2 })
	result.info_string = err(function()
		return BadgeService:GetBadgeInfoAsync("x")
	end)
	result.award_string_user = err(function()
		return BadgeService:AwardBadgeAsync("x", 1)
	end)
	return result
end
