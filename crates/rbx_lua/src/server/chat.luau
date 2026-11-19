--!nonstrict
-- TextChatService in Local Run: our own, written from the creator docs and
-- what the TextChat golden measured. A client's TextChannel:SendAsync
-- builds a TextChatMessage (Status Sending), runs OnIncomingMessage
-- (TextChatService's, then the channel's), fires SendingMessage and only
-- then returns, while the text goes to the server. The server asks the
-- channel's ShouldDeliverCallback once per TextSource in it (never firing
-- MessageReceived itself) and hands the message to each player it lets
-- through; their client runs OnIncomingMessage again (Status Success) and
-- fires TextChatService.MessageReceived, then the channel's. Nothing
-- filters text: there is no filtering service offline.
local host = ...

local M = {}

local Players = game:GetService("Players")
local TextChatService = game:GetService("TextChatService")

local callbacks = {} -- instance id -> callback name -> function
local CALLBACKS = {
	TextChatService = { OnIncomingMessage = true, OnBubbleAdded = true, OnChatWindowAdded = true },
	TextChannel = { OnIncomingMessage = true, ShouldDeliverCallback = true },
}
-- What a system message shows as (TextChat `client_system_log`).
local SYSTEM_STYLE = "<font color='#d4d4d4' weight='600' size=\"12\">%s</font>"

local function callback(owner, name)
	local mine = callbacks[host.id(owner)]
	return mine and mine[name]
end

local function sourceOf(channel, userId)
	for _, child in channel:GetChildren() do
		if child:IsA("TextSource") and child.UserId == userId then
			return child
		end
	end
	return nil
end

local function message(channel, text, status, source, id)
	local m = host.new("TextChatMessage")
	host.write(m, "Text", text)
	host.write(m, "Status", Enum.TextChatMessageStatus[status])
	host.write(m, "TextChannel", channel)
	host.write(m, "TextSource", source)
	host.write(m, "MessageId", id)
	local player = source and Players:GetPlayerByUserId(source.UserId)
	host.write(m, "PrefixText", player and (player.DisplayName .. ":") or "")
	return m
end

-- OnIncomingMessage: TextChatService's, then the channel's. What a
-- callback returns (TextChatMessageProperties) restyles the message.
local function incoming(channel, m)
	for _, owner in { TextChatService, channel } do
		local f = callback(owner, "OnIncomingMessage")
		if f then
			local ok, props = pcall(f, m)
			if not ok then
				warn(props)
			elseif typeof(props) == "Instance" and props:IsA("TextChatMessageProperties") then
				if props.Text ~= "" then
					host.write(m, "Text", props.Text)
				end
				if props.PrefixText ~= "" then
					host.write(m, "PrefixText", props.PrefixText)
				end
			end
		end
	end
end

local function sendAsync(channel, text)
	if not host.client() then
		error("TextChannel:SendAsync can only be called on the client.", 3)
	end
	text = tostring(text or "")
	local player = Players.LocalPlayer
	local source = player and sourceOf(channel, player.UserId)
	local id = host.guid()
	local m = message(channel, text, "Sending", source, id)
	incoming(channel, m)
	host.fire(TextChatService, "SendingMessage", m)
	host.send(channel, text, id)
	return m
end

local function displaySystemMessage(channel, text)
	if not host.client() then
		error("TextChannel:DisplaySystemMessage can only be called on the client.", 3)
	end
	local m = message(channel, tostring(text or ""), "Success", nil, host.guid())
	incoming(channel, m)
	host.write(m, "Text", string.format(SYSTEM_STYLE, m.Text))
	host.fire(TextChatService, "MessageReceived", m)
	host.fire(channel, "MessageReceived", m)
	host.log("", m.Text, channel.Name, 0)
	return m
end

local function addUserAsync(channel, userId)
	local existing = sourceOf(channel, userId)
	if existing then
		return existing, true
	end
	local player = Players:GetPlayerByUserId(userId)
	local source = host.new("TextSource")
	source.Name = player and player.Name or tostring(userId)
	host.write(source, "UserId", userId)
	source.Parent = channel
	return source, true
end

local methods = {
	TextChannel = {
		SendAsync = sendAsync,
		DisplaySystemMessage = displaySystemMessage,
		AddUserAsync = addUserAsync,
	},
	TextChatService = {
		CanUserChatAsync = function()
			return true
		end,
		CanUsersChatAsync = function()
			return true
		end,
		CanUsersDirectChatAsync = function(_, _, ids)
			return ids
		end,
	},
}

-- A member of a TextChannel or of TextChatService.
function M.member(class, owner, key)
	local f = methods[class] and methods[class][key]
	if f then
		return f, true
	end
	if CALLBACKS[class] and CALLBACKS[class][key] then
		return callback(owner, key), true
	end
	return nil, false
end

-- A callback set (`OnIncomingMessage = f`); whether it was one.
function M.set(class, owner, key, value)
	if not (CALLBACKS[class] and CALLBACKS[class][key]) then
		return false
	end
	local id = host.id(owner)
	callbacks[id] = callbacks[id] or {}
	callbacks[id][key] = value
	return true
end

-- The server: `player` sent `text` to `channel`.
function M.fromClient(player, channel, text, id)
	if not channel or not channel:IsA("TextChannel") then
		return
	end
	local sender = player and sourceOf(channel, player.UserId)
	if sender and not sender.CanSend then
		return
	end
	for _, to in channel:GetChildren() do
		if to:IsA("TextSource") then
			local deliver = true
			local should = callback(channel, "ShouldDeliverCallback")
			if should then
				local ok, result = pcall(should, message(channel, text, "Sending", sender, id), to)
				deliver = ok and result ~= false
			end
			local recipient = Players:GetPlayerByUserId(to.UserId)
			if deliver and recipient then
				host.deliver(recipient, channel, text, id, sender)
			end
		end
	end
end

-- A client: the server delivered a message.
function M.fromServer(channel, text, id, source)
	if not channel then
		return
	end
	local m = message(channel, text, "Success", source, id)
	incoming(channel, m)
	host.fire(TextChatService, "MessageReceived", m)
	host.fire(channel, "MessageReceived", m)
	host.log(m.PrefixText, m.Text, channel.Name, source and source.UserId or 0)
end

-- The editor's chat bar: what the local player typed, to the general
-- channel.
function M.say(text)
	local channels = TextChatService:FindFirstChild("TextChannels")
	local general = channels and channels:FindFirstChild("RBXGeneral")
	if general then
		sendAsync(general, text)
	end
end

return M
