local HttpService = game:GetService("HttpService")
local LogService = game:GetService("LogService")
local RunService = game:GetService("RunService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local BASE = script:GetAttribute("BASE")
local TOKEN = script:GetAttribute("TOKEN")
local SESSION = script:GetAttribute("SESSION")
local MAX_BYTES = 400000
local FLUSH_PERIOD = 0.25

local codec = require(ReplicatedStorage:WaitForChild("__rbxnative_codec"))
local Live = require(ReplicatedStorage:WaitForChild("__rbxnative_live"))

local Debug = require(ReplicatedStorage:WaitForChild("__rbxnative_debug"))

local t0 = os.clock()
local buffer = {}
local running = true
local tester = nil
-- Client-bound commands that arrived before the tester's client did; sent
-- in order once it joins (a RemoteEvent queues them until the mirror
-- connects its handler).
local forLater = {}

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

local function queue(message)
	table.insert(buffer, message)
end

local function flush()
	if #buffer == 0 then
		return
	end
	local pending = buffer
	buffer = {}
	local batch = {}
	local size = 0
	for _, message in pending do
		local okEnc, encoded = pcall(HttpService.JSONEncode, HttpService, message)
		if okEnc then
			if size + #encoded > MAX_BYTES and #batch > 0 then
				post(batch)
				batch = {}
				size = 0
			end
			table.insert(batch, message)
			size += #encoded + 1
		end
	end
	if #batch > 0 then
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
	queue({
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

Debug.attach("server", queue)

local live = Live.new("server", queue, codec)

local remote = ReplicatedStorage:WaitForChild("__rbxnative_remote")
remote.OnServerEvent:Connect(function(player, payload)
	if type(payload) ~= "table" then
		return
	end
	if payload.kind == "msg" then
		if player == tester and type(payload.message) == "table" then
			payload.message.side = "client"
			queue(payload.message)
		end
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

local function joined(player)
	if tester == nil then
		tester = player
		for _, cmd in forLater do
			remote:FireClient(player, { kind = "cmd", command = cmd })
		end
		table.clear(forLater)
	end
	queue({ type = "joined", user_id = player.UserId })
end

Players.PlayerRemoving:Connect(function(player)
	if player == tester then
		tester = nil
	end
	queue({ type = "left", user_id = player.UserId })
end)

Players.PlayerAdded:Connect(joined)
for _, player in Players:GetPlayers() do
	joined(player)
end

game:BindToClose(function()
	running = false
	live.stop()
	queue({ type = "bye" })
	flush()
end)

queue({
	type = "hello",
	session = SESSION,
	place_version = game.PlaceVersion,
	http_enabled = http_enabled,
	loadstring_enabled = loadstring_enabled,
	is_studio = RunService:IsStudio(),
	job_id = game.JobId,
})

task.spawn(function()
	while running do
		flush()
		task.wait(FLUSH_PERIOD)
	end
end)

task.spawn(live.snapshot)

local Stats = game:GetService("Stats")
local beats = 0
RunService.Heartbeat:Connect(function()
	beats += 1
end)

task.spawn(function()
	while running do
		local started = os.clock()
		beats = 0
		task.wait(1)
		-- task.wait resumes a little late; rate over the real interval.
		local m = { fps = beats / math.max(os.clock() - started, 0.001) }
		local function read(key, f)
			local okRead, value = pcall(f)
			if okRead and type(value) == "number" then
				m[key] = value
			end
		end
		read("physics_fps", function()
			return workspace:GetRealPhysicsFPS()
		end)
		read("memory_mb", function()
			return Stats:GetTotalMemoryUsageMb()
		end)
		read("instances", function()
			return Stats.InstanceCount
		end)
		read("send_kbps", function()
			return Stats.DataSendKbps
		end)
		read("receive_kbps", function()
			return Stats.DataReceiveKbps
		end)
		read("frame_ms", function()
			return Stats.HeartbeatTime * 1000
		end)
		read("players", function()
			return #Players:GetPlayers()
		end)
		queue({ type = "stats", side = "server", metrics = m })
	end
end)

local function ack(id, ok, result)
	queue({ type = "ack", id = id, ok = ok, result = result })
end

local function ackLive(cmd, ok, result)
	ack(cmd.id, ok == true, tostring(result))
end

local function handleLive(cmd)
	local kind = cmd.type
	if kind == "watch" then
		live.watch(cmd.items or {})
		ack(cmd.id, true, "watching")
	elseif kind == "set" then
		ackLive(cmd, live.set(cmd.target, cmd.prop, cmd.value))
	elseif kind == "set_attribute" then
		ackLive(cmd, live.set_attribute(cmd.target, cmd.name, cmd.value))
	elseif kind == "set_tags" then
		ackLive(cmd, live.set_tags(cmd.target, cmd.tags or {}))
	elseif kind == "destroy" then
		ackLive(cmd, live.destroy(cmd.target))
	elseif kind == "reparent" then
		ackLive(cmd, live.reparent(cmd.target, cmd.parent))
	elseif kind == "create" then
		ackLive(cmd, live.create(cmd.class, cmd.name, cmd.parent))
	elseif kind == "debug_set" or kind == "debug_resume" or kind == "debug_watch" then
		Debug.command(cmd)
	else
		ackLive(cmd, live.resync())
	end
end

local LIVE_COMMANDS = {
	watch = true,
	set = true,
	set_attribute = true,
	set_tags = true,
	destroy = true,
	reparent = true,
	create = true,
	resync = true,
	debug_set = true,
	debug_resume = true,
	debug_watch = true,
}

local function handle(cmd)
	local kind = cmd.type
	if LIVE_COMMANDS[kind] then
		if cmd.side == "client" then
			if tester then
				remote:FireClient(tester, { kind = "cmd", command = cmd })
			else
				table.insert(forLater, cmd)
			end
		else
			handleLive(cmd)
		end
	elseif kind == "eval" then
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
flush()
