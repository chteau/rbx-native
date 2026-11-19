-- Sound at run time: loading (IsLoaded, Loaded, TimeLength), what a
-- playing sound does on the server (TimePosition, Ended, DidLoop,
-- PlaybackSpeed, Stop/Pause/Resume, a TimePosition write), the defaults
-- and clamps, then the same on the client, which really plays it. Two of
-- Roblox's own public sounds: a 3.072 s one and a 0.562 s one. Needs a
-- player for the client half.
local LONG = "rbxassetid://9125644905"
local SHORT = "rbxassetid://12222216"

return function(h, origin)
	local result = {}
	local frame = 0
	local stepped = game:GetService("RunService").Stepped:Connect(function()
		frame += 1
	end)
	local function sound(id, props)
		local s = Instance.new("Sound")
		s.SoundId = id
		for k, v in props or {} do
			s[k] = v
		end
		s.Parent = workspace
		return s
	end
	local function waitFor(cond, limit)
		for _ = 1, limit or 600 do
			if cond() then
				return true
			end
			h.beat()
		end
		return cond()
	end

	local fresh = Instance.new("Sound")
	result.defaults = string.format(
		"%s,%s,%s,%s,%s,%s,%s,%s",
		fresh.Volume,
		fresh.PlaybackSpeed,
		fresh.RollOffMinDistance,
		fresh.RollOffMaxDistance,
		fresh.RollOffMode.Name,
		tostring(fresh.Looped),
		fresh.TimeLength,
		tostring(fresh.IsLoaded)
	)
	fresh.Volume = 20
	fresh.PlaybackSpeed = -1
	result.clamps = string.format("%s,%s", fresh.Volume, fresh.PlaybackSpeed)
	fresh:Destroy()

	-- Loading on the server.
	local long = sound(LONG)
	local loadedAt
	long.Loaded:Connect(function()
		loadedAt = frame
	end)
	local start = frame
	result.loaded_at_parent = tostring(long.IsLoaded)
	waitFor(function()
		return long.IsLoaded
	end)
	result.loaded = tostring(long.IsLoaded)
	result.loaded_event = tostring(loadedAt ~= nil)
	result._loaded_frames = loadedAt and loadedAt - start or -1
	result.time_length = h.round(long.TimeLength, 3)

	-- Playing on the server: does the clock run, does it end?
	local log = {}
	for _, name in { "Played", "Paused", "Resumed", "Stopped", "Ended", "DidLoop" } do
		long[name]:Connect(function(id)
			table.insert(log, name .. (id == LONG and "(id)" or ""))
		end)
	end
	h.beat()
	start = frame
	long:Play()
	table.insert(log, "play")
	result.playing_now = tostring(long.Playing) .. "," .. tostring(long.IsPlaying)
	h.beats(30)
	result.server_position_30 = h.round(long.TimePosition, 2)
	long:Pause()
	table.insert(log, "pause")
	local paused = long.TimePosition
	h.beats(10)
	result.server_pause_holds = tostring(long.TimePosition == paused)
	long:Resume()
	table.insert(log, "resume")
	h.beats(5)
	long.TimePosition = 2.5
	table.insert(log, "seek")
	result.server_seek = h.round(long.TimePosition, 2)
	local ended = waitFor(function()
		return not long.Playing
	end, 120)
	result.server_ends = tostring(ended)
	result.server_end_frames = ended and frame - start or -1
	result.server_after_end = string.format("%s,%s", tostring(long.Playing), h.round(long.TimePosition, 2))
	long:Play()
	h.beats(5)
	long:Stop()
	table.insert(log, "stop")
	result.server_stop_position = h.round(long.TimePosition, 2)
	h.beat()
	result.server_log = table.concat(log, " ")

	-- PlaybackSpeed and Looped on the server.
	local fast = sound(LONG, { PlaybackSpeed = 2 })
	waitFor(function()
		return fast.IsLoaded
	end)
	fast:Play()
	h.beats(30)
	result.server_speed2_30 = h.round(fast.TimePosition, 2)
	fast:Destroy()
	local loop = sound(SHORT, { Looped = true })
	local loops = 0
	loop.DidLoop:Connect(function()
		loops += 1
	end)
	waitFor(function()
		return loop.IsLoaded
	end)
	result.short_length = h.round(loop.TimeLength, 3)
	loop:Play()
	h.beats(90)
	result.server_loops = loops
	result.server_looped_playing = tostring(loop.Playing)
	loop:Destroy()

	-- Playing set before parenting, and a sound with no id.
	local auto = Instance.new("Sound")
	auto.SoundId = SHORT
	auto.Playing = true
	auto.Parent = workspace
	h.beats(2)
	result.autoplay = tostring(auto.IsPlaying)
	auto:Destroy()
	local empty = sound("")
	empty:Play()
	h.beats(10)
	result.empty = string.format("%s,%s,%s", tostring(empty.Playing), tostring(empty.IsLoaded), empty.TimeLength)
	empty:Destroy()

	-- The client.
	h.player()
	local short = sound(SHORT)
	local client_long = sound(LONG)
	h.beats(10)
	local seen = h.client("SoundPlayback", "play", short, client_long)
	for k, v in seen do
		result["client_" .. k] = v
	end
	result.server_sees_client_play = tostring(short.Playing)
	short:Destroy()
	client_long:Destroy()
	stepped:Disconnect()
	long:Destroy()
	h.beats(5)
	return result
end
