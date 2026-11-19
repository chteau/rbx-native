-- TweenService at run time on a server: where in the frame a playing tween
-- writes, PlaybackState through Play, Pause, Cancel and the end,
-- Completed's timing and argument, repeats with reverses and a delay, a
-- second tween on the same property, every tweenable type halfway, the
-- errors, and a server tween as a client sees it. Halfway values are
-- measured against a NumberValue tweened 0..1 alongside them, so a real
-- server's uneven frames cancel out. Needs a player for the client half.
return function(h, origin)
	local TweenService = game:GetService("TweenService")
	local RunService = game:GetService("RunService")
	local result = {}
	local cleanup = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local function number(v)
		local x = Instance.new("NumberValue")
		x.Value = v or 0
		x.Parent = workspace
		table.insert(cleanup, x)
		return x
	end
	local function linear(t, ...)
		return TweenInfo.new(t, Enum.EasingStyle.Linear, Enum.EasingDirection.Out, ...)
	end
	local frame, beaten = 0, false
	local counters = {
		RunService.Stepped:Connect(function()
			frame += 1
			beaten = false
		end),
		RunService.Heartbeat:Connect(function()
			beaten = true
		end),
	}
	local function waitFor(cond, limit)
		for _ = 1, limit or 240 do
			if cond() then
				return true
			end
			h.beat()
		end
		return cond()
	end

	-- Where in the frame a playing tween writes, and its first value.
	local v = number(0)
	local marks, last = {}, 0
	local letters = {
		PreAnimation = "A",
		Stepped = "S",
		PreSimulation = "P",
		PostSimulation = "Q",
		Heartbeat = "H",
	}
	local watch = {}
	for name, letter in letters do
		table.insert(
			watch,
			RunService[name]:Connect(function()
				local changed = v.Value ~= last
				last = v.Value
				table.insert(marks, letter .. (changed and "+" or "-"))
			end)
		)
	end
	h.beat()
	marks = {}
	local t = TweenService:Create(v, linear(1), { Value = 60 })
	result.state_created = t.PlaybackState.Name
	result.tween = string.format(
		"%s,%s,%s,%s,%s",
		t.ClassName,
		t.Name,
		tostring(t.Instance == v),
		tostring(t:IsA("TweenBase")),
		tostring(t.Parent)
	)
	result.tween_info = string.format("%s,%s,%s", t.TweenInfo.Time, t.TweenInfo.EasingStyle.Name, t.TweenInfo.EasingDirection.Name)
	t:Play()
	result.state_played = t.PlaybackState.Name
	result.value_at_play = v.Value
	table.insert(marks, "play")
	h.beats(3)
	for _, c in watch do
		c:Disconnect()
	end
	result.phase = table.concat(marks, " ")
	result.after_3 = h.round(v.Value, 1)
	t:Cancel()

	-- Completed: when, with what, and before or after that frame's Heartbeat.
	local v2 = number(0)
	local t2 = TweenService:Create(v2, linear(0.5), { Value = 1 })
	local done, doneFrame
	t2.Completed:Connect(function(state)
		done = string.format(
			"%s,%s,%s,%s",
			state.Name,
			t2.PlaybackState.Name,
			beaten and "after_heartbeat" or "before_heartbeat",
			tostring(v2.Value)
		)
		doneFrame = frame
	end)
	h.beat()
	local start = frame
	t2:Play()
	waitFor(function()
		return done
	end)
	result.completed = done
	result.completed_frames = doneFrame and doneFrame - start or -1
	h.beat()
	result.state_completed = t2.PlaybackState.Name

	-- Pause, then Play again: it carries on from where it was.
	local v3 = number(0)
	local t3 = TweenService:Create(v3, linear(1), { Value = 1 })
	local f3
	t3.Completed:Connect(function()
		f3 = frame
	end)
	h.beat()
	local s3 = frame
	t3:Play()
	h.beats(15)
	t3:Pause()
	result.state_paused = t3.PlaybackState.Name
	local at = v3.Value
	h.beats(15)
	result.paused_holds = tostring(v3.Value == at)
	result.paused_at = h.round(at, 2)
	t3:Play()
	result.state_resumed = t3.PlaybackState.Name
	waitFor(function()
		return f3
	end)
	result.pause_total_frames = f3 and f3 - s3 or -1

	-- Cancel: the value stays, then Play starts it again.
	local v4 = number(0)
	local t4 = TweenService:Create(v4, linear(1), { Value = 1 })
	local log4 = {}
	t4.Completed:Connect(function(state)
		table.insert(log4, state.Name)
	end)
	h.beat()
	t4:Play()
	h.beats(15)
	t4:Cancel()
	table.insert(log4, "cancel")
	local cancelled = v4.Value
	result.state_cancelled = t4.PlaybackState.Name
	h.beats(5)
	result.cancel_holds = tostring(v4.Value == cancelled)
	result.cancelled_at = h.round(cancelled, 2)
	t4:Play()
	table.insert(log4, "play")
	h.beats(15)
	result.replay_after_cancel = h.round(v4.Value, 2)
	waitFor(function()
		return t4.PlaybackState == Enum.PlaybackState.Completed
	end)
	h.beat()
	result.cancel_log = table.concat(log4, ",")

	-- Repeats, reversing, with a delay: how long, and the states on the way.
	local v5 = number(0)
	local t5 = TweenService:Create(v5, linear(0.25, 2, true, 0.25), { Value = 1 })
	local f5
	t5.Completed:Connect(function()
		f5 = frame
	end)
	h.beat()
	local s5 = frame
	t5:Play()
	result.state_delay_now = t5.PlaybackState.Name
	local states, trace, peak, reversed = {}, {}, 0, false
	for i = 1, 200 do
		local s = t5.PlaybackState.Name
		if states[#states] ~= s then
			table.insert(states, s)
		end
		if v5.Value < peak - 0.05 then
			reversed = true
		end
		peak = math.max(peak, v5.Value)
		if i % 5 == 0 then
			table.insert(trace, string.format("%s%.2f", string.sub(s, 1, 1), v5.Value))
		end
		if f5 then
			break
		end
		h.beat()
	end
	result.repeat_states = table.concat(states, ",")
	result.repeat_reversed = tostring(reversed)
	result.repeat_frames = f5 and f5 - s5 or -1
	result.repeat_end_value = h.round(v5.Value, 3)
	result._repeat_trace = table.concat(trace, " ")

	-- A second tween on the same property takes over.
	local v6 = number(0)
	local a = TweenService:Create(v6, linear(1), { Value = 1 })
	local b = TweenService:Create(v6, linear(1), { Value = -1 })
	local log6 = {}
	a.Completed:Connect(function(state)
		table.insert(log6, "a:" .. state.Name)
	end)
	b.Completed:Connect(function(state)
		table.insert(log6, "b:" .. state.Name)
	end)
	h.beat()
	a:Play()
	h.beats(15)
	local mid = v6.Value
	b:Play()
	table.insert(log6, "b_play")
	result.older_state_now = a.PlaybackState.Name
	h.beats(15)
	result.older_state = a.PlaybackState.Name
	result.newer_progress = h.round((v6.Value - mid) / (-1 - mid), 2)
	waitFor(function()
		return b.PlaybackState == Enum.PlaybackState.Completed
	end)
	h.beat()
	result.same_property_log = table.concat(log6, ",")
	result.newer_end = tostring(v6.Value)

	-- Overlapping on one of two properties: what happens to the other.
	local overlap = h.part({ Anchored = true, Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(0, 20, 0) })
	table.insert(cleanup, overlap)
	local both = TweenService:Create(overlap, linear(1), { Transparency = 1, Reflectance = 1 })
	local one = TweenService:Create(overlap, linear(1), { Transparency = 0 })
	h.beat()
	both:Play()
	h.beats(15)
	one:Play()
	h.beats(5)
	local reflect = overlap.Reflectance
	h.beats(5)
	result.overlap = string.format("%s,%s", both.PlaybackState.Name, tostring(overlap.Reflectance > reflect))
	waitFor(function()
		return one.PlaybackState == Enum.PlaybackState.Completed
	end, 120)
	both:Cancel()

	-- Every tweenable type halfway, against a 0..1 reference.
	local ref = number(0)
	local part = h.part({
		Anchored = true,
		Size = Vector3.new(2, 2, 2),
		CFrame = CFrame.new(origin),
		Transparency = 0,
	})
	local folder = Instance.new("Folder")
	folder.Parent = workspace
	local gui = Instance.new("Frame")
	gui.Position = UDim2.new(0, 0, 0, 0)
	gui.AnchorPoint = Vector2.new(0, 0)
	gui.BackgroundColor3 = Color3.new(0, 0, 0)
	gui.Visible = false
	gui.Rotation = 0
	gui.Parent = folder
	local corner = Instance.new("UICorner")
	corner.CornerRadius = UDim.new(0, 0)
	corner.Parent = gui
	local image = Instance.new("ImageLabel")
	image.SliceCenter = Rect.new(0, 0, 0, 0)
	image.Parent = folder
	local flag = Instance.new("BoolValue")
	flag.Parent = folder
	local color = Instance.new("Color3Value")
	color.Value = Color3.new(0, 0, 0)
	color.Parent = folder
	local vec = Instance.new("Vector3Value")
	vec.Parent = folder
	table.insert(cleanup, part)
	table.insert(cleanup, folder)
	local goalCFrame = CFrame.new(origin + Vector3.new(10, 0, 0)) * CFrame.Angles(0, math.rad(90), 0)
	local plays = {
		{ ref, { Value = 1 } },
		{ part, { CFrame = goalCFrame, Size = Vector3.new(4, 6, 8), Transparency = 1 } },
		{
			gui,
			{
				Position = UDim2.new(1, 1000, 0.5, 500),
				AnchorPoint = Vector2.new(1, 0.5),
				BackgroundColor3 = Color3.new(1, 0.5, 0.25),
				Rotation = 90,
				Visible = true,
			},
		},
		{ corner, { CornerRadius = UDim.new(0.5, 2000) } },
		{ image, { SliceCenter = Rect.new(10, 20, 30, 40) } },
		{ flag, { Value = true } },
		{ color, { Value = Color3.new(1, 0.5, 0.25) } },
		{ vec, { Value = Vector3.new(-4, 8, 2) } },
	}
	local tweens = {}
	for _, entry in plays do
		table.insert(tweens, TweenService:Create(entry[1], linear(1), entry[2]))
	end
	h.beat()
	for _, tween in tweens do
		tween:Play()
	end
	local flipFlag, flipVisible
	for i = 1, 30 do
		h.beat()
		if flag.Value and not flipFlag then
			flipFlag = ref.Value
		end
		if gui.Visible and not flipVisible then
			flipVisible = ref.Value
		end
	end
	local f = ref.Value
	local function rel(x, from, to)
		return h.round((x - from) / (to - from) - f, 3)
	end
	result.halfway = h.round(f, 2)
	result.type_cframe_position = rel(part.Position.X, origin.X, origin.X + 10)
	local _, yaw = part.CFrame:ToOrientation()
	result.type_cframe_rotation = h.round(math.deg(yaw) / 90 - f, 3)
	result.type_vector3 = rel(part.Size.Y, 2, 6)
	result.type_number = rel(part.Transparency, 0, 1)
	result.type_udim2_offset = rel(gui.Position.X.Offset, 0, 1000)
	result.type_udim2_scale = rel(gui.Position.Y.Scale, 0, 0.5)
	result.type_vector2 = rel(gui.AnchorPoint.Y, 0, 0.5)
	result.type_color3_g = rel(gui.BackgroundColor3.G, 0, 0.5)
	result.type_color3_b = rel(color.Value.B, 0, 0.25)
	result.type_udim = rel(corner.CornerRadius.Offset, 0, 2000)
	result.type_rect = rel(image.SliceCenter.Max.X, 0, 30)
	result.type_vector3value = rel(vec.Value.Y, 0, 8)
	result.type_bool = string.format("%s,%s", flipFlag and "early" or "late", flipVisible and "early" or "late")
	result._bool_flip = string.format("%s,%s", tostring(flipFlag), tostring(flipVisible))
	waitFor(function()
		return tweens[1].PlaybackState == Enum.PlaybackState.Completed
	end)
	h.beat()
	result.final_exact = table.concat({
		tostring(part.CFrame == goalCFrame),
		tostring(part.Size == Vector3.new(4, 6, 8)),
		tostring(gui.Position == UDim2.new(1, 1000, 0.5, 500)),
		tostring(gui.BackgroundColor3 == Color3.new(1, 0.5, 0.25)),
		tostring(corner.CornerRadius == UDim.new(0.5, 2000)),
		tostring(image.SliceCenter == Rect.new(10, 20, 30, 40)),
		tostring(flag.Value),
		tostring(gui.Visible),
	}, ",")

	-- What can't be tweened.
	local info = linear(1)
	local emitter = Instance.new("ParticleEmitter")
	emitter.Parent = part
	result.err_string = err(function()
		TweenService:Create(part, info, { Name = "x" })
	end)
	result.err_missing = err(function()
		TweenService:Create(part, info, { Nope = 1 })
	end)
	result.err_wrong_type = err(function()
		TweenService:Create(part, info, { Transparency = "a" })
	end)
	result.err_no_instance = err(function()
		TweenService:Create(nil, info, { Value = 1 })
	end)
	result.err_number_range = err(function()
		TweenService:Create(emitter, info, { Lifetime = NumberRange.new(1, 2) })
	end)
	result.err_enum = err(function()
		TweenService:Create(part, info, { Material = Enum.Material.Wood })
	end)
	result.err_sequence = err(function()
		TweenService:Create(emitter, info, { Transparency = NumberSequence.new(1) })
	end)
	local d = TweenInfo.new()
	result.tween_info_default = string.format(
		"%s,%s,%s,%s,%s,%s",
		d.Time,
		d.EasingStyle.Name,
		d.EasingDirection.Name,
		d.RepeatCount,
		tostring(d.Reverses),
		d.DelayTime
	)

	-- A server tween as a client sees it.
	h.player()
	local moving = h.part({ Anchored = true, Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(0, 5, 0) })
	table.insert(cleanup, moving)
	h.beats(10)
	local slide = TweenService:Create(moving, linear(1), { Position = origin + Vector3.new(60, 5, 0) })
	slide:Play()
	local seen = h.client("TweenRuntime", "watch", moving, origin.X + 60)
	result.client_saw_between = tostring(seen.between)
	result.client_final = tostring(seen.final)
	result._client_steps = seen.steps
	local local_gui = h.client("TweenRuntime", "gui")
	result.client_gui_end = local_gui.ending
	result.client_phase = local_gui.phase

	for _, c in counters do
		c:Disconnect()
	end
	for _, x in cleanup do
		x:Destroy()
	end
	h.beats(5)
	return result
end
