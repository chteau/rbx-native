-- workspace:Raycast against the query scene: what each ray hits with
-- default and filtered RaycastParams.
return function(h, origin)
	local scene = h.queryScene(origin)
	local byName = {}
	for _, part in scene do
		byName[part.Name] = part
	end
	local function params(props)
		local p = RaycastParams.new()
		for key, value in props or {} do
			p[key] = value
		end
		return p
	end
	local function ray(from, dir, props)
		return h.hit(workspace:Raycast(origin + from, dir, params(props)))
	end
	local z = Vector3.new(0, 0, 30)
	local down = Vector3.new(0, -20, 0)
	local r = {
		default = ray(Vector3.zero, z),
		no_params = h.hit(workspace:Raycast(origin, z)),
		respect = ray(Vector3.zero, z, { RespectCanCollide = true }),
		group = ray(Vector3.zero, z, { RespectCanCollide = true, CollisionGroup = "QueryMask" }),
		exclude = ray(Vector3.zero, z, {
			FilterType = Enum.RaycastFilterType.Exclude,
			FilterDescendantsInstances = { byName.Ghost, byName.Grouped },
		}),
		include = ray(Vector3.zero, z, {
			FilterType = Enum.RaycastFilterType.Include,
			FilterDescendantsInstances = { byName.Grouped, byName.Wall },
		}),
		ball = ray(Vector3.new(10, 10, 1), down),
		cylinder = ray(Vector3.new(-10, 10, 1), down),
		wedge = ray(Vector3.new(0, 10, -10), down),
		tilted = ray(Vector3.new(1, 20, 1), down),
		tilted_corner = ray(Vector3.new(2.5, 20, 2.5), down),
		inside = ray(Vector3.new(0, 0, 10), z),
		miss = ray(Vector3.zero, Vector3.new(0, -30, 0)),
		short = ray(Vector3.zero, Vector3.new(0, 0, 4)),
	}
	h.clear(scene)
	return r
end
