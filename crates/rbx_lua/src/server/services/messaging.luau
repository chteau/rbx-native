--!nonstrict
-- MessagingService in Local Run, our own, written from the creator docs
-- and what the Messaging golden measured on a live server: a server hears
-- its own messages (before PublishAsync returns), every subscriber gets
-- { Data, Sent }, and the errors for long topics and big or unencodable
-- messages. Messages go through the host to every server of the run.
local host = ...

local M = {}

local TOPIC_LIMIT = 80
local MESSAGE_LIMIT = 1024

local serviceId = nil

local function fail(message)
	error(message, 0)
end

local function topicOf(topic)
	if topic == nil then
		fail("Argument 1 missing or nil")
	end
	topic = tostring(topic)
	if #topic > TOPIC_LIMIT then
		fail("MessagingService: Topic names cannot be more than 80 characters.")
	end
	return topic
end

local function unencodable(value, seen)
	local kind = type(value)
	if kind == "table" then
		if seen[value] then
			return true
		end
		seen[value] = true
		for k, v in value do
			if unencodable(k, seen) or unencodable(v, seen) then
				return true
			end
		end
		seen[value] = nil
		return false
	end
	if kind == "string" then
		return utf8.len(value) == nil
	end
	return not (kind == "nil" or kind == "boolean" or kind == "number")
end

local SERVICE = {}

function SERVICE.PublishAsync(self, topic, message)
	topic = topicOf(topic)
	if unencodable(message, {}) then
		fail("MessagingService:PublishAsync(): Cannot publish message, can only accept valid UTF-8 characters.")
	end
	local text = host.encode(message)
	if #text > MESSAGE_LIMIT then
		fail("MessagingService:PublishAsync(): Cannot publish, data exceeded 1kb")
	end
	serviceId = host.id(self)
	host.publish(topic, text)
	task.wait()
end

function SERVICE.SubscribeAsync(self, topic, callback)
	topic = topicOf(topic)
	if type(callback) ~= "function" then
		fail("Unable to cast " .. (type(callback) == "number" and "double" or typeof(callback)) .. " to Function")
	end
	serviceId = host.id(self)
	task.wait()
	return host.signal(serviceId, "Message:" .. topic):Connect(callback)
end

function M.member(_, id, key)
	serviceId = serviceId or id
	return SERVICE[key], SERVICE[key] ~= nil
end

-- A message published by any server of the run.
function M.deliver(topic, text, sent)
	if serviceId then
		host.fire(serviceId, "Message:" .. topic, { Data = host.decode(text), Sent = sent })
	end
end

return M
