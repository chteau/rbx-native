-- AnimationTracks on an R15 rig's Animator, with the Rthro walk and jump
-- from the creator docs' catalog table (rbxassetid 2510202577 and
-- 2510197830): loading, Length, TimePosition and the fade-in weight every
-- beat, joint transforms at fixed beats, AdjustSpeed, AdjustWeight, Stop's
-- fade, a one-shot's Stopped/Ended order, two tracks by priority. Then a
-- KeyframeSequence of our own registered at runtime (if the server allows
-- it): KeyframeReached and marker order.
return function(h, origin)
	local floor = h.floor(origin)
	local clock = h.clock()
	local model, humanoid, root = h.character(origin, "R15", { WalkSpeed = 0 })
	root.Anchored = true
	local animator = Instance.new("Animator")
	animator.Parent = humanoid
	for _ = 1, 10 do
		clock.tick()
	end
	local result = {}
	local events = {}
	local mark = 0
	local function log(text)
		table.insert(events, string.format("%s@%d", text, clock.now() - mark))
	end
	local function load(id)
		local animation = Instance.new("Animation")
		animation.AnimationId = "rbxassetid://" .. id
		local track = animator:LoadAnimation(animation)
		local name = tostring(id)
		track.Stopped:Connect(function()
			log(name .. ":stopped")
		end)
		track.Ended:Connect(function()
			log(name .. ":ended")
		end)
		track.DidLoop:Connect(function()
			log(name .. ":loop")
		end)
		track.KeyframeReached:Connect(function(kf)
			log(name .. ":kf:" .. kf)
		end)
		return track
	end
	-- A joint's Transform as three numeric fields, degrees.
	local function euler(name, motor)
		local x, y, z = motor.Transform:ToEulerAnglesXYZ()
		result[name .. "_x"] = h.round(math.deg(x), 2)
		result[name .. "_y"] = h.round(math.deg(y), 2)
		result[name .. "_z"] = h.round(math.deg(z), 2)
	end
	local hip = model.LeftUpperLeg.LeftHip
	local knee = model.RightLowerLeg.RightKnee
	local waist = model.UpperTorso.Waist

	local walk = load(2510202577)
	mark = clock.now()
	result.length_now = h.round(walk.Length, 3)
	while walk.Length == 0 and clock.now() - mark < 600 do
		clock.tick()
	end
	result.length = h.round(walk.Length, 3)
	result.looped = walk.Looped
	result.priority = walk.Priority.Name
	result.speed = walk.Speed
	result.weight_before = walk.WeightCurrent
	walk:Play()
	result.playing_now = walk.IsPlaying
	result.weight_now = h.round(walk.WeightCurrent, 3)
	mark = clock.now()
	local tp, weight = {}, {}
	for i = 1, 40 do
		clock.tick()
		table.insert(tp, h.round(walk.TimePosition, 4))
		table.insert(weight, h.round(walk.WeightCurrent, 3))
		if i == 1 or i == 5 or i == 10 or i == 20 or i == 30 then
			euler("hip_" .. i, hip)
			euler("knee_" .. i, knee)
			euler("waist_" .. i, waist)
		end
	end
	result.tp_1 = tp[1]
	result.tp_40 = tp[40]
	result.weight_1 = weight[1]
	result.weight_3 = weight[3]
	result.weight_6 = weight[6]
	result.weight_10 = weight[10]
	walk:AdjustSpeed(2)
	result.speed_2 = walk.Speed
	local before = walk.TimePosition
	for _ = 1, 10 do
		clock.tick()
	end
	result.speed_2_advance = h.round((walk.TimePosition - before) % walk.Length, 3)
	walk:AdjustSpeed(1)
	walk:AdjustWeight(0.5)
	result.weight_target = walk.WeightTarget
	local adjust = {}
	for _ = 1, 10 do
		clock.tick()
		table.insert(adjust, h.round(walk.WeightCurrent, 3))
	end
	result.adjust_3 = adjust[3]
	result.adjust_10 = adjust[10]
	walk:AdjustWeight(1, 0)
	clock.tick()
	result.adjust_instant = h.round(walk.WeightCurrent, 3)

	-- Priority: an Action track over the Movement-priority walk.
	local jump = load(2510197830)
	while jump.Length == 0 and clock.now() - mark < 900 do
		clock.tick()
	end
	result.jump_length = h.round(jump.Length, 3)
	result.jump_looped = jump.Looped
	result.jump_priority = jump.Priority.Name
	jump.Priority = Enum.AnimationPriority.Action
	jump.Looped = false
	jump:Play(0)
	for _ = 1, 3 do
		clock.tick()
	end
	euler("both_hip", hip)
	result.playing = #animator:GetPlayingAnimationTracks()
	mark = clock.now()
	table.clear(events)
	while jump.IsPlaying and clock.now() - mark < 300 do
		clock.tick()
	end
	for _ = 1, 5 do
		clock.tick()
	end
	-- The jump's own events: the walk's depend on how long the jump took to
	-- load.
	local own = {}
	for _, event in events do
		if string.find(event, "^2510197830:") then
			table.insert(own, event)
		end
	end
	result.oneshot = table.concat(own, " ")
	result.oneshot_tp = h.round(jump.TimePosition, 3)

	-- Stop with the default fade.
	table.clear(events)
	mark = clock.now()
	walk:Stop()
	result.stop_playing_now = walk.IsPlaying
	local fade = {}
	for _ = 1, 12 do
		clock.tick()
		table.insert(fade, h.round(walk.WeightCurrent, 3))
	end
	result.stop_events = table.concat(events, " ")
	result.fade_3 = fade[3]
	result.fade_6 = fade[6]
	result.stop_playing = walk.IsPlaying
	-- Nothing poses the hip now: it keeps the last Transform written (how
	-- big depends on the walk's phase, so only whether it is the rest pose).
	result.rest_kept = hip.Transform ~= CFrame.identity
	result.playing_after = #animator:GetPlayingAnimationTracks()

	-- Our own KeyframeSequence: named keyframes A, B, C and a marker on B.
	local sequence = Instance.new("KeyframeSequence")
	sequence.Loop = false
	sequence.Priority = Enum.AnimationPriority.Action
	for i, name in { "A", "B", "C" } do
		local keyframe = Instance.new("Keyframe")
		keyframe.Name = name
		keyframe.Time = (i - 1) * 0.25
		local rootPose = Instance.new("Pose")
		rootPose.Name = "HumanoidRootPart"
		rootPose.Parent = keyframe
		local pose = Instance.new("Pose")
		pose.Name = "LowerTorso"
		pose.CFrame = CFrame.Angles(0, math.rad(30 * (i - 1)), 0)
		pose.Parent = rootPose
		if name == "B" then
			local marker = Instance.new("KeyframeMarker")
			marker.Name = "Mark"
			marker.Value = "hello"
			marker.Parent = keyframe
		end
		keyframe.Parent = sequence
	end
	local ok, id = pcall(function()
		return game:GetService("KeyframeSequenceProvider"):RegisterKeyframeSequence(sequence)
	end)
	result.register = ok and "ok" or "error"
	if ok then
		local animation = Instance.new("Animation")
		animation.AnimationId = id
		local okLoad, track = pcall(animator.LoadAnimation, animator, animation)
		result.register_load = okLoad and "ok" or "error"
		if okLoad then
			table.clear(events)
			track.KeyframeReached:Connect(function(kf)
				log("kf:" .. kf)
			end)
			track:GetMarkerReachedSignal("Mark"):Connect(function(value)
				log("marker:" .. tostring(value))
			end)
			track.Stopped:Connect(function()
				log("stopped")
			end)
			track.Ended:Connect(function()
				log("ended")
			end)
			mark = clock.now()
			track:Play(0)
			local root15 = model.LowerTorso.Root
			for i = 1, 60 do
				clock.tick()
				if i == 8 then
					euler("kfs_root_8", root15)
				end
			end
			result.kfs_length = h.round(track.Length, 3)
			result.kfs_events = table.concat(events, " ")
		end
	end
	model:Destroy()
	floor:Destroy()
	return result
end
