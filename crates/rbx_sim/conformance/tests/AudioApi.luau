-- The audio API with wires on a server: every node's pins and property
-- defaults, wiring and WiringChanged, the attenuation curves, a filter's
-- response, an analyzer and a player with nothing to hear.
local NODES = {
	AudioPlayer = { "AutoLoad", "AutoPlay", "IsPlaying", "IsReady", "Looping", "PlaybackSpeed", "TimeLength", "TimePosition", "Volume", "LoopRegion", "PlaybackRegion" },
	AudioEmitter = { "AudioInteractionGroup", "DistanceAttenuationBounds", "DistanceAttenuationMode", "PositionType", "SimulationFidelity" },
	AudioListener = { "AudioInteractionGroup", "PositionType", "SimulationFidelity" },
	AudioDeviceOutput = { "Player" },
	AudioDeviceInput = { "AccessType", "Active", "IsReady", "Muted", "Player", "Volume" },
	AudioEqualizer = { "Bypass", "HighGain", "LowGain", "MidGain", "MidRange" },
	AudioReverb = { "Bypass", "DecayRatio", "DecayTime", "Density", "Diffusion", "DryLevel", "EarlyDelayTime", "HighCutFrequency", "LateDelayTime", "LowShelfFrequency", "LowShelfGain", "ReferenceFrequency", "WetLevel" },
	AudioEcho = { "Bypass", "DelayTime", "DryLevel", "Feedback", "RampTime", "WetLevel" },
	AudioDistortion = { "Bypass", "Level" },
	AudioChorus = { "Bypass", "Depth", "Mix", "Rate" },
	AudioCompressor = { "Attack", "Bypass", "MakeupGain", "Ratio", "Release", "Threshold" },
	AudioFader = { "Bypass", "Volume" },
	AudioPitchShifter = { "Bypass", "Pitch", "WindowSize" },
	AudioFlanger = { "Bypass", "Depth", "Mix", "Rate" },
	AudioTremolo = { "Bypass", "Depth", "Duty", "Frequency", "Shape", "Skew", "Square" },
	AudioFilter = { "Bypass", "FilterType", "Frequency", "Gain", "Q" },
	AudioLimiter = { "Bypass", "MaxLevel", "Release" },
	AudioChannelMixer = { "Layout" },
	AudioChannelSplitter = { "Layout" },
	AudioAnalyzer = { "PeakLevel", "RmsLevel", "SpectrumEnabled", "WindowSize" },
	Wire = { "Connected", "SourceInstance", "SourceName", "TargetInstance", "TargetName" },
}

local function curve(t)
	local keys = {}
	for k in t do
		table.insert(keys, k)
	end
	table.sort(keys)
	local out = {}
	for _, k in keys do
		table.insert(out, string.format("%g=%g", k, t[k]))
	end
	return table.concat(out, " ")
end

return function(h)
	local r = {}
	local folder = h.new("Folder", { Name = "ConfAudio" })
	for class, props in NODES do
		local ok, node = pcall(Instance.new, class)
		if not ok then
			r[class] = "err: " .. tostring(node)
			continue
		end
		node.Parent = folder
		h.props(r, class, node, props)
		if class ~= "Wire" then
			r[class .. ".in"] = h.try(function()
				return table.concat(node:GetInputPins(), ",")
			end)
			r[class .. ".out"] = h.try(function()
				return table.concat(node:GetOutputPins(), ",")
			end)
		end
	end

	-- A chain: player -> fader -> emitter, wired after the fact.
	local player = h.new("AudioPlayer", {}, folder)
	local fader = h.new("AudioFader", { Volume = 0.5 }, folder)
	local emitter = h.new("AudioEmitter", {}, folder)
	local events = {}
	fader.WiringChanged:Connect(function(connected, pin, wire, other)
		table.insert(events, string.format("%s %s %s %s", tostring(connected), pin, typeof(wire), other and other.ClassName or "nil"))
	end)
	local loose = Instance.new("Wire")
	r.wire_loose = loose.Connected
	loose.SourceInstance = player
	r.wire_half = loose.Connected
	loose.TargetInstance = fader
	r.wire_unparented = loose.Connected
	loose.Parent = folder
	r.wire_parented = loose.Connected
	local second = h.new("Wire", { SourceInstance = fader, TargetInstance = emitter }, folder)
	h.beats(2)
	r.wire_second = second.Connected
	r.fader_in_wires = #fader:GetConnectedWires("Input")
	r.fader_out_wires = #fader:GetConnectedWires("Output")
	r.player_out_wires = #player:GetConnectedWires("Output")
	r.bad_pin = h.try(function()
		return fader:GetConnectedWires("Bogus")
	end)
	second.TargetName = "Nope"
	h.beats(2)
	r.wire_bad_target = second.Connected
	r.rename = h.try(function()
		second:RenameToDefault()
	end)
	second.TargetName = "Input"
	r.wire_renamed = second.TargetName
	h.beats(2)
	r.wire_renamed_connected = second.Connected
	loose:Destroy()
	h.beats(2)
	r.wiring_events = table.concat(events, " | ")
	r.wire_to_part = h.try(function()
		local w = h.new("Wire", { SourceInstance = workspace:FindFirstChildWhichIsA("BasePart") or folder, TargetInstance = fader }, folder)
		return w.Connected
	end)

	-- Attenuation curves.
	r.emitter_curve = h.try(function()
		return curve(emitter:GetDistanceAttenuation())
	end)
	r.emitter_curve_set = h.try(function()
		emitter:SetDistanceAttenuation({ [0] = 1, [50] = 0.5, [100] = 0 })
		return curve(emitter:GetDistanceAttenuation())
	end)
	r.emitter_curve_negative = h.try(function()
		emitter:SetDistanceAttenuation({ [-1] = 1 })
	end)
	r.emitter_curve_loud = h.try(function()
		emitter:SetDistanceAttenuation({ [10] = 2 })
	end)
	r.emitter_curve_string = h.try(function()
		emitter:SetDistanceAttenuation({ a = 1 })
	end)
	r.emitter_angle = h.try(function()
		return curve(emitter:GetAngleAttenuation())
	end)
	r.emitter_angle_big = h.try(function()
		emitter:SetAngleAttenuation({ [200] = 1 })
	end)
	local listener = h.new("AudioListener", {}, folder)
	r.listener_curve = h.try(function()
		return curve(listener:GetDistanceAttenuation())
	end)
	local near = h.part({ Anchored = true, Position = Vector3.new(2000, 300, 2000) })
	local far = h.part({ Anchored = true, Position = Vector3.new(2000, 300, 2025) })
	emitter.Parent = near
	listener.Parent = far
	h.beats(2)
	r.audibility = h.try(function()
		return emitter:GetAudibilityFor(listener)
	end)
	r.listener_audibility = h.try(function()
		return listener:GetAudibilityFor(emitter)
	end)
	r.interacting = h.try(function()
		return #emitter:GetInteractingListeners()
	end)
	r.emitter_cframe = h.try(function()
		return emitter:GetWorldCFrame().Position
	end)

	-- The filter's response.
	local filter = h.new("AudioFilter", {}, folder)
	for _, kind in Enum.AudioFilterType:GetEnumItems() do
		filter.FilterType = kind
		for _, hz in { 100, 1000, 2000, 8000 } do
			r["gain_" .. kind.Name .. "_" .. hz] = h.try(function()
				return filter:GetGainAt(hz)
			end)
		end
	end
	filter.FilterType = Enum.AudioFilterType.Peak
	filter.Gain = 6
	filter.Q = 2
	r.gain_peak6_2000 = h.try(function()
		return filter:GetGainAt(2000)
	end)
	r.gain_peak6_4000 = h.try(function()
		return filter:GetGainAt(4000)
	end)

	-- An analyzer on a silent server, a player with nothing loaded.
	local analyzer = h.new("AudioAnalyzer", {}, folder)
	h.new("Wire", { SourceInstance = player, TargetInstance = analyzer }, folder)
	r.spectrum = h.try(function()
		return #analyzer:GetSpectrum()
	end)
	r.play_empty = h.try(function()
		player:Play()
		return player.IsPlaying
	end)
	h.beats(5)
	r.empty_playing = player.IsPlaying
	r.empty_position = h.round(player.TimePosition, 2)
	r.analyzer_rms = analyzer.RmsLevel
	r.state = h.try(function()
		return player:GetPlaybackState()
	end)
	player:Stop()
	r.stopped = player.IsPlaying
	local sound = h.new("AudioPlayer", { Asset = "rbxassetid://12222216" }, folder)
	local t0 = os.clock()
	while not sound.IsReady and os.clock() - t0 < 10 do
		h.beat()
	end
	r.asset_ready = sound.IsReady
	r.asset_length = h.round(sound.TimeLength, 2)
	folder:Destroy()
	near:Destroy()
	far:Destroy()
	return r
end
