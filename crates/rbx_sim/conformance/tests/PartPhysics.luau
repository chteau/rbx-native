-- A part's physics helpers on a live server: the inertia behind
-- TorqueToAngularAcceleration/AngularAccelerationToTorque per shape and
-- for a welded assembly, GetVelocityAtPosition on a spinning part,
-- ApplyImpulseAtPosition's spin and push, IsGrounded through joints and
-- constraints, and collision groups that let a part fall through a floor.
return function(h, origin)
	local r = {}
	local made = {}
	local function part(props, class)
		local p = h.part(props, class)
		table.insert(made, p)
		return p
	end
	local function held(props, class)
		props.Anchored = true
		return part(props, class)
	end
	local function alpha(p, torque, spin)
		return h.try(function()
			return h.vec(p:TorqueToAngularAcceleration(torque, spin), 4)
		end)
	end

	-- Inertia per shape (anchored, so nothing moves while we ask)
	local block = held({ Name = "Block", Size = Vector3.new(2, 4, 6), Position = origin + Vector3.new(0, 50, 0) })
	local ball = held({ Name = "Ball", Shape = Enum.PartType.Ball, Size = Vector3.new(4, 4, 4), Position = origin + Vector3.new(10, 50, 0) })
	local cyl = held({ Name = "Cyl", Shape = Enum.PartType.Cylinder, Size = Vector3.new(6, 2, 2), Position = origin + Vector3.new(20, 50, 0) })
	local wedge = held({ Name = "Wedge", Size = Vector3.new(2, 4, 6), Position = origin + Vector3.new(30, 50, 0) }, "WedgePart")
	local unit = Vector3.new(100, 200, 300)
	r.alpha_block_anchored = alpha(block, unit)
	block.Anchored = false
	ball.Anchored = false
	cyl.Anchored = false
	wedge.Anchored = false
	for _, p in { block, ball, cyl, wedge } do
		p:SetNetworkOwner(nil)
	end
	r.alpha_block = alpha(block, unit)
	r.alpha_ball = alpha(ball, unit)
	r.alpha_cyl = alpha(cyl, unit)
	r.alpha_wedge = alpha(wedge, unit)
	r.alpha_spin = alpha(block, unit, Vector3.new(1, 2, 3))
	r.torque_block = h.try(function()
		return h.vec(block:AngularAccelerationToTorque(Vector3.new(1, 2, 3)), 3)
	end)
	r.torque_spin = h.try(function()
		return h.vec(block:AngularAccelerationToTorque(Vector3.new(1, 2, 3), Vector3.new(3, 2, 1)), 3)
	end)
	local turned = part({ Name = "Turned", Size = Vector3.new(2, 4, 6), CFrame = CFrame.new(origin + Vector3.new(40, 50, 0)) * CFrame.Angles(0, 0, math.rad(90)), Anchored = true })
	turned.Anchored = false
	turned:SetNetworkOwner(nil)
	r.alpha_turned = alpha(turned, unit)
	local arm = part({ Name = "Arm", Size = Vector3.new(4, 1, 1), Position = origin + Vector3.new(3, 50, 0), Anchored = true })
	arm.Anchored = false
	h.new("WeldConstraint", { Part0 = block, Part1 = arm }, block)
	r.alpha_welded = alpha(block, unit)
	r.alpha_welded_arm = alpha(arm, unit)
	for _, p in { block, ball, cyl, wedge, turned, arm } do
		p.Anchored = true
	end

	-- GetVelocityAtPosition on a free part spinning in the air
	local spinner = part({ Name = "Spinner", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(0, 300, 0) })
	spinner.AssemblyLinearVelocity = Vector3.new(1, 0, 0)
	spinner.AssemblyAngularVelocity = Vector3.new(0, 2, 0)
	r.vel_at = h.try(function()
		return h.vec(spinner:GetVelocityAtPosition(spinner.Position + Vector3.new(0, 0, 3)), 3)
	end)
	r.vel_at_far = h.try(function()
		return h.vec(spinner:GetVelocityAtPosition(spinner.Position + Vector3.new(5, 7, 0)), 3)
	end)
	spinner.Anchored = true
	r.vel_at_anchored = h.try(function()
		return h.vec(spinner:GetVelocityAtPosition(spinner.Position + Vector3.new(0, 0, 3)), 3)
	end)

	-- ApplyImpulseAtPosition: off-centre push on a resting-free part
	local pushed = part({ Name = "Pushed", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(10, 300, 0) })
	local mass = pushed:GetMass()
	pushed:ApplyImpulseAtPosition(Vector3.new(0, 0, mass * 10), pushed.Position + Vector3.new(1, 0, 0))
	h.beat()
	r.impulse_v = h.vec(pushed.AssemblyLinearVelocity, 2)
	r.impulse_w = h.vec(pushed.AssemblyAngularVelocity, 2)
	r.impulse_bad = h.try(function()
		pushed:ApplyImpulseAtPosition(Vector3.new(0, 1, 0))
	end)
	pushed.Anchored = true

	-- IsGrounded
	local ground = part({ Name = "Ground", Anchored = true, Size = Vector3.new(4, 1, 4), Position = origin + Vector3.new(0, 400, 0) })
	local welded = part({ Name = "Welded", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(0, 401, 0), Anchored = true })
	local hinged = part({ Name = "Hinged", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(3, 400, 0), Anchored = true })
	local roped = part({ Name = "Roped", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(0, 396, 0), Anchored = true })
	local loose = part({ Name = "Loose", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(6, 400, 0), Anchored = true })
	h.new("WeldConstraint", { Part0 = ground, Part1 = welded }, ground)
	h.new("HingeConstraint", { Attachment0 = h.attach(ground, CFrame.new(2, 0, 0)), Attachment1 = h.attach(hinged, CFrame.new(-1, 0, 0)) }, ground)
	h.new("RopeConstraint", { Attachment0 = h.attach(ground), Attachment1 = h.attach(roped), Length = 5 }, ground)
	for _, p in { welded, hinged, roped, loose } do
		p.Anchored = false
	end
	local show = tostring
	r.grounded = table.concat({
		show(h.try(function()
			return ground:IsGrounded()
		end)),
		show(h.try(function()
			return welded:IsGrounded()
		end)),
		show(h.try(function()
			return hinged:IsGrounded()
		end)),
		show(h.try(function()
			return roped:IsGrounded()
		end)),
		show(h.try(function()
			return loose:IsGrounded()
		end)),
	}, " ")
	for _, p in { welded, hinged, roped, loose } do
		p.Anchored = true
	end

	-- Collision groups decide whether a part lands
	local PhysicsService = game:GetService("PhysicsService")
	PhysicsService:RegisterCollisionGroup("PhysGhost")
	PhysicsService:CollisionGroupSetCollidable("PhysGhost", "Default", false)
	local floor = part({ Name = "GroupFloor", Anchored = true, Size = Vector3.new(20, 1, 20), Position = origin + Vector3.new(0, 500, 0) })
	local ghost = part({ Name = "Ghost", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(-3, 503, 0), CollisionGroup = "PhysGhost" })
	local solid = part({ Name = "Solid", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(3, 503, 0) })
	h.beats(30)
	r.group_ghost_y = h.round(ghost.Position.Y - origin.Y, 1) < 499
	r.group_solid_y = h.round(solid.Position.Y - origin.Y, 1)
	PhysicsService:CollisionGroupSetCollidable("PhysGhost", "Default", true)
	ghost.Anchored = true
	ghost.Position = origin + Vector3.new(-3, 503, 0)
	ghost.Anchored = false
	ghost:SetNetworkOwner(nil)
	h.beats(30)
	r.group_restored_y = h.round(ghost.Position.Y - origin.Y, 1)
	PhysicsService:UnregisterCollisionGroup("PhysGhost")

	h.clear(made)
	return r
end
