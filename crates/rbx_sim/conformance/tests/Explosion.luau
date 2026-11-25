-- An Explosion on a live server: which parts its Hit reports and at what
-- distance, the push it gives loose parts at a few distances and
-- pressures, the joints it breaks, what it does to a humanoid, and how
-- long the instance stays.
return function(h, origin)
	local r = {}
	local made = {}
	local function part(props)
		local p = h.part(props)
		table.insert(made, p)
		return p
	end
	local center = origin + Vector3.new(0, 600, 0)
	-- Loose parts in a row on an anchored shelf far below the blast centre
	-- line, so only the blast moves them.
	local loose = {}
	for i, d in { 2, 4, 8, 12, 20 } do
		loose[i] = part({ Name = "D" .. d, Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(d, 0, 0), Anchored = true })
	end
	local heavyPart = part({ Name = "Heavy", Size = Vector3.new(4, 4, 4), Position = center + Vector3.new(0, 0, 6), Anchored = true })
	local wall = part({ Name = "AnchoredWall", Size = Vector3.new(1, 4, 4), Position = center + Vector3.new(-5, 0, 0), Anchored = true })
	-- A welded pair inside the joint radius and one outside it.
	local inA = part({ Name = "InA", Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(0, 0, -3), Anchored = true })
	local inB = part({ Name = "InB", Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(0, 0, -4), Anchored = true })
	local outA = part({ Name = "OutA", Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(0, 0, -10), Anchored = true })
	local outB = part({ Name = "OutB", Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(0, 0, -11), Anchored = true })
	local inWeld = h.new("WeldConstraint", { Part0 = inA, Part1 = inB }, inA)
	local outWeld = h.new("WeldConstraint", { Part0 = outA, Part1 = outB }, outA)
	local inMotor = h.new("Motor6D", { Part0 = inA, Part1 = inB }, inB)
	local free = { inA, inB, outA, outB, heavyPart, table.unpack(loose) }
	for _, p in free do
		p.Anchored = false
	end
	for _, p in free do
		p:SetNetworkOwner(nil)
	end

	local hits = {}
	local explosion = Instance.new("Explosion")
	explosion.Position = center
	explosion.BlastRadius = 15
	explosion.BlastPressure = 500000
	explosion.DestroyJointRadiusPercent = 0.5
	explosion.Hit:Connect(function(p, distance)
		if p:IsDescendantOf(workspace) and (p.Position - center).Magnitude < 40 then
			table.insert(hits, p.Name .. "@" .. h.round(distance, 2))
		end
	end)
	local fresh = Instance.new("Explosion")
	r.defaults = table.concat({
		tostring(fresh.BlastRadius),
		tostring(fresh.BlastPressure),
		tostring(fresh.DestroyJointRadiusPercent),
		tostring(fresh.ExplosionType),
		tostring(fresh.TimeScale),
	}, " ")
	explosion.Parent = workspace
	r.hits_same_frame = #hits
	h.beat()
	table.sort(hits)
	r.hits = table.concat(hits, " ")
	for i, p in loose do
		r["v_" .. p.Name] = h.vec(p.AssemblyLinearVelocity, 1)
	end
	r.v_heavy = h.vec(heavyPart.AssemblyLinearVelocity, 1)
	r.w_heavy = h.vec(heavyPart.AssemblyAngularVelocity, 1)
	r.in_weld = inWeld.Parent ~= nil and inWeld.Enabled
	r.in_motor = inMotor.Parent ~= nil
	r.out_weld = outWeld.Parent ~= nil
	r.wall_anchored = wall.Anchored
	r.v_inA = h.vec(inA.AssemblyLinearVelocity, 1)
	r.parent_1 = explosion.Parent and explosion.Parent.Name or "nil"
	h.beats(10)
	for _, p in loose do
		r["moved_" .. p.Name] = h.vec(p.Position - center, 1)
	end
	for _, p in loose do
		p.Anchored = true
	end

	-- Low pressure: does a weak blast still break joints?
	local a = part({ Name = "WeakA", Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(0, 30, 0), Anchored = true })
	local b = part({ Name = "WeakB", Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(0, 31, 0), Anchored = true })
	local weakWeld = h.new("WeldConstraint", { Part0 = a, Part1 = b }, a)
	a.Anchored, b.Anchored = false, false
	a:SetNetworkOwner(nil)
	local weak = Instance.new("Explosion")
	weak.Position = center + Vector3.new(0, 30.5, 2)
	weak.BlastRadius = 4
	weak.BlastPressure = 0
	local weakHits = 0
	weak.Hit:Connect(function()
		weakHits += 1
	end)
	weak.Parent = workspace
	h.beat()
	r.weak_hits = weakHits
	r.weak_weld = weakWeld.Parent ~= nil
	r.weak_v = h.vec(a.AssemblyLinearVelocity, 1)
	a.Anchored, b.Anchored = true, true

	-- A humanoid in the blast
	local model, humanoid = h.character(center + Vector3.new(0, 60, 0), "R6")
	h.beats(5)
	local boom = Instance.new("Explosion")
	boom.Position = model.HumanoidRootPart.Position
	boom.BlastRadius = 6
	boom.Parent = workspace
	h.beats(3)
	r.humanoid_health = h.round(humanoid.Health, 1)
	r.humanoid_state = tostring(humanoid:GetState())
	r.humanoid_neck = model.Torso:FindFirstChild("Neck") ~= nil
	model:Destroy()

	-- NoJoints keeps joints
	local c = part({ Name = "KeepA", Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(0, 90, 0), Anchored = true })
	local d = part({ Name = "KeepB", Size = Vector3.new(1, 1, 1), Position = center + Vector3.new(0, 91, 0), Anchored = true })
	local keepWeld = h.new("WeldConstraint", { Part0 = c, Part1 = d }, c)
	c.Anchored, d.Anchored = false, false
	c:SetNetworkOwner(nil)
	local soft = Instance.new("Explosion")
	soft.Position = center + Vector3.new(0, 90.5, 1)
	soft.ExplosionType = Enum.ExplosionType.NoCraters
	soft.DestroyJointRadiusPercent = 0
	soft.Parent = workspace
	h.beat()
	r.nojoints_weld = keepWeld.Parent ~= nil
	c.Anchored, d.Anchored = true, true

	h.beats(60)
	r.parent_60 = explosion.Parent and explosion.Parent.Name or "nil"
	h.beats(120)
	r.parent_180 = explosion.Parent and explosion.Parent.Name or "nil"
	h.clear(made)
	return r
end
