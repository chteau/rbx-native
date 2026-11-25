-- EditableMesh attribute queries on a server: per-vertex and per-corner
-- lookups, the faces and vertices using an attribute, computed normals,
-- ResetNormal, MergeVertices, and what the surface queries hand back.
return function(h)
	local AssetService = game:GetService("AssetService")
	local r = {}
	local mesh = AssetService:CreateEditableMesh()
	local function list(ids)
		local out = {}
		for i, id in ids do
			out[i] = mesh:IdDebugString(id)
		end
		return table.concat(out, " ")
	end

	-- Two triangles folded along v2-v3, so the shared normal is an average.
	local v1 = mesh:AddVertex(Vector3.new(0, 0, 0))
	local v2 = mesh:AddVertex(Vector3.new(1, 0, 0))
	local v3 = mesh:AddVertex(Vector3.new(0, 0, 1))
	local v4 = mesh:AddVertex(Vector3.new(1, 1, 1))
	local f1 = mesh:AddTriangle(v1, v3, v2)
	local f2 = mesh:AddTriangle(v2, v3, v4)
	r.shared_normal = h.vec(mesh:GetNormal(mesh:GetVertexFaceNormal(v2, f1)), 4)
	r.lone_normal = h.vec(mesh:GetNormal(mesh:GetVertexFaceNormal(v1, f1)), 4)
	r.far_normal = h.vec(mesh:GetNormal(mesh:GetVertexFaceNormal(v4, f2)), 4)
	r.vertex_normals = list(mesh:GetVertexNormals(v2))
	r.vertex_uvs = list(mesh:GetVertexUVs(v3))
	r.vertex_colors = list(mesh:GetVertexColors(v4))
	r.vertex_faces = list(mesh:GetVertexFaces(v2))
	r.corner_same = mesh:GetVertexFaceUV(v2, f1) == mesh:GetVertexFaceUV(v2, f2)
	r.corner_not_on_face = h.try(function()
		return mesh:GetVertexFaceNormal(v1, f2)
	end)
	r.corner_bad_face = h.try(function()
		return mesh:GetVertexFaceColor(v1, v2)
	end)

	-- A seam: f2 gets its own color at v2.
	local red = mesh:AddColor(Color3.new(1, 0, 0), 0.25)
	r.set_vertex_face_color = h.try(function()
		mesh:SetVertexFaceColor(v2, f2, red)
		return h.show(mesh:GetColor(mesh:GetVertexFaceColor(v2, f2)))
	end)
	r.seam_colors = list(mesh:GetVertexColors(v2))
	r.faces_with_color = list(mesh:GetFacesWithColor(red))
	r.vertices_with_color = list(mesh:GetVerticesWithColor(red))
	r.faces_with_uv = list(mesh:GetFacesWithUV(mesh:GetVertexFaceUV(v3, f1)))
	r.vertices_with_uv = list(mesh:GetVerticesWithUV(mesh:GetVertexFaceUV(v3, f1)))
	r.faces_with_normal = list(mesh:GetFacesWithNormal(mesh:GetVertexFaceNormal(v2, f1)))
	r.vertices_with_normal = list(mesh:GetVerticesWithNormal(mesh:GetVertexFaceNormal(v2, f1)))
	r.faces_with_attribute_vertex = h.try(function()
		return list(mesh:GetFacesWithAttribute(v3))
	end)
	r.faces_with_attribute_color = h.try(function()
		return list(mesh:GetFacesWithAttribute(red))
	end)
	r.vertices_with_attribute_face = h.try(function()
		return list(mesh:GetVerticesWithAttribute(f2))
	end)
	r.faces_with_wrong_kind = h.try(function()
		return mesh:GetFacesWithColor(v1)
	end)
	r.unused_color_faces = h.try(function()
		return #mesh:GetFacesWithColor(mesh:AddColor(Color3.new(), 1))
	end)

	-- Normals: set, reset, a fresh computed one, one on no face.
	local n = mesh:GetVertexFaceNormal(v4, f2)
	r.set_normal = h.try(function()
		mesh:SetNormal(n, Vector3.new(0, 2, 0))
		return h.vec(mesh:GetNormal(n), 4)
	end)
	r.reset_normal = h.try(function()
		mesh:ResetNormal(n)
		return h.vec(mesh:GetNormal(n), 4)
	end)
	r.computed_unused = h.try(function()
		return h.show(mesh:GetNormal(mesh:AddNormal()))
	end)
	r.add_normal_given = h.vec(mesh:GetNormal(mesh:AddNormal(Vector3.new(3, 0, 4))), 4)
	r.set_vertex_face_normal = h.try(function()
		local sharp = mesh:AddNormal()
		mesh:SetVertexFaceNormal(v2, f2, sharp)
		return h.vec(mesh:GetNormal(sharp), 4) .. " " .. h.vec(mesh:GetNormal(mesh:GetVertexFaceNormal(v2, f1)), 4)
	end)
	r.set_vertex_face_uv = h.try(function()
		local uv = mesh:AddUV(Vector2.new(0.25, 0.75))
		mesh:SetVertexFaceUV(v3, f2, uv)
		return h.show(mesh:GetUV(mesh:GetVertexFaceUV(v3, f2))) .. " " .. h.show(mesh:GetUV(mesh:GetVertexFaceUV(v3, f1)))
	end)
	r.set_wrong_kind = h.try(function()
		mesh:SetVertexFaceUV(v3, f2, red)
	end)
	r.color_alpha_set = h.try(function()
		mesh:SetColorAlpha(red, 0.3)
		return mesh:GetColorAlpha(red)
	end)
	r.color_alpha_clamp = h.try(function()
		mesh:SetColorAlpha(red, 2)
		return mesh:GetColorAlpha(red)
	end)
	r.uv_out_of_range = h.try(function()
		local uv = mesh:AddUV(Vector2.new(-3, 7.5))
		return h.show(mesh:GetUV(uv))
	end)

	-- The surface queries' full returns.
	r.raycast = h.try(function()
		local face, point, bary, a, b, c = mesh:RaycastLocal(Vector3.new(0.25, 5, 0.25), Vector3.new(0, -10, 0))
		return string.format(
			"%s %s %s %s",
			face and mesh:IdDebugString(face) or "nil",
			point and h.vec(point, 3) or "nil",
			bary and h.vec(bary, 3) or "nil",
			a and list({ a, b, c }) or "nil"
		)
	end)
	r.raycast_short = h.try(function()
		return select("#", mesh:RaycastLocal(Vector3.new(0.25, 5, 0.25), Vector3.new(0, -1, 0)))
	end)
	r.raycast_back = h.try(function()
		local face = mesh:RaycastLocal(Vector3.new(0.25, -5, 0.25), Vector3.new(0, 10, 0))
		return face and mesh:IdDebugString(face) or "nil"
	end)
	r.closest = h.try(function()
		local face, point, bary, a, b, c = mesh:FindClosestPointOnSurface(Vector3.new(2, 2, 2))
		return string.format("%s %s %s %s", mesh:IdDebugString(face), h.vec(point, 3), h.vec(bary, 3), list({ a, b, c }))
	end)
	r.closest_empty = h.try(function()
		return select("#", AssetService:CreateEditableMesh():FindClosestPointOnSurface(Vector3.zero))
	end)
	r.closest_vertex_empty = h.try(function()
		return AssetService:CreateEditableMesh():FindClosestVertex(Vector3.zero)
	end)
	r.adjacent_faces = list(mesh:GetAdjacentFaces(f1))
	r.adjacent_vertices = list(mesh:GetAdjacentVertices(v2))
	r.empty_size = h.show(AssetService:CreateEditableMesh():GetSize())

	-- A quad: what the queries say about its corners.
	local quad = AssetService:CreateEditableMesh()
	local q = {}
	for i, p in { Vector3.new(0, 0, 0), Vector3.new(0, 0, 2), Vector3.new(2, 0, 2), Vector3.new(2, 0, 0) } do
		q[i] = quad:AddVertex(p)
	end
	local qf = quad:AddFace(q[1], q[2], q[3], q[4])
	r.quad_ray = h.try(function()
		local face, point, bary, a, b, c = quad:RaycastLocal(Vector3.new(1.5, 1, 0.5), Vector3.new(0, -2, 0))
		local corners = {}
		for i, id in { a, b, c } do
			corners[i] = table.find(q, id) or 0
		end
		return string.format("%s %s %s %s", tostring(face == qf), h.vec(point, 3), h.vec(bary, 3), table.concat(corners, ","))
	end)
	r.quad_normal = h.vec(quad:GetNormal(quad:GetFaceNormals(qf)[1]), 4)
	r.quad_adjacent = #quad:GetAdjacentVertices(q[1])

	-- MergeVertices: two triangles with coincident corners.
	local loose = AssetService:CreateEditableMesh()
	local a1 = loose:AddVertex(Vector3.new(0, 0, 0))
	local b1 = loose:AddVertex(Vector3.new(1, 0, 0))
	local c1 = loose:AddVertex(Vector3.new(0, 0, 1))
	local b2 = loose:AddVertex(Vector3.new(1.0001, 0, 0))
	local c2 = loose:AddVertex(Vector3.new(0, 0, 1))
	local d2 = loose:AddVertex(Vector3.new(1, 0, 1))
	local lf2 = loose:AddTriangle(b2, c2, d2)
	loose:AddTriangle(a1, c1, b1)
	r.merge = h.try(function()
		local map = loose:MergeVertices(0.001)
		local pairs_ = {}
		for from, to in map do
			table.insert(pairs_, loose:IdDebugString(from) .. ">" .. loose:IdDebugString(to))
		end
		table.sort(pairs_)
		return table.concat(pairs_, " ")
	end)
	r.merge_after = h.try(function()
		local names = {}
		for i, id in loose:GetFaceVertices(lf2) do
			names[i] = loose:IdDebugString(id)
		end
		return #loose:GetVertices() .. " " .. table.concat(names, " ")
	end)
	r.merge_tight = h.try(function()
		local m = AssetService:CreateEditableMesh()
		m:AddVertex(Vector3.zero)
		m:AddVertex(Vector3.new(0.01, 0, 0))
		local count = 0
		for _ in m:MergeVertices(0.001) do
			count += 1
		end
		return count
	end)
	r.merge_zero = h.try(function()
		local m = AssetService:CreateEditableMesh()
		m:AddVertex(Vector3.zero)
		m:AddVertex(Vector3.zero)
		local count = 0
		for _ in m:MergeVertices(0) do
			count += 1
		end
		return count .. " " .. #m:GetVertices()
	end)

	-- How a computed normal weighs the faces around it: one vertex shared
	-- by a flat face and a tilted one, the tilted face's angle at the vertex
	-- and its area varied separately.
	local function weighted(tilt, far)
		local m = AssetService:CreateEditableMesh()
		local o = m:AddVertex(Vector3.new(0, 0, 0))
		local x = m:AddVertex(Vector3.new(1, 0, 0))
		local z = m:AddVertex(Vector3.new(0, 0, -1))
		local t = m:AddVertex(tilt)
		local flat = m:AddTriangle(o, x, z)
		m:AddTriangle(o, t, x)
		if far then
			m:AddTriangle(o, z, m:AddVertex(far))
		end
		return h.vec(m:GetNormal(m:GetVertexFaceNormal(o, flat)), 4)
	end
	r.weight_a = weighted(Vector3.new(0, 1, 0))
	r.weight_b = weighted(Vector3.new(0, 3, 0))
	r.weight_c = weighted(Vector3.new(0, 1, 1))
	r.weight_d = weighted(Vector3.new(-1, 1, 0))
	r.weight_e = weighted(Vector3.new(0, 1, 0), Vector3.new(-1, 0, 0))
	r.weight_f = weighted(Vector3.new(0.2, 2, 0))
	return r
end
