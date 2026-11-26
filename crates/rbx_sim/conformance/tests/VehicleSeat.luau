-- A VehicleSeat taken by an R15 (VehicleSeat:Sit): the seat's defaults, the
-- Occupant and state, what Humanoid:Move does to Throttle and Steer while
-- seated, and a jump off. The seat is anchored: only the readings matter.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(60, 2, 60))
	local seat = h.part({
		Size = Vector3.new(2, 1, 2),
		CFrame = CFrame.new(origin + Vector3.new(0, 0.5, 0)),
		Anchored = true,
	}, "VehicleSeat")
	local result = {}
	h.props(result, "seat", seat, { "MaxSpeed", "Torque", "TurnSpeed", "HeadsUpDisplay", "Throttle", "Steer" })
	local model, humanoid, root = h.character(origin + Vector3.new(6, 0, 0), "R15")
	h.beats(30)
	local clock = h.clock()
	local states = h.states(humanoid, clock)
	local function beats(n)
		for _ = 1, n do
			clock.tick()
		end
	end
	seat:Sit(humanoid)
	result.occupant_now = seat.Occupant == humanoid
	beats(10)
	result.sit = humanoid.Sit
	result.state = humanoid:GetState().Name
	result.root_above_seat = h.round(root.Position.Y - seat.Position.Y, 2)
	humanoid:Move(Vector3.new(0, 0, -1))
	beats(5)
	result.throttle_fwd = seat.Throttle
	result.throttle_float_fwd = h.round(seat.ThrottleFloat, 2)
	humanoid:Move(Vector3.new(1, 0, 0))
	beats(5)
	result.steer_right = seat.Steer
	result.throttle_right = seat.Throttle
	humanoid:Move(Vector3.zero)
	beats(5)
	result.throttle_stop = seat.Throttle
	result.steer_stop = seat.Steer
	humanoid.Jump = true
	beats(30)
	result.occupant_after_jump = tostring(seat.Occupant)
	result.sit_after_jump = humanoid.Sit
	result.states = table.concat(states, " ")
	model:Destroy()
	seat:Destroy()
	floor:Destroy()
	return result
end
