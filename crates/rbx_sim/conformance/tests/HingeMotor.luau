-- A 2x2x2 block on a vertical HingeConstraint motor (3 rad/s, unlimited
-- torque): its spin and the hinge angle after 10 and 30 frames.
return function(h, origin)
	local pivot, wheel = h.rig(origin, { Name = "Wheel", CanCollide = false, Size = Vector3.new(2, 2, 2), Position = origin })
	local up = CFrame.fromMatrix(Vector3.zero, Vector3.yAxis, Vector3.xAxis)
	local hinge = h.new("HingeConstraint", {
		Attachment0 = h.attach(pivot, up),
		Attachment1 = h.attach(wheel, up),
		ActuatorType = Enum.ActuatorType.Motor,
		AngularVelocity = 3,
		MotorMaxTorque = math.huge,
	}, pivot)
	local r = {}
	for i = 1, 30 do
		h.beat()
		if i == 10 or i == 30 then
			r["spin_" .. i] = h.round(wheel.AssemblyAngularVelocity.Y, 3)
			r["angle_" .. i] = h.round(hinge.CurrentAngle, 2)
			r["drop_" .. i] = h.round(wheel.Position.Y - origin.Y, 3)
		end
	end
	h.clear({ pivot, wheel })
	return r
end
