-- The other web-backed services on a server: HttpService's secrets and
-- stream clients, AnalyticsService, the distributed counter, DataStore
-- batch reads, friends, bans (only reads and argument errors), chat
-- groups, and the services whose prompts are client-only. Fields under
-- `_` hold live data. Needs a player.
return function(h)
	local HttpService = game:GetService("HttpService")
	local AnalyticsService = game:GetService("AnalyticsService")
	local MemoryStoreService = game:GetService("MemoryStoreService")
	local DataStoreService = game:GetService("DataStoreService")
	local Players = game:GetService("Players")
	local SocialService = game:GetService("SocialService")
	local TextChatService = game:GetService("TextChatService")
	local ExperienceNotificationService = game:GetService("ExperienceNotificationService")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local function kind(f)
		local ok, value = pcall(f)
		return ok and ("ok:" .. typeof(value)) or (tostring(value):gsub("^.-:%d+: ", ""))
	end
	local function keys(t)
		if type(t) ~= "table" then
			return typeof(t)
		end
		local list = {}
		for k, v in t do
			table.insert(list, tostring(k) .. ":" .. typeof(v))
		end
		table.sort(list)
		return table.concat(list, ",")
	end
	local player = Players:GetPlayers()[1]
	while not player do
		Players.PlayerAdded:Wait()
		player = Players:GetPlayers()[1]
	end
	local suffix = HttpService:GenerateGUID(false):sub(1, 8)

	-- HttpService.
	result.secret_missing = err(function()
		return HttpService:GetSecret("rbxnative_missing")
	end)
	result.secret_nil = err(function()
		return HttpService:GetSecret(nil)
	end)
	result.stream_no_url = err(function()
		return HttpService:CreateWebStreamClient(Enum.WebStreamClientType.SSE, {})
	end)
	result.stream_bad_type = err(function()
		return HttpService:CreateWebStreamClient("x", { Url = "https://example.com" })
	end)

	-- AnalyticsService: logging works, the argument errors.
	result.log_custom = err(function()
		AnalyticsService:LogCustomEvent(player, "RbxNativeConformance", 1)
	end)
	result.log_custom_nil = err(function()
		AnalyticsService:LogCustomEvent(nil, "RbxNativeConformance", 1)
	end)
	result.log_economy_bad = err(function()
		AnalyticsService:LogEconomyEvent(player, "x", "Coins", 1, 1, "Shop")
	end)
	result.log_progression = err(function()
		AnalyticsService:LogProgressionStartEvent(player, "Path", 1)
	end)
	result.log_funnel = err(function()
		AnalyticsService:LogOnboardingFunnelStepEvent(player, 1, "Start")
	end)
	result.duration = kind(function()
		return AnalyticsService:GetDurationLoggerTimestamp()
	end)
	result.segments = kind(function()
		return AnalyticsService:GetPlayerSegmentsAsync(player)
	end)

	-- MemoryStore distributed counter.
	local okCounter, counter = pcall(function()
		return MemoryStoreService:GetDistributedCounter("rbxnative_" .. suffix)
	end)
	result.counter = okCounter and typeof(counter) or (tostring(counter):gsub("^.-:%d+: ", ""))
	if okCounter then
		result.counter_class = counter.ClassName
		result.counter_inc = select(2, pcall(counter.IncrementAsync, counter, 5, 60))
		result.counter_inc2 = select(2, pcall(counter.IncrementAsync, counter, -2, 60))
		local okGet, value = pcall(counter.GetAsync, counter)
		result.counter_get = okGet and value or tostring(value)
		result.counter_bad = err(function()
			return counter:IncrementAsync("x", 60)
		end)
	end

	-- DataStore batch reads.
	result.batch_standard = err(function()
		return DataStoreService:GetDataStore("RbxNativeBatch"):BatchGetAsync({ "a" })
	end)
	local store = DataStoreService:GetOrderedDataStore("RbxNativeBatch")
	local okBatch, batch = pcall(function()
		return store:BatchGetAsync({ "missing_a_" .. suffix, "missing_b_" .. suffix })
	end)
	result.batch = okBatch and ("ok:" .. keys(batch)) or (tostring(batch):gsub("^.-:%d+: ", ""))
	result.batch_empty = kind(function()
		return store:BatchGetAsync({})
	end)
	result.batch_string = err(function()
		return store:BatchGetAsync("x")
	end)

	-- Friends.
	result.is_friend_one = kind(function()
		return player:IsFriendsWithAsync(1)
	end)
	result.friends_universe = kind(function()
		return player:GetFriendsInUniverseAsync()
	end)
	result.friends_played = kind(function()
		return player:GetFriendsWhoPlayedAsync()
	end)

	-- Bans: the history reads, a ban without users fails.
	local okHistory, history = pcall(function()
		return Players:GetBanHistoryAsync(player.UserId)
	end)
	result.ban_history = okHistory and history.ClassName or (tostring(history):gsub("^.-:%d+: ", ""))
	if okHistory then
		result.ban_history_page = typeof(history:GetCurrentPage())
		result._ban_history_count = #history:GetCurrentPage()
	end
	result.ban_empty = err(function()
		Players:BanAsync({})
	end)
	result.unban_empty = err(function()
		Players:UnbanAsync({})
	end)

	-- Chat groups and the client-only prompts.
	result.chat_groups = kind(function()
		return TextChatService:GetChatGroupsAsync({ player })
	end)
	result.rate_server = err(function()
		return SocialService:PromptRateExperienceAsync()
	end)
	result.feedback_server = err(function()
		return SocialService:PromptFeedbackSubmissionAsync()
	end)
	result.rsvp_server = err(function()
		return SocialService:PromptRsvpToEventAsync("1")
	end)
	result.notify_can_server = err(function()
		return ExperienceNotificationService:CanPromptOptInAsync()
	end)
	result.notify_prompt_server = err(function()
		ExperienceNotificationService:PromptOptIn()
	end)
	return result
end
