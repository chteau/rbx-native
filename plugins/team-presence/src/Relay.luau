--!strict
--! Signed HTTP client for the presence relay; each request carries the HMAC headers from Auth.

local HttpService = game:GetService("HttpService")

local Auth = require(script.Parent.Auth)
local Schema = require(script.Parent.Schema)

export type Config = { url: string, key: string, room: string, userId: number }

local Relay = {}
Relay.__index = Relay

function Relay.new(config: Config)
	local self = {
		base = string.gsub(config.url, "/+$", ""),
		key = config.key,
		room = config.room,
		userId = config.userId,
		sendKey = true,
	}
	return setmetatable(self, Relay)
end

local function nowMs(): number
	return DateTime.now().UnixTimestampMillis
end

-- Returns the response or nil plus an error text.
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

function Relay:publish(state: any): (boolean, string?)
	local res, err = self:_send("POST", "/state", Schema.encode(state))
	if res == nil then
		return false, err
	end
	return true, nil
end

function Relay:poll(cursor: number): (any?, string?)
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
	self:_send("DELETE", "/state", "")
end

return Relay
