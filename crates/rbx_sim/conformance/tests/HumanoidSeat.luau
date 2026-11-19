-- Seats: Seat:Sit puts an R15 on a seat (the weld, where the root ends up,
-- the state), a jump gets it off; one dropped onto a seat sits on touch,
-- one dropped onto a disabled seat doesn't; Sit = true with no seat; a
-- VehicleSeat's Throttle and Steer from Move.
return function(h, origin)
	local floor = h.floor(origin)
	local clock = h.clock()
	local function seat(class, offset, props)
		local s = h.part({
			Name = class,
			Anchored = true,
			Size = Vector3.new(2, 1, 2),
			Position = origin + offset + Vector3.new(0, 0.5, 0),
		}, class)
		for key, value in props or {} do
			s[key] = value
		end
		return s
	end
	local result = {}
	local chair = seat("Seat", Vector3.new(-20, 0, 0))
	local sitter, humanoid, root = h.character(origin + Vector3.new(-20, 0, 10), "R15")
	local touchSeat = seat("Seat", Vector3.new(0, 0, 0))
	local dropped, droppedHumanoid = h.character(origin + Vector3.new(0, 4, 0), "R15")
	local offSeat = seat("Seat", Vector3.new(10, 0, 0), { Disabled = true })
	local refused, refusedHumanoid = h.character(origin + Vector3.new(10, 4, 0), "R15")
	local plain, plainHumanoid = h.character(origin + Vector3.new(20, 0, 0), "R15")
	local car = seat("VehicleSeat", Vector3.new(-20, 0, -20))
	local driver, driverHumanoid, driverRoot = h.character(origin + Vector3.new(-20, 0, -10), "R15")
	for _ = 1, 30 do
		clock.tick()
	end
	local log = h.states(humanoid, clock)
	local start = clock.now()
	chair:Sit(humanoid)
	result.sit_now = humanoid.Sit
	plainHumanoid.Sit = true
	car:Sit(driverHumanoid)
	for _ = 1, 10 do
		clock.tick()
	end
	result.sit = humanoid.Sit
	result.seat_part = humanoid.SeatPart and humanoid.SeatPart.Name or "nil"
	result.occupant = chair.Occupant == humanoid
	result.state = humanoid:GetState().Name
	local weld = chair:FindFirstChild("SeatWeld")
	result.weld = weld and string.format(
		"%s %s>%s c0=%s c1=%s",
		weld.ClassName,
		weld.Part0 and weld.Part0.Name or "nil",
		weld.Part1 and weld.Part1.Name or "nil",
		h.vec(weld.C0.Position, 3),
		h.vec(weld.C1.Position, 3)
	) or "nil"
	result.offset = h.vec(chair.CFrame:ToObjectSpace(root.CFrame).Position, 3)
	result.offset_yaw = h.yaw(chair.CFrame:ToObjectSpace(root.CFrame))
	result.plain_sit = plainHumanoid.Sit
	result.plain_state = plainHumanoid:GetState().Name
	result.car_occupant = car.Occupant == driverHumanoid
	result.car_offset = h.vec(car.CFrame:ToObjectSpace(driverRoot.CFrame).Position, 3)
	driverHumanoid:Move(car.CFrame.LookVector)
	for _ = 1, 5 do
		clock.tick()
	end
	result.throttle = car.Throttle
	result.steer = car.Steer
	driverHumanoid:Move(car.CFrame.RightVector)
	for _ = 1, 5 do
		clock.tick()
	end
	result.throttle_right = car.Throttle
	result.steer_right = car.Steer
	humanoid.Jump = true
	for _ = 1, 10 do
		clock.tick()
	end
	result.jump_sit = humanoid.Sit
	result.jump_seat_part = humanoid.SeatPart and humanoid.SeatPart.Name or "nil"
	result.jump_occupant = chair.Occupant and "set" or "nil"
	result.jump_weld = chair:FindFirstChild("SeatWeld") and "kept" or "gone"
	for _ = 1, 60 do
		clock.tick()
	end
	result.touch_sit = droppedHumanoid.Sit
	result.touch_seat = touchSeat.Occupant == droppedHumanoid
	result.disabled_sit = refusedHumanoid.Sit
	result.after_state = humanoid:GetState().Name
	local entries = {}
	for _, entry in log do
		local old, new, beat = string.match(entry, "(%w+)>(%w+)@(%d+)")
		table.insert(entries, string.format("%s>%s@%d", old, new, tonumber(beat) - start))
	end
	result.states = table.concat(entries, " ")
	for _, thing in { sitter, dropped, refused, plain, driver, chair, touchSeat, offSeat, car, floor } do
		thing:Destroy()
	end
	return result
end
