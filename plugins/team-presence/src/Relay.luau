--!strict
--! The presence transport. Three kinds, all official:
--!  - "hosted" (the default): an rbx-native collaborator runs the relay
--!    behind a Cloudflare quick tunnel and announces its address on ntfy
--!    (see Announce); we read the address, then speak "worker" to it. Studio
--!    cannot host, so with nobody in rbx-native we wait.
--!  - "worker": the signed-WebSocket relay in relay/ (here over HTTP long-poll,
--!    since Studio's HttpService cannot hold a socket), which keeps the room
--!    and answers with a snapshot.
--!  - "ntfy": a plain ntfy pub/sub server, which keeps nothing for us: we
--!    publish our signed state to the room's topic, poll the topic for the
--!    others, and rebuild the room here. Peers trust each other by HMAC.

local HttpService = game:GetService("HttpService")

local Announce = require(script.Parent.Announce)
local Auth = require(script.Parent.Auth)
local Ntfy = require(script.Parent.Ntfy)
local Schema = require(script.Parent.Schema)

export type Config = { kind: string, url: string, key: string, encKey: string, room: string, userId: number }

local VERSION_MISMATCH =
	"The relay runs a different Team Presence version; update the plugin, rbx-native and the relay together."

-- Plugins have no CSPRNG API. A v4 GUID is 122 random bits, so 12 of its 16
-- bytes, mixed with a Random stream, make a nonce that does not repeat.
local rng = Random.new()

local function nonceFor(): string
	local hex = string.gsub(HttpService:GenerateGUID(false), "[^%x]", "")
	local bytes = table.create(12)
	for i = 1, 12 do
		local guidByte = tonumber(string.sub(hex, i * 2 - 1, i * 2), 16) or 0
		bytes[i] = string.char(bit32.bxor(guidByte, rng:NextInteger(0, 255)))
	end
	return table.concat(bytes)
end

local EXPIRY_MS = 15000
-- How often a hosted room looks for its host while it has none.
local RESOLVE_EVERY = 15
local WAITING = "Waiting for an rbx-native collaborator to open the place"


local Relay = {}
Relay.__index = Relay

function Relay.new(config: Config)
	local url = string.gsub(config.url, "/+$", "")
	local hosted = config.kind == "hosted"
	local self = {
		kind = config.kind,
		-- hosted: the announce server; `base` is the host, once found.
		announceBase = if hosted then url else nil,
		base = if hosted then nil else url,
		lastResolve = -math.huge,
		key = config.key,
		encKey = config.encKey,
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

-- A short wait between ntfy polls, so the poll loop does not spin; a
-- hosted room with no host yet waits until it looks again.
function Relay:pollDelay(): number
	if self.kind == "hosted" and self.base == nil then
		return RESOLVE_EVERY
	end
	return if self.kind == "ntfy" then 1 else 0
end

-- Whether a hosted room is waiting for an rbx-native user to host it (not
-- an error: nothing is wrong, nobody is there yet).
function Relay:waiting(): boolean
	return self.kind == "hosted" and self.base == nil
end

-- hosted: finds the host from the newest announcement, at most once every
-- RESOLVE_EVERY seconds. True when there is one to talk to.
function Relay:_resolve(): (boolean, string?)
	if self.base ~= nil then
		return true, nil
	end
	if os.clock() - self.lastResolve < RESOLVE_EVERY then
		return false, WAITING
	end
	self.lastResolve = os.clock()
	local url = string.format("%s/%s/json?poll=1&since=12h", self.announceBase, Announce.topic(self.room))
	local ok, res = pcall(function()
		return HttpService:RequestAsync({ Url = url, Method = "GET" })
	end)
	if not ok then
		return false, tostring(res)
	end
	if not res.Success then
		return false, string.format("announce HTTP %d", res.StatusCode)
	end
	local texts = {}
	for line in string.gmatch(res.Body or "", "[^\n]+") do
		local decoded, parsed = pcall(function()
			return HttpService:JSONDecode(line)
		end)
		if decoded and type(parsed) == "table" and parsed.event == "message" and type(parsed.message) == "string" then
			table.insert(texts, parsed.message)
		end
	end
	local host = Announce.newest(self.key, self.room, nowMs(), texts)
	if not host then
		return false, WAITING
	end
	self.base = string.gsub(host, "/+$", "")
	self.sendKey = true
	return true, nil
end

-- hosted: the host stopped answering (left, crashed, or handed over); look
-- for it again on the next call.
function Relay:_lost()
	if self.kind == "hosted" then
		self.base = nil
		self.lastResolve = -math.huge
	end
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
		if res.StatusCode == 400 and string.find(res.Body or "", "version", 1, true) then
			return nil, VERSION_MISMATCH, res
		end
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
	if self.kind == "hosted" then
		local found, why = self:_resolve()
		if not found then
			return nil, why
		end
	end
	local res, err, raw = self:_request(method, suffix, body)
	if res == nil and raw == nil and self.kind == "hosted" then
		-- No HTTP answer at all: the host may be gone, or its address not
		-- reachable yet; look again shortly.
		self:_lost()
		err = `Can't reach the rbx-native host yet ({err})`
	elseif res == nil and raw and self.kind == "hosted" and (raw.StatusCode == 404 or raw.StatusCode >= 500) then
		-- A closed tunnel answers 404/502/530 from Cloudflare's edge (an
		-- HTML page: keep it out of the status line).
		self:_lost()
		err = `Can't reach the rbx-native host yet (HTTP {raw.StatusCode})`
	end
	if res == nil and needsKey(raw) and not self.sendKey then
		self.sendKey = true
		res, err = self:_request(method, suffix, body)
	end
	return res, err
end

-- ===== ntfy (plain pub/sub) =====

function Relay:_ntfyPublish(state: any): (boolean, string?)
	local body = Ntfy.fitEncode(self.key, self.encKey, self.room, self.topic, self.userId, nowMs(), state, nonceFor)
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
				local state = Ntfy.decode(self.key, self.encKey, self.room, self.topic, now, parsed.message)
				-- Our own echo is this user in Studio; the same user in
				-- rbx-native is another sender, keyed apart.
				if state and not (state.userId == self.userId and state.client == "studio") then
					state.seen = now
					self.peers[`{state.userId}/{state.client}`] = { state = state, seen = now }
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
	local res, err = self:_send("POST", "/state", Schema.sealState(self.encKey, self.room, state, nonceFor))
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
	local ok, snapshot = pcall(Schema.openSnapshot, self.encKey, self.room, res.Body)
	if not ok then
		return nil, tostring(snapshot)
	end
	return snapshot, nil
end

-- The value-less edit journal since `sinceMs`, for the "While you were
-- away" catch-up. Worker: a signed GET of its bounded journal. ntfy: the
-- topic's retained messages, verified against each message's own time
-- (they predate now by more than the signing skew), edits lifted out,
-- filtered to after `sinceMs` and de-duplicated. Mirrors
-- crates/rbx_presence/src/catchup.rs.
function Relay:journal(sinceMs: number): (any?, string?)
	if self.kind == "ntfy" then
		local url = string.format("%s/%s/json?poll=1&since=12h", self.base, self.topic)
		local ok, res = pcall(function()
			return HttpService:RequestAsync({ Url = url, Method = "GET" })
		end)
		if not ok then
			return nil, tostring(res)
		end
		if not res.Success then
			return nil, string.format("ntfy journal HTTP %d", res.StatusCode)
		end
		local now = nowMs()
		local seen: { [string]: boolean } = {}
		local entries = {}
		for line in string.gmatch(res.Body or "", "[^\n]+") do
			local parsed
			local decoded = pcall(function()
				parsed = HttpService:JSONDecode(line)
			end)
			if
				decoded
				and type(parsed) == "table"
				and parsed.event == "message"
				and type(parsed.message) == "string"
			then
				local checkAt = if type(parsed.time) == "number" then parsed.time * 1000 else now
				local state = Ntfy.decode(self.key, self.encKey, self.room, self.topic, checkAt, parsed.message)
				if state then
					for _, edit in state.edits or {} do
						if edit.at > sinceMs then
							local dedupe =
								string.format("%.0f\0%s\0%s\0%.0f", state.userId, edit.path, edit.property, edit.at)
							if not seen[dedupe] then
								seen[dedupe] = true
								table.insert(entries, {
									userId = state.userId,
									name = state.name,
									path = edit.path,
									property = edit.property,
									at = edit.at,
								})
							end
						end
					end
				end
			end
		end
		return entries, nil
	end
	local res, err = self:_send("GET", string.format("/journal?since=%.0f", sinceMs), "")
	if res == nil then
		return nil, err
	end
	local parsed
	local ok = pcall(function()
		parsed = HttpService:JSONDecode(res.Body)
	end)
	if not ok or type(parsed) ~= "table" then
		return nil, "journal reply"
	end
	local opened = {}
	local raw = parsed.entries
	if type(raw) == "table" then
		for _, entry in raw do
			local plain = Schema.openJournal(self.encKey, self.room, entry)
			if plain ~= nil then
				table.insert(opened, plain)
			end
		end
	end
	return opened, nil
end

function Relay:leave()
	if self.kind == "ntfy" or self:waiting() then
		-- Nothing to delete on a dumb pub/sub (peers expire us), or no host.
		return
	end
	self:_send("DELETE", "/state", "")
end

return Relay
