--!strict
--! The presence transport. Two kinds, both official:
--!  - "worker": the signed-WebSocket relay in relay/ (here over HTTP long-poll,
--!    since Studio's HttpService cannot hold a socket), which keeps the room
--!    and answers with a snapshot.
--!  - "ntfy": a plain ntfy pub/sub server, which keeps nothing for us: we
--!    publish our signed state to the room's topic, poll the topic for the
--!    others, and rebuild the room here. Peers trust each other by HMAC.

local HttpService = game:GetService("HttpService")

local Auth = require(script.Parent.Auth)
local Ntfy = require(script.Parent.Ntfy)
local Schema = require(script.Parent.Schema)

export type Config = { kind: string, url: string, key: string, room: string, userId: number }

local EXPIRY_MS = 15000

local Relay = {}
Relay.__index = Relay

function Relay.new(config: Config)
	local self = {
		kind = config.kind,
		base = string.gsub(config.url, "/+$", ""),
		key = config.key,
		room = config.room,
		topic = Ntfy.topic(config.room),
		userId = config.userId,
		sendKey = true,
		-- ntfy only: the room rebuilt from the stream, and the newest id seen.
		peers = {} :: { [number]: { state: any, seen: number } },
		cursor = 0,
		lastId = nil :: string?,
	}
	return setmetatable(self, Relay)
end

local function nowMs(): number
	return DateTime.now().UnixTimestampMillis
end

-- A short wait between ntfy polls, so the poll loop does not spin.
function Relay:pollDelay(): number
	return if self.kind == "ntfy" then 1 else 0
end

-- ===== Worker (signed HTTP) =====

function Relay:_request(method: string, suffix: string, body: string): (any?, string?, any?)
	local path = "/v1/rooms/" .. self.room .. suffix
	local headers = Auth.headers(self.key, method, path, self.userId, nowMs(), body, self.sendKey)
	if body ~= "" then
		headers["Content-Type"] = "application/json"
	end
	local request: { [string]: any } = { Url = self.base .. path, Method = method, Headers = headers }
	if body ~= "" then
		request.Body = body
	end
	local ok, res = pcall(function()
		return HttpService:RequestAsync(request)
	end)
	if not ok then
		return nil, tostring(res)
	end
	if not res.Success then
		return nil, string.format("HTTP %d: %s", res.StatusCode, string.sub(res.Body or "", 1, 120)), res
	end
	self.sendKey = false
	return res, nil
end

local function needsKey(res: any): boolean
	if res == nil or res.StatusCode ~= 401 then
		return false
	end
	local ok, decoded = pcall(function()
		return HttpService:JSONDecode(res.Body)
	end)
	return ok and type(decoded) == "table" and decoded.error == "key"
end

function Relay:_send(method: string, suffix: string, body: string): (any?, string?)
	local res, err, raw = self:_request(method, suffix, body)
	if res == nil and needsKey(raw) and not self.sendKey then
		self.sendKey = true
		res, err = self:_request(method, suffix, body)
	end
	return res, err
end

-- ===== ntfy (plain pub/sub) =====

function Relay:_ntfyPublish(state: any): (boolean, string?)
	local body = Ntfy.encode(self.key, self.topic, self.userId, nowMs(), state)
	local ok, res = pcall(function()
		return HttpService:RequestAsync({
			Url = self.base .. "/" .. self.topic,
			Method = "POST",
			Headers = { ["Content-Type"] = "text/plain" },
			Body = body,
		})
	end)
	if not ok then
		return false, tostring(res)
	end
	if not res.Success then
		return false, string.format("ntfy publish HTTP %d", res.StatusCode)
	end
	return true, nil
end

function Relay:_ntfyPoll(): (any?, string?)
	local since = self.lastId or "10s"
	local url = string.format("%s/%s/json?poll=1&since=%s", self.base, self.topic, HttpService:UrlEncode(since))
	local ok, res = pcall(function()
		return HttpService:RequestAsync({ Url = url, Method = "GET" })
	end)
	if not ok then
		return nil, tostring(res)
	end
	if not res.Success then
		return nil, string.format("ntfy poll HTTP %d", res.StatusCode)
	end
	local now = nowMs()
	local changed = false
	for line in string.gmatch(res.Body or "", "[^\n]+") do
		local parsed
		local decoded = pcall(function()
			parsed = HttpService:JSONDecode(line)
		end)
		if decoded and type(parsed) == "table" then
			if type(parsed.id) == "string" then
				self.lastId = parsed.id
			end
			if parsed.event == "message" and type(parsed.message) == "string" then
				local state = Ntfy.decode(self.key, self.topic, now, parsed.message)
				if state and state.userId ~= self.userId then
					state.seen = now
					self.peers[state.userId] = { state = state, seen = now }
					changed = true
				end
			end
		end
	end
	for id, peer in self.peers do
		if now - peer.seen > EXPIRY_MS then
			self.peers[id] = nil
			changed = true
		end
	end
	if changed then
		self.cursor += 1
	end
	local users = {}
	for _, peer in self.peers do
		table.insert(users, peer.state)
	end
	return { cursor = self.cursor, users = users }, nil
end

-- ===== Shared surface =====

function Relay:publish(state: any): (boolean, string?)
	if self.kind == "ntfy" then
		return self:_ntfyPublish(state)
	end
	local res, err = self:_send("POST", "/state", Schema.encode(state))
	if res == nil then
		return false, err
	end
	return true, nil
end

function Relay:poll(cursor: number): (any?, string?)
	if self.kind == "ntfy" then
		return self:_ntfyPoll()
	end
	local res, err = self:_send("GET", string.format("/poll?since=%.0f", cursor), "")
	if res == nil then
		return nil, err
	end
	local ok, snapshot = pcall(Schema.decodeSnapshot, res.Body)
	if not ok then
		return nil, tostring(snapshot)
	end
	return snapshot, nil
end

function Relay:leave()
	if self.kind == "ntfy" then
		-- Nothing to delete on a dumb pub/sub; peers expire us.
		return
	end
	self:_send("DELETE", "/state", "")
end

return Relay
