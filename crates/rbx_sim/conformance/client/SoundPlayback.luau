-- SoundPlayback on the client, which really plays: loading, the clock,
-- when Ended fires for a short sound, PlaybackSpeed, a looped sound's
-- DidLoop.
local RunService = game:GetService("RunService")

return function(ch, step, short, long)
	local frame = 0
	local stepped = RunService.Heartbeat:Connect(function()
		frame += 1
	end)
	local function waitFor(cond, limit)
		for _ = 1, limit or 600 do
			if cond() then
				return true
			end
			ch.beat()
		end
		return cond()
	end
	local out = {}
	waitFor(function()
		return short.IsLoaded and long.IsLoaded
	end)
	out.loaded = tostring(short.IsLoaded) .. "," .. tostring(long.IsLoaded)
	out.length = string.format("%.3f,%.3f", short.TimeLength, long.TimeLength)

	local log = {}
	local endedAt, endedPos
	short.Ended:Connect(function()
		endedAt = frame
		endedPos = short.TimePosition
		table.insert(log, "Ended:" .. tostring(short.Playing))
	end)
	short.Played:Connect(function()
		table.insert(log, "Played")
	end)
	ch.beat()
	local start = frame
	short:Play()
	table.insert(log, "play")
	waitFor(function()
		return endedAt
	end, 120)
	out.ended_frames = endedAt and endedAt - start or -1
	out.ended_position = endedPos and math.floor(endedPos * 1000 + 0.5) / 1000 or -1
	out.after_end = string.format("%s,%.2f", tostring(short.Playing), short.TimePosition)
	out.log = table.concat(log, " ")

	long:Play()
	ch.beats(60)
	out.position_60 = math.floor(long.TimePosition * 100 + 0.5) / 100
	long.PlaybackSpeed = 2
	local before = long.TimePosition
	ch.beats(30)
	out.speed2_30 = math.floor((long.TimePosition - before) * 100 + 0.5) / 100
	long.PlaybackSpeed = 1
	long.Looped = true
	local loops = 0
	long.DidLoop:Connect(function()
		loops += 1
	end)
	long.TimePosition = 2.9
	ch.beats(30)
	out.loops = loops
	out.looped_position = math.floor(long.TimePosition * 100 + 0.5) / 100
	long:Stop()
	stepped:Disconnect()
	return out
end
