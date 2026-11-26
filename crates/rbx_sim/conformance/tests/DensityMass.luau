-- CustomPhysicalProperties density: GetMass for light to heavy blocks,
-- AssemblyMass of a welded pair, then a light block sliding into a heavy
-- one and a heavy one into a light one (velocities after the hit).
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(120, 2, 40))
	local function block(density, position, size)
		return h.part({
			Size = size or Vector3.new(2, 2, 2),
			Position = origin + position,
			CustomPhysicalProperties = PhysicalProperties.new(density, 0, 0, 100, 100),
		})
	end
	local result = {}
	for i, d in { 0.01, 0.7, 7, 100 } do
		local b = block(d, Vector3.new(-40, 1, -10), Vector3.new(2, 1, 3))
		result["mass_" .. i] = h.round(b:GetMass(), 4)
		b:Destroy()
	end
	local a = block(1, Vector3.new(30, 1, 10))
	local b = block(3, Vector3.new(32, 1, 10))
	h.new("WeldConstraint", { Part0 = a, Part1 = b }, a)
	result.weld_assembly = h.round(a.AssemblyMass, 4)
	result.weld_mass = h.round(a:GetMass(), 4)
	a:Destroy()
	b:Destroy()
	local light1, heavy1 = block(0.7, Vector3.new(-20, 1, 0)), block(7, Vector3.new(-10, 1, 0))
	local heavy2, light2 = block(7, Vector3.new(10, 1, 5)), block(0.7, Vector3.new(20, 1, 5))
	h.beats(20)
	light1.AssemblyLinearVelocity = Vector3.new(30, 0, 0)
	heavy2.AssemblyLinearVelocity = Vector3.new(30, 0, 0)
	h.beats(30)
	result.light_into_heavy_a = h.round(light1.AssemblyLinearVelocity.X, 2)
	result.light_into_heavy_b = h.round(heavy1.AssemblyLinearVelocity.X, 2)
	result.heavy_into_light_a = h.round(heavy2.AssemblyLinearVelocity.X, 2)
	result.heavy_into_light_b = h.round(light2.AssemblyLinearVelocity.X, 2)
	h.clear({ light1, heavy1, heavy2, light2 })
	floor:Destroy()
	return result
end
