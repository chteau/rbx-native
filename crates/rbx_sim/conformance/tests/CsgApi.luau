-- Solid modeling from scripts: BasePart:UnionAsync / SubtractAsync /
-- IntersectAsync and GeometryService's batch versions. What comes back
-- (class, name, parent, size, frame, mass, centre of mass, what it keeps
-- of the parts), the options, the error texts, whether the calls yield,
-- and what a union collides with.
return function(h, origin)
	local GS = game:GetService("GeometryService")
	local RunService = game:GetService("RunService")
	local r = {}
	local made = {}
	local function keep(x)
		if typeof(x) == "Instance" then
			table.insert(made, x)
		end
		return x
	end
	local function part(props, class)
		props.Anchored = if props.Anchored == nil then true else props.Anchored
		props.Position = origin + (props.Position or Vector3.zero)
		return keep(h.part(props, class))
	end
	local function describe(prefix, u)
		r[prefix .. "_class"] = u.ClassName
		r[prefix .. "_name"] = u.Name
		r[prefix .. "_parent"] = if u.Parent then u.Parent.Name else "nil"
		r[prefix .. "_size"] = h.vec(u.Size, 3)
		r[prefix .. "_pos"] = h.vec(u.Position - origin, 3)
		r[prefix .. "_rot"] = h.vec(u.Orientation, 2)
		r[prefix .. "_mass"] = h.round(u:GetMass(), 3)
		r[prefix .. "_com"] = h.vec(u.CenterOfMass, 3)
		r[prefix .. "_pivot"] = h.vec(u.PivotOffset.Position, 3)
	end
	local function section(name, f)
		local ok, err = pcall(f)
		if not ok then
			r[name .. "_error"] = (tostring(err):gsub("^.-:%d+: ", ""))
		end
	end
	local function try(f)
		return h.try(f)
	end

	-- An L: a 4x2x2 wooden red bar and a 2x2x2 blue cube on its right half.
	section("union", function()
		local a = part({
			Name = "Bar",
			Size = Vector3.new(4, 2, 2),
			Color = Color3.fromRGB(200, 30, 30),
			Material = Enum.Material.Wood,
			Transparency = 0.25,
			Reflectance = 0.1,
			CanCollide = false,
		})
		local b = part({
			Name = "Cube",
			Size = Vector3.new(2, 2, 2),
			Position = Vector3.new(1, 2, 0),
			Color = Color3.fromRGB(30, 30, 200),
			Material = Enum.Material.Metal,
		})
		local beats = 0
		local c = RunService.Heartbeat:Connect(function()
			beats += 1
		end)
		local u = keep(a:UnionAsync({ b }))
		c:Disconnect()
		r.union_yields = beats > 0
		describe("union", u)
		r.union_color = h.vec(Vector3.new(u.Color.R, u.Color.G, u.Color.B) * 255, 0)
		r.union_material = u.Material.Name
		r.union_transparency = h.round(u.Transparency, 3)
		r.union_reflectance = h.round(u.Reflectance, 3)
		r.union_anchored = u.Anchored
		r.union_cancollide = u.CanCollide
		r.union_usepartcolor = u.UsePartColor
		r.union_collision = u.CollisionFidelity.Name
		r.union_render = u.RenderFidelity.Name
		r.union_smoothing = h.round(u.SmoothingAngle, 3)
		r.union_sources_kept = a.Parent ~= nil and b.Parent ~= nil
		r.union_children = #u:GetChildren()
		u.Parent = workspace
		r.union_mass_parented = h.round(u:GetMass(), 3)
		r.union_assembly_mass = h.round(u.AssemblyMass, 3)
	end)

	-- The frame a union takes when the calling part is turned.
	section("rot", function()
		local a = part({ Size = Vector3.new(4, 2, 2), Orientation = Vector3.new(0, 30, 0) })
		local b = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(0, 1.5, 0) })
		describe("rot", keep(a:UnionAsync({ b })))
	end)

	-- A 4x4x4 block with a 2x2x2 corner cut away, and one bored through by a
	-- cylinder along X.
	section("sub", function()
		local a = part({ Size = Vector3.new(4, 4, 4), Material = Enum.Material.Concrete })
		local b = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(1, 1, 1) })
		describe("sub", keep(a:SubtractAsync({ b })))
		r.sub_material = a.Material.Name
	end)
	section("bore", function()
		local a = part({ Size = Vector3.new(4, 4, 4) })
		local b = part({ Size = Vector3.new(6, 2, 2), Shape = Enum.PartType.Cylinder })
		describe("bore", keep(a:SubtractAsync({ b })))
	end)

	-- Two 4x4x4 blocks overlapping in a 2x2x4 box.
	section("inter", function()
		local a = part({ Size = Vector3.new(4, 4, 4) })
		local b = part({ Size = Vector3.new(4, 4, 4), Position = Vector3.new(2, 2, 0) })
		describe("inter", keep(a:IntersectAsync({ b })))
	end)

	-- Fidelity arguments.
	section("fid", function()
		local a = part({ Size = Vector3.new(2, 2, 2) })
		local b = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(1, 0, 0) })
		local u = keep(a:UnionAsync({ b }, Enum.CollisionFidelity.Box, Enum.RenderFidelity.Performance))
		r.fid_collision = u.CollisionFidelity.Name
		r.fid_render = u.RenderFidelity.Name
		local s2 = keep(a:SubtractAsync({ b }, "Hull", "Precise"))
		r.fid_sub_collision = s2.CollisionFidelity.Name
		r.fid_sub_render = s2.RenderFidelity.Name
		r.fid_bad = try(function()
			return keep(a:UnionAsync({ b }, "Nope")).ClassName
		end)
	end)

	-- Errors.
	section("err", function()
		local a = part({ Size = Vector3.new(2, 2, 2) })
		local big = part({ Size = Vector3.new(4, 4, 4) })
		local far = part({ Size = Vector3.new(1, 1, 1), Position = Vector3.new(10, 0, 0) })
		r.err_empty = try(function()
			local u = keep(a:UnionAsync({}))
			return typeof(u) .. " " .. (if typeof(u) == "Instance" then u.ClassName else "")
		end)
		r.err_self = try(function()
			return keep(a:UnionAsync({ a })).ClassName
		end)
		r.err_model = try(function()
			return keep(a:UnionAsync({ Instance.new("Model") })).ClassName
		end)
		r.err_string = try(function()
			return keep(a:UnionAsync("x")).ClassName
		end)
		r.err_all_cut = try(function()
			return keep(a:SubtractAsync({ big })).ClassName
		end)
		r.err_disjoint = try(function()
			return keep(a:IntersectAsync({ far })).ClassName
		end)
		r.err_far_union = try(function()
			local u = keep(a:UnionAsync({ far }))
			return u.ClassName .. " " .. h.vec(u.Size, 3)
		end)
	end)

	-- GeometryService: SplitApart, options, the batch results.
	section("gs", function()
		local a = part({ Size = Vector3.new(2, 2, 2) })
		local c = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(6, 0, 0) })
		local beats = 0
		local conn = RunService.Heartbeat:Connect(function()
			beats += 1
		end)
		local split = GS:UnionAsync(a, { c })
		conn:Disconnect()
		r.gs_yields = beats > 0
		r.gs_split_type = typeof(split)
		r.gs_split_count = #split
		for i, u in split do
			keep(u)
			describe("gs_split" .. i, u)
		end
		local joined = GS:UnionAsync(a, { c }, { SplitApart = false })
		r.gs_joined_count = #joined
		for _, u in joined do
			keep(u)
			describe("gs_joined", u)
			r.gs_joined_collision = u.CollisionFidelity.Name
			r.gs_joined_render = u.RenderFidelity.Name
		end
		local opts = GS:UnionAsync(a, { c }, {
			CollisionFidelity = Enum.CollisionFidelity.Hull,
			RenderFidelity = Enum.RenderFidelity.Precise,
			SplitApart = false,
		})
		r.gs_opts_collision = opts[1].CollisionFidelity.Name
		r.gs_opts_render = opts[1].RenderFidelity.Name
		keep(opts[1])

		local bar = part({ Size = Vector3.new(6, 2, 2) })
		local cutter = part({ Size = Vector3.new(1, 4, 4) })
		local halves = GS:SubtractAsync(bar, { cutter })
		r.gs_sub_count = #halves
		for i, u in halves do
			keep(u)
			r["gs_sub" .. i .. "_size"] = h.vec(u.Size, 3)
			r["gs_sub" .. i .. "_pos"] = h.vec(u.Position - origin, 3)
			r["gs_sub" .. i .. "_class"] = u.ClassName
		end
		local b = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(1, 1, 0) })
		local inter = GS:IntersectAsync(a, { b })
		r.gs_inter_count = #inter
		describe("gs_inter", keep(inter[1]))

		r.gs_err_bad_option = try(function()
			return #GS:UnionAsync(a, { c }, { SplitApart = "yes" })
		end)
		r.gs_err_unknown_option = try(function()
			return #GS:UnionAsync(a, { c }, { Bogus = true })
		end)
		r.gs_err_empty = try(function()
			return #GS:UnionAsync(a, {})
		end)
		r.gs_err_not_part = try(function()
			return #GS:UnionAsync(workspace, { c })
		end)
		r.gs_err_all_cut = try(function()
			return #GS:SubtractAsync(a, { part({ Size = Vector3.new(5, 5, 5) }) })
		end)
		r.gs_err_disjoint = try(function()
			return #GS:IntersectAsync(a, { c })
		end)
	end)

	-- Voronoi fragments.
	local function shape(v)
		if typeof(v) == "Instance" then
			return v.ClassName .. ":" .. v.Name
		elseif type(v) == "table" then
			local keys = {}
			for k, x in v do
				table.insert(keys, tostring(k) .. "=" .. shape(x))
			end
			table.sort(keys)
			return "{" .. table.concat(keys, ",") .. "}"
		end
		return typeof(v)
	end
	section("frag", function()
		local block = part({ Name = "Block", Size = Vector3.new(4, 4, 4), Material = Enum.Material.Slate })
		local pieces = GS:FragmentAsync(block, { origin + Vector3.new(-1, 0, 0), origin + Vector3.new(1, 0, 0) })
		r.frag_count = #pieces
		r.frag_shape = shape(pieces)
		local total, n = 0, 0
		local function visit(v)
			if typeof(v) == "Instance" then
				keep(v)
				n += 1
				total += v:GetMass()
				r["frag" .. n .. "_class"] = v.ClassName
				r["frag" .. n .. "_size"] = h.vec(v.Size, 3)
				r["frag" .. n .. "_pos"] = h.vec(v.Position - origin, 3)
				r["frag" .. n .. "_com"] = h.vec(v.CenterOfMass, 3)
				r["frag" .. n .. "_material"] = v.Material.Name
				r["frag" .. n .. "_parent"] = if v.Parent then v.Parent.Name else "nil"
			elseif type(v) == "table" then
				for _, x in v do
					visit(x)
				end
			end
		end
		visit(pieces)
		r.frag_mass = h.round(total, 3)
		r.frag_block_parent = if block.Parent then block.Parent.Name else "nil"
	end)
	section("sites", function()
		local block = part({ Size = Vector3.new(4, 4, 4) })
		local sites = GS:GenerateFragmentSites(block)
		r.sites_type = typeof(sites)
		r.sites_count = #sites
		r.sites_entry = shape(sites[1])
		local lo, hi = Vector3.one * math.huge, -Vector3.one * math.huge
		for _, s in sites do
			lo, hi = lo:Min(s), hi:Max(s)
		end
		r.sites_near_world = (lo - block.Position).Magnitude < 10 and (hi - block.Position).Magnitude < 10
		r.sites_near_local = lo.Magnitude < 10 and hi.Magnitude < 10
		r.sites_inside = (function()
			for _, s in sites do
				local l = block.CFrame:PointToObjectSpace(s)
				if math.abs(l.X) > 2 or math.abs(l.Y) > 2 or math.abs(l.Z) > 2 then
					return false
				end
			end
			return true
		end)()
	end)

	-- Constraints a union should keep from the part it replaced.
	section("keep", function()
		local a = part({ Name = "Source", Size = Vector3.new(2, 2, 2) })
		local b = part({ Name = "Added", Size = Vector3.new(2, 2, 2), Position = Vector3.new(1, 0, 0) })
		local other = part({ Name = "Other", Size = Vector3.new(1, 1, 1), Position = Vector3.new(-2, 0, 0) })
		local weld = h.new("WeldConstraint", { Name = "Weld", Part0 = a, Part1 = other }, a)
		local att0 = h.attach(a, CFrame.new(0, 1, 0))
		att0.Name = "Att0"
		local att1 = h.attach(other)
		h.new("RopeConstraint", { Name = "Rope", Attachment0 = att0, Attachment1 = att1, Length = 3 }, a)
		local u = keep(a:UnionAsync({ b }))
		u.Name = "Result"
		u.Parent = workspace
		local list = GS:CalculateConstraintsToPreserve(a, { u })
		r.keep_count = #list
		local shapes = {}
		for _, entry in list do
			table.insert(shapes, shape(entry))
		end
		table.sort(shapes)
		r.keep_entries = table.concat(shapes, ";")
		r.keep_weld_part = weld.Part0 == a
		r.keep_att_parent = att0.Parent.Name
		local none = GS:CalculateConstraintsToPreserve(a, { u }, { weldConstraintPreserve = Enum.WeldConstraintPreserve.None })
		local shapes2 = {}
		for _, entry in none do
			table.insert(shapes2, shape(entry))
		end
		table.sort(shapes2)
		r.keep_none = table.concat(shapes2, ";")
		r.keep_err_option = try(function()
			return #GS:CalculateConstraintsToPreserve(a, { u }, { Bogus = 1 })
		end)
	end)

	-- GeometryService results keep the part's frame, rotated too.
	section("gsrot", function()
		local a = part({ Size = Vector3.new(4, 2, 2), Orientation = Vector3.new(0, 30, 0) })
		local b = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(0, 1.5, 0) })
		local out = GS:UnionAsync(a, { b })
		describe("gsrot", keep(out[1]))
	end)

	-- SubstituteGeometry: one operation takes another's shape.
	section("subst", function()
		local a = part({ Size = Vector3.new(4, 2, 2), Material = Enum.Material.Wood })
		local b = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(1, 2, 0) })
		local c = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(0, 0, 10) })
		local d = part({ Size = Vector3.new(2, 2, 2), Position = Vector3.new(1, 0, 10) })
		local target = keep(a:UnionAsync({ b }))
		local source = keep(c:UnionAsync({ d }))
		target.Position = origin + Vector3.new(0, 5, 0)
		target:SubstituteGeometry(source)
		describe("subst", target)
		r.subst_material = target.Material.Name
		r.subst_err_part = try(function()
			target:SubstituteGeometry(c)
		end)
		r.subst_part_size = h.vec(target.Size, 3)
		r.subst_part_mass = h.round(target:GetMass(), 3)
		r.subst_part_pos = h.vec(target.Position - origin, 3)
	end)

	-- What a hollow union collides with: two cubes and the gap between.
	section("hit", function()
		local results = {}
		for i, fidelity in { "Default", "Box" } do
			local at = Vector3.new(30 + i * 15, 0, 0)
			local a = part({ Size = Vector3.new(2, 2, 2), Position = at + Vector3.new(-2, 0, 0) })
			local b = part({ Size = Vector3.new(2, 2, 2), Position = at + Vector3.new(2, 0, 0) })
			local u = keep(a:UnionAsync({ b }, Enum.CollisionFidelity[fidelity]))
			u.Anchored = true
			u.Parent = workspace
			a.Parent = nil
			b.Parent = nil
			local only = RaycastParams.new()
			only.FilterType = Enum.RaycastFilterType.Include
			only.FilterDescendantsInstances = { u }
			local gap = workspace:Raycast(origin + at + Vector3.new(0, 10, 0), Vector3.new(0, -20, 0), only)
			local cube = workspace:Raycast(origin + at + Vector3.new(2, 10, 0), Vector3.new(0, -20, 0), only)
			local key = "hit_" .. string.lower(fidelity)
			r[key .. "_gap"] = if gap then h.round(gap.Distance, 2) else -1
			r[key .. "_cube"] = if cube then h.round(cube.Distance, 2) else -1
			results[i] = u
		end
	end)

	h.clear(made)
	return r
end
