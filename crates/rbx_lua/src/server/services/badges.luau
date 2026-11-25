--!nonstrict
-- BadgeService in Local Run: badges are awarded into the place's local
-- ledger (the ownership table purchases use), never on Roblox. A badge's
-- name and icon come from Roblox's public badges API, kept for offline
-- runs; offline with nothing kept, an award still lands under a stand-in
-- name and GetBadgeInfoAsync fails. The player hears of an award through
-- the core GUI's toast.
local host = ...

local M = {}

local MAX_CHECK = 10

local function fail(message)
	error(message, 0)
end

local function int64(value)
	-- Like any number argument, a numeric string converts ("123").
	if type(value) == "string" and tonumber(value) then
		value = tonumber(value)
	end
	if type(value) ~= "number" then
		fail("Unable to cast " .. (value == nil and "nil" or typeof(value)) .. " to int64")
	end
	return math.floor(value)
end

local function user(value)
	if type(value) ~= "number" then
		fail("Unable to cast " .. (if type(value) == "table" then "table" else typeof(value)) .. " to User")
	end
	return math.floor(value)
end

local function badge(value, position)
	if value == nil then
		fail("Argument " .. position .. " missing or nil")
	end
	return int64(value)
end

local function info(badgeId)
	local found = host.web("https://badges.roblox.com/v1/badges/" .. badgeId, "GET", nil, "BadgeService:GetBadgeInfoAsync()")
	return {
		Name = found.displayName or found.name,
		Description = found.displayDescription or found.description or "",
		IconImageId = found.displayIconImageId or found.iconImageId or 0,
		IsEnabled = found.enabled ~= false,
	}
end

local SERVICE = {}

function SERVICE.GetBadgeInfoAsync(_, badgeId)
	badgeId = int64(badgeId)
	if badgeId <= 0 then
		fail("BadgeService:GetBadgeInfoAsync(): badge id must be positive")
	end
	return info(badgeId)
end

function SERVICE.AwardBadgeAsync(_, userId, badgeId)
	if not host.server() then
		fail("BadgeService:AwardBadgeAsync() can only be called from the server")
	end
	userId = user(userId)
	badgeId = badge(badgeId, 2)
	local player = game:GetService("Players"):GetPlayerByUserId(userId)
	if not player then
		fail("BadgeService:AwardBadgeAsync(): the player is not in the game")
	end
	local ok, found = pcall(info, badgeId)
	if ok and not found.IsEnabled then
		fail("BadgeService:AwardBadgeAsync(): the badge is disabled")
	end
	task.wait()
	if host.owns(userId, "badge:" .. badgeId) then
		return false
	end
	host.grant(userId, "badge:" .. badgeId)
	local name = if ok then found.Name else "Badge " .. badgeId
	host.notify(player, "Badge Awarded", 'You won the "' .. name .. '" badge!')
	return true
end
SERVICE.AwardBadge = SERVICE.AwardBadgeAsync

function SERVICE.UserHasBadgeAsync(_, userId, badgeId)
	userId = user(userId)
	badgeId = badge(badgeId, 2)
	task.wait()
	return host.owns(userId, "badge:" .. badgeId) or host.owns_live(userId, "badge", badgeId)
end
SERVICE.UserHasBadge = SERVICE.UserHasBadgeAsync

function SERVICE.CheckUserBadgesAsync(_, userId, badgeIds)
	userId = user(userId)
	if type(badgeIds) ~= "table" then
		fail("Unable to cast " .. (if type(badgeIds) == "number" then "double" else typeof(badgeIds)) .. " to Array")
	end
	if #badgeIds > MAX_CHECK then
		fail("CheckUserBadgesAsync: Cannot query more than " .. MAX_CHECK .. " badges.")
	end
	task.wait()
	local owned = {}
	for _, badgeId in badgeIds do
		if host.owns(userId, "badge:" .. int64(badgeId)) then
			table.insert(owned, badgeId)
		end
	end
	return owned
end
SERVICE.GetUserBadgesAsync = SERVICE.CheckUserBadgesAsync

function M.member(_, _, key)
	return SERVICE[key], SERVICE[key] ~= nil
end

return M
