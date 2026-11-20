--!nonstrict
-- HttpService's requests in Local Run: the argument checks and messages a
-- live server gives (the HttpJson golden), HttpEnabled, Roblox's 500
-- requests a minute, then the request itself through the app's network
-- gate (refused offline, which fails as an unreachable server does).
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

local function headers(list)
	local out = {}
	if type(list) == "table" then
		for name, value in list do
			out[tostring(name)] = tostring(value)
		end
	end
	return out
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
	url = checkUrl(url)
	return body(send({ Url = url, Method = "GET", Headers = headers(extra) }))
end

function SERVICE.PostAsync(_, url, data, contentType, _compress, extra)
	if data == nil then
		fail("Argument 2 missing or nil")
	end
	url = checkUrl(url)
	local h = headers(extra)
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
	local url = checkUrl(options.Url)
	if not METHODS[method] then
		fail("Invalid request options, Method must be one of GET, HEAD, POST, PUT, DELETE, OPTIONS, PATCH")
	end
	return send({
		Url = url,
		Method = method,
		Headers = headers(options.Headers),
		Body = options.Body ~= nil and tostring(options.Body) or nil,
	})
end

function M.member(_, _, key)
	return SERVICE[key], SERVICE[key] ~= nil
end

return M
