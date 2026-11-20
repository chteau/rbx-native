--!nonstrict
-- The social and account services in Local Run: GroupService and
-- UserService read Roblox's public web APIs (kept for offline runs; the
-- run's own players, user ids below 1, are answered here), SocialService
-- has no friends to invite, PolicyService gives an adult's policy in a
-- country without extra rules, VoiceChatService reports voice off, and
-- AvatarEditorService's and GroupService's prompts open the editor's own
-- dialog, nothing saved to the account.
local host = ...

local M = {}

local function fail(message)
	error(message, 0)
end

local function typeName(value)
	if value == nil then
		return "nil"
	end
	if type(value) == "number" then
		return "double"
	end
	return typeof(value)
end

local function int64(value)
	if type(value) ~= "number" then
		fail("Unable to cast " .. typeName(value) .. " to int64")
	end
	return math.floor(value)
end

local function isPlayer(value)
	return typeof(value) == "Instance" and value:IsA("Player")
end

local function clientOnly(method)
	if host.server() then
		fail(method .. " can only be called from the client")
	end
end

local function localPlayer()
	return game:GetService("Players").LocalPlayer
end

-- An empty, finished listing.
local function emptyPages()
	return (host.new("StandardPages", "StandardPages"))
end

local PAGES = {}

function PAGES.GetCurrentPage(_)
	return {}
end

function PAGES.AdvanceToNextPageAsync(_)
	fail("No pages to advance to")
end

-- Dialog answers: kind -> what the dialog said, read by the waiting call.
local answers = {}

local function promptAndWait(kind, player, id)
	answers[kind] = nil
	host.prompt(kind, player, id or 0)
	while answers[kind] == nil do
		task.wait()
	end
	local answer = answers[kind]
	answers[kind] = nil
	return answer
end

local GROUPS = {}

function GROUPS.GetGroupInfoAsync(_, groupId)
	groupId = int64(groupId)
	local found = host.web("https://groups.roblox.com/v1/groups/" .. groupId, "GET", nil, "GroupService:GetGroupInfoAsync()")
	local roles = host.web("https://groups.roblox.com/v1/groups/" .. groupId .. "/roles", "GET", nil, "GroupService:GetGroupInfoAsync()")
	local list = {}
	for _, role in roles.roles or {} do
		table.insert(list, { Name = role.name, Rank = role.rank })
	end
	local owner = found.owner
	return {
		Name = found.name,
		Id = found.id,
		Owner = owner and { Name = owner.username, Id = owner.userId } or nil,
		Description = found.description or "",
		EmblemUrl = "",
		Roles = list,
	}
end

function GROUPS.GetGroupsAsync(_, userId)
	userId = int64(userId)
	if userId < 1 then
		task.wait()
		return {}
	end
	local found = host.web(
		"https://groups.roblox.com/v2/users/" .. userId .. "/groups/roles",
		"GET",
		nil,
		"GroupService:GetGroupsAsync()"
	)
	local list = {}
	for _, entry in found.data or {} do
		table.insert(list, {
			Name = entry.group.name,
			Id = entry.group.id,
			EmblemUrl = "",
			EmblemId = 0,
			Rank = entry.role.rank,
			Role = entry.role.name,
			IsPrimary = entry.isPrimaryGroup == true,
			IsInClan = false,
		})
	end
	return list
end

function GROUPS.GetRolesInGroupAsync(self, userId, groupId)
	groupId = int64(groupId)
	for _, group in GROUPS.GetGroupsAsync(self, userId) do
		if group.Id == groupId then
			return { { Name = group.Role, Rank = group.Rank } }
		end
	end
	return {}
end

function GROUPS.GetAlliesAsync(_, groupId)
	int64(groupId)
	return emptyPages()
end
GROUPS.GetEnemiesAsync = GROUPS.GetAlliesAsync

function GROUPS.PromptJoinAsync(_, groupId)
	clientOnly("GroupService:PromptJoinAsync()")
	groupId = int64(groupId)
	local joined = promptAndWait("GroupService.PromptJoinAsync", localPlayer(), groupId)
	return if joined then Enum.GroupMembershipStatus.Joined else Enum.GroupMembershipStatus.None
end

local USERS = {}

local function localInfo(userId)
	local player = game:GetService("Players"):GetPlayerByUserId(userId)
	if player then
		return { Id = userId, Username = player.Name, DisplayName = player.DisplayName, HasVerifiedBadge = false }
	end
end

function USERS.GetUserInfosByUserIdsAsync(_, userIds)
	if type(userIds) ~= "table" then
		fail("Unable to cast " .. typeName(userIds) .. " to Array")
	end
	if #userIds > 200 then
		fail("UserService:GetUserInfosByUserIdsAsync(): too many user ids, at most 200")
	end
	local out, remote = {}, {}
	for _, userId in userIds do
		userId = int64(userId)
		local here = localInfo(userId)
		if here then
			table.insert(out, here)
		elseif userId > 0 then
			table.insert(remote, userId)
		end
	end
	if #remote > 0 then
		local body = host.encode({ userIds = remote, excludeBannedUsers = false })
		local found = host.web("https://users.roblox.com/v1/users", "POST", body, "UserService:GetUserInfosByUserIdsAsync()")
		for _, user in found.data or {} do
			table.insert(out, {
				Id = user.id,
				Username = user.name,
				DisplayName = user.displayName,
				HasVerifiedBadge = user.hasVerifiedBadge == true,
			})
		end
	else
		task.wait()
	end
	return out
end

function USERS.GetUserFromGlobalUserIdAsync(_, userId)
	return int64(userId)
end

local SOCIAL = {}

function SOCIAL.CanSendGameInviteAsync(_, player)
	if not isPlayer(player) then
		fail("SocialService:CanSendGameInviteAsync(): player must be a Player")
	end
	task.wait()
	return false
end

function SOCIAL.CanSendCallInviteAsync(_, player)
	if not isPlayer(player) then
		fail("SocialService:CanSendCallInviteAsync(): player must be a Player")
	end
	task.wait()
	return false
end

local socialId = nil

function SOCIAL.PromptGameInvite(self, player)
	socialId = host.id(self)
	if not isPlayer(player) then
		fail("SocialService:PromptGameInvite(): player must be a Player")
	end
	host.notify(player, "Invite friends", "Local Run has no friends to invite.")
	task.defer(host.fire, socialId, "GameInvitePromptClosed", player, {})
end

function SOCIAL.PromptPhoneBook(self, player)
	socialId = host.id(self)
	if not isPlayer(player) then
		fail("SocialService:PromptPhoneBook(): player must be a Player")
	end
	host.notify(player, "Call friends", "Local Run has no friends to call.")
	task.defer(host.fire, socialId, "PhoneBookPromptClosed", player)
end

function SOCIAL.PromptLinkSharingAsync(_, player)
	if not isPlayer(player) then
		fail("SocialService:PromptLinkSharingAsync(): player must be a Player")
	end
	host.notify(player, "Share", "Local Run has no links to share.")
	task.wait()
	return Enum.PromptLinkSharingResult.PlayerLeft
end

function SOCIAL.GetPlayersByPartyId(_)
	return {}
end

function SOCIAL.GetPartyAsync(_)
	task.wait()
	return {}
end

function SOCIAL.GetEventRsvpStatusAsync(_)
	task.wait()
	return Enum.RsvpStatus.None
end

function SOCIAL.GetUpcomingExperienceEventsAsync(_)
	task.wait()
	return {}
end

function SOCIAL.ShowSelfView(_) end
function SOCIAL.HideSelfView(_) end

local POLICY = {}

function POLICY.GetPolicyInfoForPlayerAsync(_, player)
	if not isPlayer(player) then
		fail("PolicyService:GetPolicyInfoForPlayerAsync(): player must be a Player")
	end
	task.wait()
	return {
		AreAdsAllowed = true,
		ArePaidRandomItemsRestricted = false,
		AllowedExternalLinkReferences = { "Discord", "Facebook", "Twitch", "YouTube", "X", "GitHub", "Guilded" },
		IsContentSharingAllowed = true,
		IsEligibleToPurchaseCommerceProduct = true,
		IsEligibleToPurchaseSubscription = true,
		IsPaidItemTradingAllowed = true,
		IsSubjectToChinaPolicies = false,
	}
end

function POLICY.CanViewBrandProjectAsync(_, player)
	if not isPlayer(player) then
		fail("PolicyService:CanViewBrandProjectAsync(): player must be a Player")
	end
	task.wait()
	return true
end

local VOICE = {}

function VOICE.IsVoiceEnabledForUserIdAsync(_, userId)
	int64(userId)
	task.wait()
	return false
end

function VOICE.GetChatGroupsAsync(_)
	task.wait()
	return {}
end

-- AvatarEditorService: each prompt asks the editor's dialog and answers
-- through its Completed event; a confirmed save changes nothing on the
-- account.
local AVATAR = {}
local avatarId = nil
local saving = {} -- prompt kind -> the extra values its Completed event carries

local ACCESSORY = {
	HairAccessory = "Hair",
	FaceAccessory = "Face",
	NeckAccessory = "Neck",
	ShoulderAccessory = "Shoulder",
	FrontAccessory = "Front",
	BackAccessory = "Back",
	WaistAccessory = "Waist",
	Hat = "Hat",
	TShirtAccessory = "TShirt",
	ShirtAccessory = "Shirt",
	PantsAccessory = "Pants",
	JacketAccessory = "Jacket",
	SweaterAccessory = "Sweater",
	ShortsAccessory = "Shorts",
	LeftShoeAccessory = "LeftShoe",
	RightShoeAccessory = "RightShoe",
	DressSkirtAccessory = "DressSkirt",
	EyebrowAccessory = "Eyebrow",
	EyelashAccessory = "Eyelash",
}

function AVATAR.GetAccessoryType(_, assetType)
	if typeof(assetType) ~= "EnumItem" then
		fail("Unable to cast " .. typeName(assetType) .. " to AvatarAssetType")
	end
	local name = ACCESSORY[assetType.Name]
	return if name then Enum.AccessoryType[name] else Enum.AccessoryType.Unknown
end

local function avatarPrompt(method, extra)
	return function(self, ...)
		avatarId = host.id(self)
		clientOnly("AvatarEditorService:" .. method .. "()")
		local kind = "AvatarEditorService." .. method
		saving[kind] = extra and extra(...) or {}
		local id = select(1, ...)
		host.prompt(kind, localPlayer(), if type(id) == "number" then id else 0)
	end
end

AVATAR.PromptSaveAvatar = avatarPrompt("PromptSaveAvatar", function(description)
	return { description }
end)
AVATAR.PromptCreateOutfit = avatarPrompt("PromptCreateOutfit", function()
	return { nil }
end)
AVATAR.PromptDeleteOutfit = avatarPrompt("PromptDeleteOutfit")
AVATAR.PromptRenameOutfit = avatarPrompt("PromptRenameOutfit")
AVATAR.PromptUpdateOutfit = avatarPrompt("PromptUpdateOutfit")
AVATAR.PromptSetFavorite = avatarPrompt("PromptSetFavorite")
AVATAR.PromptAllowInventoryReadAccess = avatarPrompt("PromptAllowInventoryReadAccess")

function AVATAR.GetFavoriteAsync(_)
	task.wait()
	return false
end

local AVATAR_EVENTS = {
	PromptSaveAvatarCompleted = true,
	PromptCreateOutfitCompleted = true,
	PromptDeleteOutfitCompleted = true,
	PromptRenameOutfitCompleted = true,
	PromptUpdateOutfitCompleted = true,
	PromptSetFavoriteCompleted = true,
	PromptAllowInventoryReadAccessCompleted = true,
}

local SOCIAL_EVENTS = {
	GameInvitePromptClosed = true,
	PhoneBookPromptClosed = true,
	CallInviteStateChanged = true,
	ShareSheetClosed = true,
}

local TABLES = {
	GroupService = GROUPS,
	UserService = USERS,
	SocialService = SOCIAL,
	PolicyService = POLICY,
	VoiceChatService = VOICE,
	AvatarEditorService = AVATAR,
	StandardPages = PAGES,
}

function M.member(class, id, key)
	if (class == "AvatarEditorService" and AVATAR_EVENTS[key]) or (class == "SocialService" and SOCIAL_EVENTS[key]) then
		return host.signal(id, key), true
	end
	local members = TABLES[class]
	local member = members and members[key]
	return member, member ~= nil
end

-- The editor's dialog closed on one of this module's prompts.
function M.finished(kind, _, _, confirmed)
	if kind == "GroupService.PromptJoinAsync" then
		answers[kind] = confirmed
		return
	end
	if not avatarId then
		return
	end
	local event = string.gsub(kind, "^AvatarEditorService%.", "") .. "Completed"
	local result = if confirmed then Enum.AvatarPromptResult.Success else Enum.AvatarPromptResult.PermissionDenied
	local extra = saving[kind] or {}
	saving[kind] = nil
	host.fire(avatarId, event, result, table.unpack(extra))
end

return M
