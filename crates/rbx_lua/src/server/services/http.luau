--!nonstrict
-- HttpService's requests in Local Run: the argument checks and messages a
-- live server gives (the HttpJson golden), HttpEnabled, Roblox's 500
-- requests a minute, then the request itself through the app's network
-- gate (refused offline, which fails as an unreachable server does).
-- Secrets come from Studio's local store, web stream clients stream for
-- real (SSE, raw, WebSocket).
local host = ...

local M = {}

local PER_MINUTE = 500
local sent = {} -- times of the last minute's requests

local METHODS = { GET = true, HEAD = true, POST = true, PUT = true, DELETE = true, OPTIONS = true, PATCH = true }

local function fail(message)
	error(message, 0)
end

local function checkUrl(url)
	if url == nil then
		fail("Argument 1 missing or nil")
	end
	url = tostring(url)
	local lower = url:lower()
	if lower:sub(1, 7) ~= "http://" and lower:sub(1, 8) ~= "https://" then
		fail("URL must be http")
	end
	return url
end

local function send(options)
	if not host.http_enabled() then
		fail("Http requests are not enabled. Enable via game settings")
	end
	local now = host.clock()
	for i = #sent, 1, -1 do
		if now - sent[i] >= 60 then
			table.remove(sent, i)
		end
	end
	if #sent >= PER_MINUTE then
		fail("Number of requests exceeded limit")
	end
	table.insert(sent, now)
	return host.request(options)
end

-- Headers as sent: a Secret goes as its value, when `url` may carry it.
local function headers(list, url)
	local out = {}
	if type(list) == "table" then
		for name, value in list do
			out[tostring(name)] = host.reveal(value, url) or tostring(value)
		end
	end
	return out
end

-- A URL that may be a Secret (the whole URL, as GetSecret gives it).
local function urlOf(url)
	if typeof(url) == "Secret" then
		return checkUrl(host.reveal(url))
	end
	return checkUrl(url)
end

local function body(response)
	if not response.Success then
		fail(string.format("HTTP %d (%s)", response.StatusCode, response.StatusMessage))
	end
	return response.Body
end

local CONTENT_TYPES = {
	ApplicationJson = "application/json",
	ApplicationXml = "application/xml",
	ApplicationUrlEncoded = "application/x-www-form-urlencoded",
	TextPlain = "text/plain",
	TextXml = "text/xml",
}

local SERVICE = {}

function SERVICE.GetAsync(_, url, _nocache, extra)
	url = urlOf(url)
	return body(send({ Url = url, Method = "GET", Headers = headers(extra, url) }))
end

function SERVICE.PostAsync(_, url, data, contentType, _compress, extra)
	if data == nil then
		fail("Argument 2 missing or nil")
	end
	url = urlOf(url)
	local h = headers(extra, url)
	local kind = typeof(contentType) == "EnumItem" and CONTENT_TYPES[contentType.Name] or "application/json"
	h["Content-Type"] = h["Content-Type"] or kind
	return body(send({ Url = url, Method = "POST", Headers = h, Body = tostring(data) }))
end

function SERVICE.RequestAsync(_, options)
	if type(options) ~= "table" then
		fail("Unable to cast " .. typeof(options) .. " to Dictionary")
	end
	if options.Url == nil then
		fail("Invalid request options")
	end
	local method = options.Method == nil and "GET" or tostring(options.Method):upper()
	if options.Body ~= nil and (method == "GET" or method == "HEAD") then
		fail("Invalid request options, Body should be empty for 'GET' or 'HEAD' method")
	end
	local url = urlOf(options.Url)
	if not METHODS[method] then
		fail("Invalid request options, Method must be one of GET, HEAD, POST, PUT, DELETE, OPTIONS, PATCH")
	end
	return send({
		Url = url,
		Method = method,
		Headers = headers(options.Headers, url),
		Body = options.Body ~= nil and tostring(options.Body) or nil,
	})
end

-- Studio's local secrets (the place's secrets.json in Local Run), as
-- Secret values only a request may carry.
function SERVICE.GetSecret(_, key)
	if type(key) ~= "string" then
		fail(if key == nil then "Argument 1 missing or nil" else "Unable to cast " .. typeof(key) .. " to string")
	end
	local secret = host.secret(key)
	if secret == nil then
		fail("Can't find secret with given key")
	end
	return secret
end

-- Web stream clients (Studio only on Roblox, so a Local Run has them).
local STREAM_LIMIT = 4
local streams = {} -- instance id -> { state, kind }

local STREAM_KINDS = { SSE = true, RawStream = true, WebSocket = true }

function SERVICE.CreateWebStreamClient(_, kind, options)
	if typeof(kind) ~= "EnumItem" or kind.EnumType ~= Enum.WebStreamClientType then
		fail("Unable to cast " .. (kind == nil and "nil" or typeof(kind)) .. " to WebStreamClientType")
	end
	if type(options) ~= "table" then
		fail("Unable to cast " .. typeof(options) .. " to Dictionary")
	end
	if options.Url == nil then
		fail("Invalid request options")
	end
	local open = 0
	for _, s in streams do
		if s.state ~= Enum.WebStreamClientState.Closed then
			open += 1
		end
	end
	if open >= STREAM_LIMIT then
		fail("Too many web stream clients are open: close one first")
	end
	if not host.http_enabled() then
		fail("Http requests are not enabled. Enable via game settings")
	end
	local url = urlOf(options.Url)
	local method = options.Method == nil and "GET" or tostring(options.Method):upper()
	assert(STREAM_KINDS[kind.Name])
	local instance, id = host.new("WebStreamClient", "WebStreamClient")
	streams[id] = { state = Enum.WebStreamClientState.Connecting, kind = kind.Name }
	host.stream_open(id, kind.Name, url, method, headers(options.Headers, url), options.Body and tostring(options.Body))
	return instance
end

local CLIENT = {}

function CLIENT.Send(self, data)
	local s = streams[host.id(self)]
	if s.kind ~= "WebSocket" then
		fail("WebStreamClient:Send() only works on a WebSocket client")
	end
	if s.state ~= Enum.WebStreamClientState.Open then
		fail("WebStreamClient:Send() needs an open connection")
	end
	host.stream_send(host.id(self), tostring(data))
end

function CLIENT.Close(self)
	local id = host.id(self)
	local s = streams[id]
	if s.state == Enum.WebStreamClientState.Closed then
		return
	end
	host.stream_send(id, nil)
	s.state = Enum.WebStreamClientState.Closed
	host.fire(id, "Closed")
end

-- What a client's connection did, once a frame.
function M.stream_event(id, kind, code, text)
	local s = streams[id]
	if not s or s.state == Enum.WebStreamClientState.Closed then
		return
	end
	if kind == "Opened" then
		s.state = Enum.WebStreamClientState.Open
		host.fire(id, "Opened", code, text)
	elseif kind == "MessageReceived" then
		host.fire(id, "MessageReceived", text)
	elseif kind == "Error" then
		s.state = Enum.WebStreamClientState.Error
		host.fire(id, "Error", code, text)
	else
		s.state = Enum.WebStreamClientState.Closed
		host.fire(id, "Closed")
	end
end

local CLIENT_EVENTS = { Opened = true, MessageReceived = true, Error = true, Closed = true }

function M.member(class, id, key)
	if class == "WebStreamClient" then
		local s = streams[id]
		if not s then
			return nil, false
		end
		if key == "ConnectionState" then
			return s.state, true
		end
		if CLIENT_EVENTS[key] then
			return host.signal(id, key), true
		end
		return CLIENT[key], CLIENT[key] ~= nil
	end
	return SERVICE[key], SERVICE[key] ~= nil
end

return M
