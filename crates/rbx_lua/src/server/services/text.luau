--!nonstrict
-- TextService and the legacy Chat service in Local Run. Filtering has no
-- filter service to ask offline, so text passes through unchanged, but the
-- calls keep Roblox's shape: server only, a TextFilterResult whose
-- Get*Async methods hand the text back, the argument checks. Measuring
-- goes through the host's text layout (the editor's, when it runs one).
local host = ...

local M = {}

local MAX_FILTER = 100000

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

local function text(value)
	if value == nil then
		fail("Argument 1 missing or nil")
	end
	if type(value) == "number" then
		return tostring(value)
	end
	if type(value) ~= "string" then
		fail("Unable to cast " .. typeName(value) .. " to string")
	end
	return value
end

local function serverOnly(method)
	if not host.server() then
		fail(method .. " can only be called from the server")
	end
end

local function familyOf(font)
	if typeof(font) == "EnumItem" then
		local ok, face = pcall(Font.fromEnum, font)
		return if ok then face.Family else ""
	end
	if typeof(font) == "Font" then
		return font.Family
	end
	return ""
end

local results = {} -- TextFilterResult id -> its text

local TEXT = {}

function TEXT.GetTextSize(_, value, size, font, frame)
	value = text(value)
	if type(size) ~= "number" then
		fail("Unable to cast " .. typeName(size) .. " to int")
	end
	if typeof(frame) ~= "Vector2" then
		fail("Unable to cast " .. typeName(frame) .. " to Vector2")
	end
	local w, h = host.text_size(value, size, familyOf(font), frame.X)
	return Vector2.new(math.ceil(w), math.ceil(h))
end

function TEXT.GetTextBoundsAsync(_, params)
	if typeof(params) ~= "Instance" or not params:IsA("GetTextBoundsParams") then
		fail("Unable to cast " .. typeName(params) .. " to GetTextBoundsParams")
	end
	task.wait()
	local w, h = host.text_size(params.Text, params.Size, familyOf(params.Font), params.Width)
	return Vector2.new(math.ceil(w), math.ceil(h))
end

function TEXT.GetTextSizeOffsetAsync(_)
	task.wait()
	return 0
end

local function filter(method, value, fromUserId)
	serverOnly(method)
	value = text(value)
	int64(fromUserId)
	if #value > MAX_FILTER then
		fail(method .. ": the string is too long")
	end
	task.wait()
	local result, id = host.new("TextFilterResult", "TextFilterResult")
	results[id] = value
	return result
end

function TEXT.FilterStringAsync(_, value, fromUserId, _context)
	return filter("TextService:FilterStringAsync()", value, fromUserId)
end

function TEXT.FilterAndTranslateStringAsync(_, value, fromUserId, _locales, _context)
	return filter("TextService:FilterAndTranslateStringAsync()", value, fromUserId)
end

function TEXT.GetFamilyInfoAsync(_, assetId)
	fail("TextService:GetFamilyInfoAsync(): family " .. tostring(assetId) .. " isn't available in Local Run")
end

local RESULT = {}

local function resultText(self)
	task.wait()
	return results[host.id(self)] or ""
end

function RESULT.GetNonChatStringForBroadcastAsync(self)
	return resultText(self)
end

function RESULT.GetNonChatStringForUserAsync(self, toUserId)
	int64(toUserId)
	return resultText(self)
end

function RESULT.GetChatForUserAsync(self, toUserId)
	int64(toUserId)
	return resultText(self)
end

-- The legacy Chat service: bubbles go to `Chatted` (and our bubble chat),
-- callbacks are kept per type, filtering passes through.
local CHAT = {}
local callbacks = {}
local chatId = nil

function CHAT.Chat(self, partOrCharacter, message, color)
	chatId = host.id(self)
	if typeof(partOrCharacter) ~= "Instance" then
		fail("Unable to cast " .. typeName(partOrCharacter) .. " to Instance")
	end
	message = text(message)
	task.defer(host.fire, chatId, "Chatted", partOrCharacter, message, color or Enum.ChatColor.White)
end

function CHAT.RegisterChatCallback(_, callbackType, callback)
	if typeof(callbackType) ~= "EnumItem" then
		fail("Unable to cast " .. typeName(callbackType) .. " to ChatCallbackType")
	end
	callbacks[callbackType.Name] = callback
end

function CHAT.InvokeChatCallback(_, callbackType, arguments)
	if typeof(callbackType) ~= "EnumItem" then
		fail("Unable to cast " .. typeName(callbackType) .. " to ChatCallbackType")
	end
	local callback = callbacks[callbackType.Name]
	if not callback then
		fail("No callback registered for " .. callbackType.Name)
	end
	return callback(table.unpack(if type(arguments) == "table" then arguments else {}))
end

function CHAT.SetBubbleChatSettings(_, settings)
	if type(settings) ~= "table" then
		fail("Unable to cast " .. typeName(settings) .. " to Dictionary")
	end
end

function CHAT.CanUserChatAsync(_, userId)
	int64(userId)
	task.wait()
	return true
end

function CHAT.CanUsersChatAsync(_, from, to)
	int64(from)
	int64(to)
	task.wait()
	return true
end

local function isPlayer(value)
	return typeof(value) == "Instance" and value:IsA("Player")
end

function CHAT.FilterStringAsync(_, value, playerFrom, playerTo)
	serverOnly("Chat:FilterStringAsync()")
	value = text(value)
	if not isPlayer(playerFrom) or not isPlayer(playerTo) then
		fail("Chat:FilterStringAsync(): players must be Player instances")
	end
	task.wait()
	return value
end

function CHAT.FilterStringForBroadcast(_, value, playerFrom)
	serverOnly("Chat:FilterStringForBroadcast()")
	value = text(value)
	if playerFrom == nil then
		fail("Argument 2 missing or nil")
	end
	if not isPlayer(playerFrom) then
		fail("Chat:FilterStringForBroadcast(): playerFrom must be a Player")
	end
	return value
end

local TABLES = { TextService = TEXT, TextFilterResult = RESULT, Chat = CHAT }

function M.member(class, id, key)
	if class == "Chat" and key == "Chatted" then
		return host.signal(id, key), true
	end
	local member = TABLES[class][key]
	return member, member ~= nil
end

return M
