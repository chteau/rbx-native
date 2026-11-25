-- Blockcast, Spherecast and Shapecast against the query scene.
return function(h, origin)
	local scene = h.queryScene(origin)
	local z = Vector3.new(0, 0, 30)
	local down = Vector3.new(0, -20, 0)
	local respect = RaycastParams.new()
	respect.RespectCanCollide = true
	local block = Vector3.new(2, 2, 2)
	local caster = h.part({
		Name = "Caster",
		Anchored = true,
		Shape = Enum.PartType.Ball,
		Size = Vector3.new(2, 2, 2),
		Position = origin + Vector3.new(-10, 10, 0),
	})
	local skipCaster = RaycastParams.new()
	skipCaster.FilterDescendantsInstances = { caster }
	local r = {
		block = h.hit(workspace:Blockcast(CFrame.new(origin), block, z)),
		block_respect = h.hit(workspace:Blockcast(CFrame.new(origin), block, z, respect)),
		block_start_inside = h.hit(workspace:Blockcast(CFrame.new(origin + Vector3.new(0, 0, 5)), block, z)),
		block_turned = h.hit(
			workspace:Blockcast(CFrame.new(origin + Vector3.new(10, 10, 0)) * CFrame.Angles(0, 0, math.rad(45)), block, down)
		),
		block_wedge = h.hit(workspace:Blockcast(CFrame.new(origin + Vector3.new(0, 10, -10)), block, down)),
		sphere = h.hit(workspace:Spherecast(origin, 1, z)),
		sphere_ball = h.hit(workspace:Spherecast(origin + Vector3.new(10, 10, 0.5), 1, down)),
		sphere_cylinder = h.hit(workspace:Spherecast(origin + Vector3.new(-10, 10, 1), 0.5, down)),
		shape_ball = h.hit(workspace:Shapecast(caster, down, skipCaster)),
		miss = h.hit(workspace:Spherecast(origin, 1, Vector3.new(0, -30, 0))),
	}
	caster:Destroy()
	h.clear(scene)
	return r
end
