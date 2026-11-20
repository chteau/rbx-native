-- MessagingService on one server: a server gets its own messages, what the
-- callback is handed (Data, Sent), how long delivery takes in frames,
-- several subscribers and Disconnect, and the errors for long topics, big
-- or unserialisable messages. The topic carries a per-pass suffix.
return function(h)
	local MessagingService = game:GetService("MessagingService")
	local HttpService = game:GetService("HttpService")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local topic = "localrun_m12_" .. HttpService:GenerateGUID(false):sub(1, 8)
	local got = {}
	local second = 0
	local connection = MessagingService:SubscribeAsync(topic, function(message)
		table.insert(got, message)
	end)
	result.connection_type = typeof(connection)
	local other = MessagingService:SubscribeAsync(topic, function()
		second += 1
	end)
	local sentAt = os.time()
	result.publish = err(function()
		return MessagingService:PublishAsync(topic, { n = 1, s = "x" })
	end)
	local frames = 0
	while #got == 0 and frames < 600 do
		h.beat()
		frames += 1
	end
	result.received = #got
	result._frames = frames
	result.delivered_within_10s = frames < 600
	local message = got[1]
	if type(message) == "table" then
		local keys = {}
		for k in message do
			table.insert(keys, tostring(k))
		end
		table.sort(keys)
		result.message_keys = table.concat(keys, ",")
		result.data = type(message.Data) == "table" and (tostring(message.Data.n) .. "," .. tostring(message.Data.s))
			or tostring(message.Data)
		result.sent_type = type(message.Sent)
		result.sent_close = type(message.Sent) == "number" and math.abs(message.Sent - sentAt) < 60
	end
	result.second_subscriber = second
	other:Disconnect()
	MessagingService:PublishAsync(topic, "plain")
	frames = 0
	while #got < 2 and frames < 600 do
		h.beat()
		frames += 1
	end
	result.plain = got[2] and tostring(got[2].Data) or "none"
	result.second_after_disconnect = second
	connection:Disconnect()
	result.topic_long = err(function()
		MessagingService:PublishAsync(string.rep("t", 81), 1)
	end)
	result.topic_80 = err(function()
		MessagingService:PublishAsync("localrun_m12_" .. string.rep("t", 67), 1)
	end)
	result.subscribe_topic_long = err(function()
		MessagingService:SubscribeAsync(string.rep("t", 81), function() end)
	end)
	result.message_big = err(function()
		MessagingService:PublishAsync(topic, string.rep("m", 1025))
	end)
	result.message_function = err(function()
		MessagingService:PublishAsync(topic, print)
	end)
	result.message_instance = err(function()
		MessagingService:PublishAsync(topic, workspace)
	end)
	result.subscribe_not_function = err(function()
		MessagingService:SubscribeAsync(topic, 5)
	end)
	return result
end
