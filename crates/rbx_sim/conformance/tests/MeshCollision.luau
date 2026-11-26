-- A MeshPart made at runtime from an EditableMesh of two cubes offset
-- diagonally (a gap between them), once per CollisionFidelity: what a ray
-- down the gap and one onto a cube hit, and whether a small part in the
-- gap overlaps it. Box and Hull fill the gap, a decomposition doesn't.
return function(h, origin)
	local AssetService = game:GetService("AssetService")
	local function cube(mesh, min, max)
		local v = {}
		for i = 0, 7 do
			local x = if bit32.band(i, 4) ~= 0 then max.X else min.X
			local y = if bit32.band(i, 2) ~= 0 then max.Y else min.Y
			local z = if bit32.band(i, 1) ~= 0 then max.Z else min.Z
			v[i] = mesh:AddVertex(Vector3.new(x, y, z))
		end
		for _, q in { { 0, 1, 3, 2 }, { 4, 6, 7, 5 }, { 0, 4, 5, 1 }, { 2, 3, 7, 6 }, { 0, 2, 6, 4 }, { 1, 5, 7, 3 } } do
			mesh:AddTriangle(v[q[1]], v[q[2]], v[q[3]])
			mesh:AddTriangle(v[q[1]], v[q[3]], v[q[4]])
		end
	end
	local r = {}
	local parts = {}
	local ok, err = pcall(function()
		for i, name in { "Box", "Hull", "Default", "PreciseConvexDecomposition" } do
			local mesh = AssetService:CreateEditableMesh()
			cube(mesh, Vector3.new(-3, -1, -1), Vector3.new(-1, 1, 1))
			cube(mesh, Vector3.new(1, 1, -1), Vector3.new(3, 3, 1))
			local part = AssetService:CreateMeshPartAsync(Content.fromObject(mesh), {
				CollisionFidelity = Enum.CollisionFidelity[name],
			})
			local at = origin + Vector3.new((i - 1) * 20, 0, 0)
			part.Name = name
			part.Anchored = true
			part.Position = at
			part.Parent = workspace
			table.insert(parts, part)
			local only = RaycastParams.new()
			only.FilterType = Enum.RaycastFilterType.Include
			only.FilterDescendantsInstances = { part }
			local gap = workspace:Raycast(at + Vector3.new(0, 10, 0), Vector3.new(0, -20, 0), only)
			local onCube = workspace:Raycast(at + Vector3.new(2, 10, 0), Vector3.new(0, -20, 0), only)
			local key = string.lower(name)
			r[key .. "_size"] = h.vec(part.Size, 2)
			r[key .. "_gap"] = if gap then h.round(gap.Distance, 2) else -1
			r[key .. "_cube"] = if onCube then h.round(onCube.Distance, 2) else -1
			local probe = h.part({ Name = "Probe", Anchored = true, CanCollide = false, Size = Vector3.new(0.4, 0.4, 0.4), Position = at })
			local overlap = OverlapParams.new()
			overlap.FilterType = Enum.RaycastFilterType.Include
			overlap.FilterDescendantsInstances = { part }
			r[key .. "_overlap"] = #workspace:GetPartsInPart(probe, overlap)
			probe:Destroy()
		end
	end)
	if not ok then
		r.error = tostring(err)
	end
	h.clear(parts)
	return r
end
