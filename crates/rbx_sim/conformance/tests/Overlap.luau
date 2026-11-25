-- GetPartBoundsInBox, GetPartBoundsInRadius and GetPartsInPart against the
-- query scene, with OverlapParams. The probe at (3.5, 10, 3.5) is inside
-- Tilted's bounding box but outside its shape.
return function(h, origin)
	local scene = h.queryScene(origin)
	local byName = {}
	for _, part in scene do
		byName[part.Name] = part
	end
	local function params(props)
		local p = OverlapParams.new()
		for key, value in props or {} do
			p[key] = value
		end
		return p
	end
	local function box(at, size, props)
		return h.names(workspace:GetPartBoundsInBox(CFrame.new(origin + at), size, params(props)))
	end
	local function radius(at, r, props)
		return h.names(workspace:GetPartBoundsInRadius(origin + at, r, params(props)))
	end
	local probe = h.part({
		Name = "Probe",
		Anchored = true,
		CanCollide = false,
		Size = Vector3.new(1, 1, 1),
		Position = origin + Vector3.new(3.5, 10, 3.5),
	})
	local probeBall = h.part({
		Name = "ProbeBall",
		Anchored = true,
		CanCollide = false,
		Shape = Enum.PartType.Ball,
		Size = Vector3.new(2, 2, 2),
		Position = origin + Vector3.new(10, 2.5, 0),
	})
	local cornerProbe = h.part({
		Name = "CornerProbe",
		Anchored = true,
		CanCollide = false,
		Size = Vector3.new(1, 1, 1),
		Position = origin + Vector3.new(11.9, 1.9, 1.9),
	})
	local skipProbes = { probe, probeBall, cornerProbe }
	local wide = Vector3.new(40, 40, 40)
	local r = {
		box_touch = box(Vector3.new(0, 0, 5), Vector3.new(2, 2, 2), { FilterDescendantsInstances = skipProbes }),
		box_wide = box(Vector3.zero, wide, { FilterDescendantsInstances = skipProbes }),
		box_bounds = box(Vector3.new(3.5, 10, 3.5), Vector3.new(1, 1, 1), { FilterDescendantsInstances = skipProbes }),
		box_respect = box(Vector3.zero, wide, { FilterDescendantsInstances = skipProbes, RespectCanCollide = true }),
		box_group = box(Vector3.zero, wide, { FilterDescendantsInstances = skipProbes, CollisionGroup = "QueryMask" }),
		box_include = box(Vector3.zero, wide, {
			FilterType = Enum.RaycastFilterType.Include,
			FilterDescendantsInstances = { byName.Ball, byName.Cyl, byName.Hidden },
		}),
		box_max = #workspace:GetPartBoundsInBox(CFrame.new(origin), wide, params({ MaxParts = 2 })),
		radius = radius(Vector3.zero, 6, { FilterDescendantsInstances = skipProbes }),
		radius_bounds = radius(Vector3.new(3.5, 10, 3.5), 0.5, { FilterDescendantsInstances = skipProbes }),
		radius_corner = radius(Vector3.new(11.9, 1.9, 1.9), 0.3, { FilterDescendantsInstances = skipProbes }),
		in_part_bounds = h.names(workspace:GetPartsInPart(probe)),
		in_part_ball = h.names(workspace:GetPartsInPart(probeBall)),
		in_part_corner = h.names(workspace:GetPartsInPart(cornerProbe)),
		in_part_filtered = h.names(workspace:GetPartsInPart(probeBall, params({
			FilterType = Enum.RaycastFilterType.Include,
			FilterDescendantsInstances = { byName.Cyl },
		}))),
	}
	h.clear(skipProbes)
	h.clear(scene)
	return r
end
