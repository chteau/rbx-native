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
	-- Like any number argument, a numeric string converts ("123").
	if type(value) == "string" and tonumber(value) then
		value = tonumber(value)
	end
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

-- The user's real groups, as the web reports them (cached for offline
-- runs). Test players (ids below 1) and a failed request have none.
local function web(url, context)
	local ok, found = pcall(host.web, url, "GET", nil, context)
	return if ok and type(found) == "table" then found else {}
end

local function roleIn(user, group)
	if user < 1 then
		return nil
	end
	local found = web("https://groups.roblox.com/v2/users/" .. user .. "/groups/roles", "Player:IsInGroupAsync()")
	for _, entry in found.data or {} do
		if entry.group and entry.group.id == group then
			return entry.role
		end
	end
	return nil
end

-- Friends lists need a signed-in session on the web, which a Local Run
-- doesn't have: nobody has friends here.
function PLAYERS.GetFriendsAsync(_, id)
	userId(id)
	return pages()
end

-- A web call on Roblox: it yields, so a script that fetches one inside a
-- RemoteEvent handler sees what arrived alongside the event first.
function PLAYERS.GetHumanoidDescriptionFromUserId(self, id)
	id = userId(id)
	task.wait()
	return self["GetHumanoidDescriptionFromUserId#fetch"](self, id)
end

function PLAYERS.GetHumanoidDescriptionFromUserIdAsync(self, id)
	return self:GetHumanoidDescriptionFromUserId(id)
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

function PLAYER.IsInGroupAsync(self, id)
	return roleIn(self.UserId, userId(id)) ~= nil
end
PLAYER.IsInGroup = PLAYER.IsInGroupAsync

-- Outside a group a player is a guest, rank 0.
function PLAYER.GetRankInGroupAsync(self, id)
	local role = roleIn(self.UserId, userId(id))
	return if role then role.rank else 0
end
PLAYER.GetRankInGroup = PLAYER.GetRankInGroupAsync

function PLAYER.GetRoleInGroupAsync(self, id)
	local role = roleIn(self.UserId, userId(id))
	return if role then role.name else "Guest"
end
PLAYER.GetRoleInGroup = PLAYER.GetRoleInGroupAsync

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

-- Assets load on demand here, so a preload has nothing to wait for: each
-- entry reports Success at once.
local CONTENT = {}

function CONTENT.PreloadAsync(_, list, callback)
	if type(list) ~= "table" then
		fail("Unable to cast to Array")
	end
	if callback ~= nil then
		for _, entry in list do
			callback(if typeof(entry) == "Instance" then "" else tostring(entry), Enum.AssetFetchStatus.Success)
		end
	end
end

function CONTENT.Preload(_, _content) end

function CONTENT.GetAssetFetchStatus(_)
	return Enum.AssetFetchStatus.Success
end

function CONTENT.GetAssetFetchStatusChangedSignal(self)
	return self.Changed
end

function CONTENT.UnregisterDefaultEncryptionKey(_) end
function CONTENT.UnregisterEncryptionKey(_) end

-- ReplicatedFirst: a Local Run shows no default loading screen to remove,
-- and its join snapshot lands whole.
local REPLICATED_FIRST = {}

function REPLICATED_FIRST.RemoveDefaultLoadingScreen(_) end
function REPLICATED_FIRST.SetDefaultLoadingScreen(_, _show) end

function REPLICATED_FIRST.IsDefaultLoadingScreen(_)
	return false
end

function REPLICATED_FIRST.IsFinishedReplicating(_)
	return true
end

-- UserSettings(): the player's own settings, which a game's scripts read
-- (the PlayerModule's ControlModule and camera read UserGameSettings) and
-- may watch. One object per VM, with the defaults a fresh install has.
local USER = {}
local userSettings

function M.UserSettings()
	if not userSettings then
		userSettings = host.new("UserSettings", "UserSettings")
		local gameSettings = host.new("UserGameSettings", "UserGameSettings")
		gameSettings.Parent = userSettings
	end
	return userSettings
end

function USER.GetService(self, name)
	local found = self:FindFirstChildOfClass(name)
	if not found then
		error("'" .. tostring(name) .. "' is not a valid Service name", 2)
	end
	return found
end
USER.FindService = function(self, name)
	return self:FindFirstChildOfClass(name)
end
function USER.IsUserFeatureEnabled(_, _name)
	return false
end

-- UserGameSettings' methods: what the menu would show and the player's own
-- choices, none of which Local Run has a menu for.
local GAME_SETTINGS = {
	GetCameraYInvertValue = function()
		return 1
	end,
	GetOnboardingCompleted = function()
		return false
	end,
	GetTutorialState = function()
		return false
	end,
	InFullScreen = function()
		return false
	end,
	InStudioMode = function()
		return false
	end,
	SetCameraYInvertVisible = function() end,
	SetGamepadCameraSensitivityVisible = function() end,
	SetOnboardingCompleted = function() end,
	SetTutorialState = function() end,
}
local GAME_SETTINGS_SIGNALS = { FullscreenChanged = true, PerformanceStatsVisibleChanged = true, StudioModeChanged = true }

-- PlayerScripts' movement and camera mode registry, which the default
-- PlayerModule and forks of it fill on start: per kind, the modes in order,
-- and a signal each registration fires.
local PLAYER_SCRIPTS = {}
local MODE_KINDS = { "ComputerCameraMovementMode", "ComputerMovementMode", "TouchCameraMovementMode", "TouchMovementMode" }
local registered = {} -- player scripts id -> kind -> modes

local function modes(self, kind)
	local id = host.id(self)
	registered[id] = registered[id] or {}
	registered[id][kind] = registered[id][kind] or {}
	return registered[id][kind]
end

for _, kind in MODE_KINDS do
	PLAYER_SCRIPTS["GetRegistered" .. kind .. "s"] = function(self)
		return table.clone(modes(self, kind))
	end
	PLAYER_SCRIPTS["Register" .. kind] = function(self, mode)
		local list = modes(self, kind)
		if not table.find(list, mode) then
			table.insert(list, mode)
		end
		host.fire(host.id(self), kind .. "Registered", mode)
	end
	PLAYER_SCRIPTS["Clear" .. kind .. "s"] = function(self)
		table.clear(modes(self, kind))
	end
end

local TABLES = {
	Debris = DEBRIS,
	ScriptContext = {},
	UserGameSettings = GAME_SETTINGS,
	PlayerScripts = PLAYER_SCRIPTS,
	UserSettings = USER,
	Players = PLAYERS,
	Player = PLAYER,
	RunService = RUN,
	FriendPages = FRIEND_PAGES,
	ContentProvider = CONTENT,
	ReplicatedFirst = REPLICATED_FIRST,
}

function M.member(class, id, key)
	if class == "FriendPages" and key == "IsFinished" then
		return true, true
	end
	if class == "PlayerScripts" and type(key) == "string" and key:match("Registered$") then
		for _, kind in MODE_KINDS do
			if key == kind .. "Registered" then
				return host.signal(id, key), true
			end
		end
	end
	-- ScriptContext.Error(message, stackTrace, script): fired by the
	-- scheduler for each error a script thread dies of.
	if class == "ScriptContext" and key == "Error" then
		return host.signal(id, key), true
	end
	if class == "UserGameSettings" and GAME_SETTINGS_SIGNALS[key] then
		return host.signal(id, key), true
	end
	if class == "UserSettings" and key == "GameSettings" then
		return M.UserSettings():FindFirstChildOfClass("UserGameSettings"), true
	end
	local found = TABLES[class][key]
	return found, found ~= nil
end

return M
