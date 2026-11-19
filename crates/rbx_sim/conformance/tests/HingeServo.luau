-- The same rig as HingeMotor as a servo: target 90 degrees at 2 rad/s,
-- unlimited torque. CurrentAngle (degrees) after 15, 30, 60 and 90 frames.
return function(h, origin)
	local pivot, wheel = h.rig(origin, { Name = "Wheel", CanCollide = false, Size = Vector3.new(2, 2, 2), Position = origin })
	local up = CFrame.fromMatrix(Vector3.zero, Vector3.yAxis, Vector3.xAxis)
	local hinge = h.new("HingeConstraint", {
		Attachment0 = h.attach(pivot, up),
		Attachment1 = h.attach(wheel, up),
		ActuatorType = Enum.ActuatorType.Servo,
		TargetAngle = 90,
		AngularSpeed = 2,
		ServoMaxTorque = math.huge,
	}, pivot)
	local r = {}
	for i = 1, 90 do
		h.beat()
		if i == 15 or i == 30 or i == 60 or i == 90 then
			r["angle_" .. i] = h.round(hinge.CurrentAngle, 2)
		end
	end
	h.clear({ pivot, wheel })
	return r
end
