-- TextChat on the client: SendAsync's message and the events around it,
-- then a system message and an empty one.
local Players = game:GetService("Players")
local TextChatService = game:GetService("TextChatService")

return function(ch)
	local out = {}
	local channels = TextChatService:WaitForChild("TextChannels", 10)
	local general = channels and channels:WaitForChild("RBXGeneral", 10)
	local system = channels and channels:WaitForChild("RBXSystem", 10)
	if not general then
		return { send = "no channel" }
	end
	local log = {}
	local connections = {
		TextChatService.SendingMessage:Connect(function(m)
			table.insert(log, "sending:" .. m.Status.Name)
		end),
		TextChatService.MessageReceived:Connect(function(m)
			table.insert(log, "received:" .. m.Status.Name .. ":" .. m.Text)
		end),
		general.MessageReceived:Connect(function(m)
			table.insert(log, "channel:" .. m.Status.Name)
		end),
	}
	TextChatService.OnIncomingMessage = function(m)
		table.insert(log, "incoming:" .. m.Status.Name)
		return nil
	end
	local ok, message = pcall(function()
		return general:SendAsync("conformance hello")
	end)
	if ok then
		out.send = string.format(
			"%s,%s,%s,%s,%s,%s",
			typeof(message),
			message.ClassName,
			message.Status.Name,
			message.Text,
			tostring(message.TextSource ~= nil and message.TextSource.UserId == Players.LocalPlayer.UserId),
			tostring(message.TextChannel == general)
		)
		out.message_id = tostring(#message.MessageId > 0)
	else
		out.send = tostring(message):gsub("^.-:%d+: ", "")
	end
	table.insert(log, "returned")
	ch.beats(60)
	out.log = table.concat(log, " ")

	log = {}
	if system then
		local sys = system:DisplaySystemMessage("conformance system")
		out.system = string.format("%s,%s,%s", typeof(sys), sys and sys.Status.Name or "nil", tostring(sys and sys.TextSource))
	end
	ch.beats(10)
	out.system_log = table.concat(log, " ")
	local okEmpty, why = pcall(function()
		return general:SendAsync("")
	end)
	out.empty = okEmpty and "ok" or (tostring(why):gsub("^.-:%d+: ", ""))
	ch.beats(10)
	TextChatService.OnIncomingMessage = nil
	for _, c in connections do
		c:Disconnect()
	end
	return out
end
