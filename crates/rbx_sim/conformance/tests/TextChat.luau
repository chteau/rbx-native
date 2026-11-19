-- TextChatService: the default channels and configurations a server makes,
-- the player's TextSource, what a server may call, a channel the server
-- makes, and on the client SendAsync with the events it fires on both
-- sides (SendingMessage, OnIncomingMessage, MessageReceived,
-- ShouldDeliverCallback) and a system message. Needs a player.
return function(h)
	local TextChatService = game:GetService("TextChatService")
	local result = {}
	local player = h.player()
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local function names(list, pick)
		local out = {}
		for _, x in list do
			table.insert(out, pick(x))
		end
		table.sort(out)
		return table.concat(out, ",")
	end
	result.version = TextChatService.ChatVersion.Name
	result.create_default = tostring(TextChatService.CreateDefaultTextChannels)
	local channels = TextChatService:WaitForChild("TextChannels", 10)
	result.channels = channels and names(channels:GetChildren(), function(c)
		return c.ClassName .. ":" .. c.Name
	end) or "none"
	result.configs = names(TextChatService:GetChildren(), function(c)
		return c.ClassName
	end)
	local general = channels and channels:FindFirstChild("RBXGeneral")
	local source
	for _ = 1, 300 do
		for _, child in general and general:GetChildren() or {} do
			if child:IsA("TextSource") and child.UserId == player.UserId then
				source = child
			end
		end
		if source then
			break
		end
		h.beat()
	end
	result.source = source
			and string.format("%s,%s,%s", source.ClassName, tostring(source.Name == player.Name), tostring(source.CanSend))
		or "none"
	result.server_send = general and err(function()
		general:SendAsync("x")
	end) or "none"
	result.server_system = general and err(function()
		general:DisplaySystemMessage("x")
	end) or "none"

	local log = {}
	if general then
		general.ShouldDeliverCallback = function(message, to)
			table.insert(log, "deliver:" .. message.Status.Name .. ":" .. tostring(to.UserId == player.UserId))
			return true
		end
		general.MessageReceived:Connect(function(m)
			table.insert(log, "channel:" .. m.Text)
		end)
	end
	TextChatService.MessageReceived:Connect(function(m)
		table.insert(log, "service:" .. m.Text)
	end)
	h.beats(5)
	local seen = h.client("TextChat")
	for k, v in seen do
		result["client_" .. k] = v
	end
	h.beats(30)
	result.server_log = table.concat(log, " ")
	if general then
		general.ShouldDeliverCallback = nil
	end

	if channels then
		local own = Instance.new("TextChannel")
		own.Name = "ConfChannel"
		own.Parent = channels
		local ok, added, success = pcall(function()
			return own:AddUserAsync(player.UserId)
		end)
		result.add_user = ok and string.format("%s,%s,%s", added and added.ClassName or "nil", tostring(success), tostring(added and added.Parent == own)) or "error"
		own:Destroy()
	end
	h.beats(5)
	return result
end
