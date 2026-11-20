--!nonstrict
-- The smaller helpers of Debris, Players, Player and RunService that a
-- Local Run answers by itself: one machine, no friends or groups online,
-- no network delay, no prediction. Members not listed here fall through
-- to the engine's own.
local host = ...

local M = {}

local function fail(message)
	error(message, 0)
end

local function userId(value)
	if type(value) ~= "number" then
		fail("Unable to cast " .. (value == nil and "nil" or typeof(value)) .. " to int64")
	end
	return value
end

-- A friends listing: one (empty) page, finished.
local function pages()
	return (host.new("FriendPages", "FriendPages"))
end

local FRIEND_PAGES = {}

function FRIEND_PAGES.GetCurrentPage(_)
	return {}
end

function FRIEND_PAGES.AdvanceToNextPageAsync(_)
	fail("No pages to advance to")
end

local DEBRIS = {}

function DEBRIS.AddItem(_, item, lifetime)
	if typeof(item) ~= "Instance" then
		fail("Argument 1 missing or nil")
	end
	lifetime = tonumber(lifetime) or 10
	task.delay(math.max(lifetime, 0), function()
		pcall(item.Destroy, item)
	end)
end

local PLAYERS = {}

local function playerById(players, id)
	for _, p in players:GetPlayers() do
		if p.UserId == id then
			return p
		end
	end
end

function PLAYERS.GetNameFromUserIdAsync(self, id)
	id = userId(id)
	local p = playerById(self, id)
	if p then
		return p.Name
	end
	if id < 0 then
		return "Player" .. -id
	end
	return host.user_name(id)
end

function PLAYERS.GetUserIdFromNameAsync(self, name)
	if type(name) ~= "string" then
		fail("Unable to cast " .. typeof(name) .. " to string")
	end
	for _, p in self:GetPlayers() do
		if string.lower(p.Name) == string.lower(name) then
			return p.UserId
		end
	end
	return host.user_id(name)
end

function PLAYERS.GetFriendsAsync(_, id)
	userId(id)
	return pages()
end

function PLAYERS.GetHumanoidDescriptionFromUserIdAsync(self, id)
	return self:GetHumanoidDescriptionFromUserId(userId(id))
end

function PLAYERS.CreateHumanoidModelFromDescriptionAsync(self, description, rigType, assetTypeVerification)
	return self:CreateHumanoidModelFromDescription(description, rigType, assetTypeVerification)
end

function PLAYERS.CreateHumanoidModelFromUserIdAsync(self, id)
	return self:CreateHumanoidModelFromDescription(self:GetHumanoidDescriptionFromUserId(userId(id)))
end

function PLAYERS.GetCharacterAppearanceInfoAsync(_, id)
	userId(id)
	return { assets = {}, bodyColors = {}, scales = {}, playerAvatarType = "R15", emotes = {} }
end

function PLAYERS.GetUserThumbnailAsync(_, id)
	return "rbxthumb://type=AvatarHeadShot&id=" .. userId(id) .. "&w=150&h=150", true
end

local PLAYER = {}

local function rootOf(player)
	local character = player.Character
	return character and (character:FindFirstChild("Head") or character.PrimaryPart)
end

function PLAYER.DistanceFromCharacter(self, point)
	if typeof(point) ~= "Vector3" then
		fail("Unable to cast " .. typeof(point) .. " to Vector3")
	end
	local root = rootOf(self)
	return root and (root.Position - point).Magnitude or 0
end

-- What the player joined with: a teleport's data and source place, when
-- the run started from one (see teleport.luau).
function PLAYER.GetJoinData(self)
	local data = { GameJoinContext = {}, LaunchData = "" }
	local arrival = host.arrival(self.UserId)
	if arrival then
		data.SourcePlaceId = arrival.source
		data.SourceGameId = ""
		if arrival.data ~= nil then
			data.TeleportData = host.decode(arrival.data)
		end
	end
	return data
end

function PLAYER.GetNetworkPing(_)
	return 0
end

function PLAYER.HasAppearanceLoaded(self)
	return self.Character ~= nil
end

function PLAYER.IsVerified(_)
	return false
end

function PLAYER.Move(self, direction, relativeToCamera)
	local character = self.Character
	local humanoid = character and character:FindFirstChildOfClass("Humanoid")
	if humanoid then
		humanoid:Move(direction, relativeToCamera)
	end
end

function PLAYER.AddReplicationFocus(_, part) end
function PLAYER.RemoveReplicationFocus(_, part) end

function PLAYER.RequestStreamAroundAsync(_, position)
	if typeof(position) ~= "Vector3" then
		fail("Unable to cast " .. typeof(position) .. " to Vector3")
	end
end

function PLAYER.LoadCharacterAsync(self)
	self:LoadCharacter()
end

function PLAYER.IsFriendsWithAsync(_, id)
	userId(id)
	return false
end

function PLAYER.IsInGroupAsync(_, id)
	userId(id)
	return false
end

function PLAYER.GetFriendsOnlineAsync(_)
	return {}
end

function PLAYER.GetFriendsInServerAsync(_)
	return {}
end

local RUN = {}

function RUN.IsResimulating(_)
	return false
end

function RUN.GetPredictionStatus(_)
	return Enum.PredictionStatus.None
end

function RUN.SetPredictionMode(_) end

function RUN.BindToSimulation(self, callback)
	if type(callback) ~= "function" then
		fail("Argument 1 missing or nil")
	end
	return self.PreSimulation:Connect(callback)
end

local TABLES = { Debris = DEBRIS, Players = PLAYERS, Player = PLAYER, RunService = RUN, FriendPages = FRIEND_PAGES }

function M.member(class, _, key)
	if class == "FriendPages" and key == "IsFinished" then
		return true, true
	end
	local found = TABLES[class][key]
	return found, found ~= nil
end

return M
