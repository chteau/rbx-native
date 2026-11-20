-- PolicyService, SocialService, GroupService, UserService and
-- VoiceChatService on a server, for the run's own player: the policy
-- table's fields, what invites and voice say, the argument errors.
-- Needs a player.
return function(h)
	local PolicyService = game:GetService("PolicyService")
	local SocialService = game:GetService("SocialService")
	local GroupService = game:GetService("GroupService")
	local VoiceChatService = game:GetService("VoiceChatService")
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
	local policy = PolicyService:GetPolicyInfoForPlayerAsync(player)
	local keys = {}
	for key in policy do
		table.insert(keys, key)
	end
	table.sort(keys)
	result.policy_keys = table.concat(keys, ",")
	result.policy_china = policy.IsSubjectToChinaPolicies
	result.policy_nil = err(function()
		return PolicyService:GetPolicyInfoForPlayerAsync(nil)
	end)
	result.can_invite = err(function()
		return SocialService:CanSendGameInviteAsync(player)
	end)
	result.can_invite_nil = err(function()
		return SocialService:CanSendGameInviteAsync(nil)
	end)
	result.groups_string = err(function()
		return GroupService:GetGroupsAsync("x")
	end)
	local okVoice, voice = pcall(function()
		return VoiceChatService:IsVoiceEnabledForUserIdAsync(player.UserId)
	end)
	result.voice_enabled = if okVoice then voice else tostring(voice):gsub("^.-:%d+: ", "")
	return result
end
