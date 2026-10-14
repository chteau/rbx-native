local HttpService = game:GetService("HttpService")
local LogService = game:GetService("LogService")
local RunService = game:GetService("RunService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local BASE = script:GetAttribute("BASE")
local TOKEN = script:GetAttribute("TOKEN")
local SESSION = script:GetAttribute("SESSION")
local MAX_BATCH = 200

local t0 = os.clock()
local buffer = {}
local running = true

local function post(items)
	local ok, res = pcall(function()
		return HttpService:RequestAsync({
			Url = BASE .. "/v1/ingest",
			Method = "POST",
			Headers = {
				["Content-Type"] = "application/json",
				["X-Probe-Token"] = TOKEN,
			},
			Body = HttpService:JSONEncode(items),
		})
	end)
	return ok and res.Success
end

local function flush()
	while #buffer > 0 do
		local batch = {}
		local n = math.min(#buffer, MAX_BATCH)
		for i = 1, n do
			batch[i] = buffer[i]
		end
		table.move(buffer, n + 1, #buffer + n, 1)
		for i = #buffer - n + 1, #buffer do
			buffer[i] = nil
		end
		post(batch)
	end
end

local levels = {
	[Enum.MessageType.MessageOutput] = "output",
	[Enum.MessageType.MessageInfo] = "info",
	[Enum.MessageType.MessageWarning] = "warning",
	[Enum.MessageType.MessageError] = "error",
}

local function push(side, level, message, time)
	table.insert(buffer, {
		type = "log",
		side = side,
		level = level,
		message = message,
		time = time,
	})
end

-- Neither flag can simply be read: LoadStringEnabled is not scriptable at
-- all, so loadstring is probed by calling it; HttpEnabled is read in a pcall.
local http_read, http_value = pcall(function()
	return HttpService.HttpEnabled
end)
local http_enabled = http_read and http_value == true
local compiled, chunk = pcall(loadstring, "return 1")
local loadstring_enabled = compiled and type(chunk) == "function"

LogService.MessageOut:Connect(function(message, messageType)
	push("server", levels[messageType] or "output", message, os.clock() - t0)
end)

local remote = ReplicatedStorage:WaitForChild("__rbxnative_remote")
remote.OnServerEvent:Connect(function(_player, payload)
	if type(payload) ~= "table" then
		return
	end
	for _, entry in payload do
		if type(entry) == "table" and type(entry.message) == "string" then
			local level = entry.level
			if level ~= "output" and level ~= "info" and level ~= "warning" and level ~= "error" then
				level = "output"
			end
			push("client", level, entry.message, tonumber(entry.time) or 0)
		end
	end
end)

Players.PlayerRemoving:Connect(function(player)
	post({ { type = "left", user_id = player.UserId } })
end)

Players.PlayerAdded:Connect(function(player)
	post({ { type = "joined", user_id = player.UserId } })
end)
for _, player in Players:GetPlayers() do
	post({ { type = "joined", user_id = player.UserId } })
end

game:BindToClose(function()
	running = false
	flush()
	post({ { type = "bye" } })
end)

post({
	{
		type = "hello",
		session = SESSION,
		place_version = game.PlaceVersion,
		http_enabled = http_enabled,
		loadstring_enabled = loadstring_enabled,
		is_studio = RunService:IsStudio(),
		job_id = game.JobId,
	},
})

task.spawn(function()
	while running do
		task.wait(0.2)
		if #buffer > 0 then
			flush()
		end
	end
end)

local function ack(id, ok, result)
	post({ { type = "ack", id = id, ok = ok, result = result } })
end

local function handle(cmd)
	local kind = cmd.type
	if kind == "eval" then
		local fn, err = loadstring(cmd.source)
		if not fn then
			ack(cmd.id, false, tostring(err))
			return
		end
		local ok, value = pcall(fn)
		ack(cmd.id, ok, tostring(value))
	elseif kind == "reload" then
		local fn, err = loadstring(cmd.source)
		if not fn then
			ack(cmd.id, false, tostring(err))
			return
		end
		task.spawn(fn)
		ack(cmd.id, true, "reloaded " .. tostring(cmd.path))
	elseif kind == "shutdown" then
		for _, p in Players:GetPlayers() do
			p:Kick("Play session ended")
		end
		ack(cmd.id, true, "shutdown")
		running = false
	else
		ack(cmd.id, false, "unknown command: " .. tostring(kind))
	end
end

local last = 0
local backoff = 0
while running do
	local ok, res = pcall(function()
		return HttpService:RequestAsync({
			Url = BASE .. "/v1/poll?since=" .. last,
			Method = "GET",
			Headers = { ["X-Probe-Token"] = TOKEN },
		})
	end)
	if ok and res.StatusCode == 404 then
		break
	end
	if ok and (res.StatusCode == 200 or res.StatusCode == 204) then
		backoff = 0
		if res.StatusCode == 200 then
			local decoded, cmds = pcall(function()
				return HttpService:JSONDecode(res.Body)
			end)
			if decoded and type(cmds) == "table" then
				for _, cmd in cmds do
					if cmd.id > last then
						handle(cmd)
						last = cmd.id
						if not running then
							break
						end
					end
				end
			end
		end
	else
		backoff = if backoff == 0 then 2 else math.min(backoff * 2, 8)
		task.wait(backoff)
	end
end
