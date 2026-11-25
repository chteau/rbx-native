-- A character on a moving platform: an unanchored raft carried at 8 stud/s
-- along X by a LinearVelocity, and an anchored lift moved by CFrame every
-- beat. How far each rider and platform go in 60 beats.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(160, 2, 160))
	local raft = h.part({ Name = "Raft", Size = Vector3.new(12, 1, 12), Position = origin + Vector3.new(-40, 4.5, -15) })
	local att = h.attach(raft)
	local carry = h.new("LinearVelocity", {
		Attachment0 = att,
		MaxForce = math.huge,
		VectorVelocity = Vector3.zero,
		RelativeTo = Enum.ActuatorRelativeTo.World,
	}, raft)
	h.new("AlignOrientation", {
		Attachment0 = att,
		Mode = Enum.OrientationAlignmentMode.OneAttachment,
		RigidityEnabled = true,
	}, raft)
	local lift = h.part({
		Name = "Lift",
		Anchored = true,
		Size = Vector3.new(12, 1, 12),
		Position = origin + Vector3.new(-40, 4.5, 15),
	})
	local rider, _, riderRoot = h.character(origin + Vector3.new(-40, 5, -15), "R15")
	local passenger, _, passengerRoot = h.character(origin + Vector3.new(-40, 5, 15), "R15")
	h.beats(40)
	local r0, p0, raft0, lift0 = riderRoot.Position, passengerRoot.Position, raft.Position, lift.Position
	carry.VectorVelocity = Vector3.new(8, 0, 0)
	local result = { _rider = {}, _raft = {} }
	for _ = 1, 60 do
		lift.CFrame = lift.CFrame + Vector3.new(8 / 60, 0, 0)
		h.beat()
		table.insert(result._rider, h.round(riderRoot.Position.X - r0.X, 2))
		table.insert(result._raft, h.round(raft.Position.X - raft0.X, 2))
	end
	result.raft_dx = h.round(raft.Position.X - raft0.X, 2)
	result.raft_dy = h.round(raft.Position.Y - raft0.Y, 2)
	result.rider_dx = h.round(riderRoot.Position.X - r0.X, 2)
	result.rider_vx = h.round(riderRoot.AssemblyLinearVelocity.X, 2)
	result.rider_dy = h.round(riderRoot.Position.Y - r0.Y, 2)
	result.lift_dx = h.round(lift.Position.X - lift0.X, 2)
	result.passenger_dx = h.round(passengerRoot.Position.X - p0.X, 2)
	result.passenger_dy = h.round(passengerRoot.Position.Y - p0.Y, 2)
	rider:Destroy()
	passenger:Destroy()
	raft:Destroy()
	lift:Destroy()
	floor:Destroy()
	return result
end
