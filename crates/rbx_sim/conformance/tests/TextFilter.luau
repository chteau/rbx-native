-- TextService and the legacy Chat service on a server: filtering's result
-- object and argument errors, and the shapes GetTextSize gives. Needs a
-- player.
return function(h)
	local TextService = game:GetService("TextService")
	local Chat = game:GetService("Chat")
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
	local filtered = TextService:FilterStringAsync("hello world", player.UserId)
	result.result_class = filtered.ClassName
	result.broadcast = filtered:GetNonChatStringForBroadcastAsync()
	result.for_user = filtered:GetNonChatStringForUserAsync(player.UserId)
	result.filter_nil = err(function()
		return TextService:FilterStringAsync(nil, player.UserId)
	end)
	result.filter_bad_user = err(function()
		return TextService:FilterStringAsync("hi", "x")
	end)
	result.chat_filter = Chat:FilterStringForBroadcast("hello", player)
	result.chat_filter_nil_player = err(function()
		return Chat:FilterStringForBroadcast("hello", nil)
	end)
	result.can_chat = Chat:CanUserChatAsync(player.UserId)
	local empty = TextService:GetTextSize("", 20, Enum.Font.SourceSans, Vector2.new(1000, 1000))
	result.size_empty_y = empty.Y
	local one = TextService:GetTextSize("Hello", 20, Enum.Font.SourceSans, Vector2.new(1000, 1000))
	local two = TextService:GetTextSize("Hello\nHello", 20, Enum.Font.SourceSans, Vector2.new(1000, 1000))
	result.size_lines = two.Y / one.Y
	result.size_same_width = two.X == one.X
	result.size_bad_frame = err(function()
		return TextService:GetTextSize("x", 20, Enum.Font.SourceSans, 5)
	end)
	return result
end
