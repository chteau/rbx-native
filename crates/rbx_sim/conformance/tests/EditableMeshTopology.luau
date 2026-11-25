-- EditableMesh topology on a server: what an id looks like, which
-- attributes a vertex and a face bring, quads, Triangulate, SetFaceVertices,
-- Clear, what a fixed-size mesh refuses, the vertex and triangle limits.
return function(h)
	local AssetService = game:GetService("AssetService")
	local r = {}
	local function list(ids, mesh)
		local out = {}
		for i, id in ids do
			out[i] = mesh:IdDebugString(id)
		end
		return table.concat(out, " ")
	end

	local mesh = AssetService:CreateEditableMesh()
	local v1 = mesh:AddVertex(Vector3.new(0, 0, 0))
	local v2 = mesh:AddVertex(Vector3.new(1, 0, 0))
	local v3 = mesh:AddVertex(Vector3.new(1, 0, 1))
	local v4 = mesh:AddVertex(Vector3.new(0, 0, 1))
	r.id_vertex = v1
	r.id_vertex2 = v2
	r.id_debug_vertex = mesh:IdDebugString(v1)
	r.after_vertices = string.format(
		"%d %d %d %d",
		#mesh:GetVertices(),
		#mesh:GetNormals(),
		#mesh:GetUVs(),
		#mesh:GetColors()
	)
	r.debug_bad = h.try(function()
		return mesh:IdDebugString(123456)
	end)
	local quad = mesh:AddFace(v1, v4, v3, v2)
	r.id_face = quad
	r.id_debug_face = mesh:IdDebugString(quad)
	r.after_quad = string.format(
		"%d %d %d %d %d",
		#mesh:GetFaces(),
		#mesh:GetVertices(),
		#mesh:GetNormals(),
		#mesh:GetUVs(),
		#mesh:GetColors()
	)
	r.quad_vertices = list(mesh:GetFaceVertices(quad), mesh)
	r.quad_normals = list(mesh:GetFaceNormals(quad), mesh)
	r.quad_uvs = list(mesh:GetFaceUVs(quad), mesh)
	r.quad_colors = list(mesh:GetFaceColors(quad), mesh)
	r.normals = list(mesh:GetNormals(), mesh)
	r.uvs = list(mesh:GetUVs(), mesh)
	r.colors = list(mesh:GetColors(), mesh)
	r.raw_attributes = string.format(
		"%.0f %.0f %.0f",
		mesh:GetFaceNormals(quad)[1],
		mesh:GetFaceUVs(quad)[1],
		mesh:GetFaceColors(quad)[1]
	)
	r.quad_normal = h.show(mesh:GetNormal(mesh:GetFaceNormals(quad)[1]))
	r.default_uv = h.show(mesh:GetUV(mesh:GetFaceUVs(quad)[1]))
	r.default_color = h.show(mesh:GetColor(mesh:GetFaceColors(quad)[1]))
	r.default_alpha = mesh:GetColorAlpha(mesh:GetFaceColors(quad)[1])

	-- A second face sharing two vertices reuses their attributes.
	local v5 = mesh:AddVertex(Vector3.new(2, 0, 0))
	local tri = mesh:AddTriangle(v2, v3, v5)
	r.id_tri = mesh:IdDebugString(tri)
	r.tri_normals = list(mesh:GetFaceNormals(tri), mesh)
	r.tri_uvs = list(mesh:GetFaceUVs(tri), mesh)
	r.after_tri = string.format("%d %d %d %d", #mesh:GetFaces(), #mesh:GetNormals(), #mesh:GetUVs(), #mesh:GetColors())

	r.face_two = h.try(function()
		return mesh:AddFace(v1, v2)
	end)
	r.face_five = h.try(function()
		return mesh:AddFace(v1, v2, v3, v4, v5)
	end)
	r.face_repeat = h.try(function()
		return mesh:IdDebugString(mesh:AddFace(v1, v1, v2))
	end)
	r.face_bad = h.try(function()
		return mesh:AddFace(v1, v2, 99)
	end)
	r.face_numbers = h.try(function()
		return mesh:AddFace(v1, v2, "x")
	end)
	r.triangle_face_id = h.try(function()
		return mesh:AddTriangle(v1, v2, quad)
	end)
	r.position_face_id = h.try(function()
		return mesh:GetPosition(quad)
	end)

	-- SetFaceVertices turns the triangle into a quad and back.
	local v6 = mesh:AddVertex(Vector3.new(2, 0, 1))
	r.set_face_quad = h.try(function()
		mesh:SetFaceVertices(tri, { v2, v3, v6, v5 })
		return list(mesh:GetFaceVertices(tri), mesh) .. " | " .. list(mesh:GetFaceNormals(tri), mesh)
	end)
	r.set_face_two = h.try(function()
		mesh:SetFaceVertices(tri, { v2, v3 })
	end)
	r.set_face_attr_count = h.try(function()
		mesh:SetFaceNormals(quad, { mesh:GetFaceNormals(quad)[1] })
	end)
	r.set_face_attr_kind = h.try(function()
		mesh:SetFaceUVs(quad, mesh:GetFaceNormals(quad))
	end)

	-- Triangulate splits the quads; ids of the triangles left alone stay.
	r.triangulate = h.try(function()
		mesh:Triangulate()
		local faces = mesh:GetFaces()
		local corners = {}
		for i, f in faces do
			corners[i] = #mesh:GetFaceVertices(f)
		end
		return #faces .. " " .. table.concat(corners, ",") .. " " .. list(faces, mesh)
	end)
	r.triangulated_vertices = h.try(function()
		local out = {}
		for i, f in mesh:GetFaces() do
			out[i] = list(mesh:GetFaceVertices(f), mesh)
		end
		return table.concat(out, " / ")
	end)

	-- Removing and re-adding: stable ids, a reused slot gets a new version.
	r.remove_split_quad = h.try(function()
		mesh:RemoveFace(quad)
	end)
	r.remove_live = h.try(function()
		local faces = mesh:GetFaces()
		mesh:RemoveFace(faces[#faces])
		return mesh:IdDebugString(faces[#faces])
	end)
	r.removed_face = h.try(function()
		return mesh:GetFaceVertices(quad)
	end)
	r.removed_debug = h.try(function()
		return mesh:IdDebugString(quad)
	end)
	r.remove_unused = h.try(function()
		return list(mesh:RemoveUnused(), mesh)
	end)
	local v7 = mesh:AddVertex(Vector3.new(5, 5, 5))
	r.reused_vertex = mesh:IdDebugString(v7)
	r.raw_reused = string.format("%.0f", v7)
	r.vertex_list = list(mesh:GetVertices(), mesh)

	r.clear = h.try(function()
		mesh:Clear()
		return string.format("%d %d %d %d %d", #mesh:GetVertices(), #mesh:GetFaces(), #mesh:GetNormals(), #mesh:GetUVs(), #mesh:GetColors())
	end)
	r.after_clear = mesh:IdDebugString(mesh:AddVertex(Vector3.zero))
	r.stale_after_clear = h.try(function()
		return mesh:GetPosition(v1)
	end)

	-- A fixed-size copy keeps the counts and refuses to change them.
	local source = AssetService:CreateEditableMesh()
	local a = source:AddVertex(Vector3.new(0, 0, 0))
	local b = source:AddVertex(Vector3.new(1, 0, 0))
	local c = source:AddVertex(Vector3.new(0, 1, 0))
	local sf = source:AddTriangle(a, b, c)
	local fixed = AssetService:CreateEditableMeshAsync(Content.fromObject(source))
	r.fixed = fixed.FixedSize
	r.fixed_ids = list(fixed:GetVertices(), fixed) .. " | " .. list(fixed:GetFaces(), fixed)
	r.fixed_same_ids = fixed:GetVertices()[1] == a and fixed:GetFaces()[1] == sf
	r.fixed_counts = string.format("%d %d %d", #fixed:GetNormals(), #fixed:GetUVs(), #fixed:GetColors())
	r.fixed_add_vertex = h.try(function()
		return fixed:AddVertex(Vector3.one)
	end)
	r.fixed_add_face = h.try(function()
		return fixed:AddTriangle(a, b, c)
	end)
	r.fixed_add_normal = h.try(function()
		return fixed:AddNormal()
	end)
	r.fixed_add_uv = h.try(function()
		return fixed:AddUV(Vector2.zero)
	end)
	r.fixed_add_color = h.try(function()
		return fixed:AddColor(Color3.new(), 1)
	end)
	r.fixed_remove_face = h.try(function()
		return fixed:RemoveFace(fixed:GetFaces()[1])
	end)
	r.fixed_remove_unused = h.try(function()
		return #fixed:RemoveUnused()
	end)
	r.fixed_clear = h.try(function()
		fixed:Clear()
	end)
	r.fixed_triangulate = h.try(function()
		fixed:Triangulate()
	end)
	r.fixed_set_position = h.try(function()
		fixed:SetPosition(fixed:GetVertices()[1], Vector3.new(3, 3, 3))
		return fixed:GetPosition(fixed:GetVertices()[1])
	end)
	r.fixed_set_face_uvs = h.try(function()
		local uvs = fixed:GetUVs()
		fixed:SetFaceUVs(fixed:GetFaces()[1], { uvs[1], uvs[1], uvs[1] })
	end)
	r.fixed_source_untouched = h.show(source:GetPosition(a))
	r.growable_copy = h.try(function()
		local copy = AssetService:CreateEditableMeshAsync(Content.fromObject(source), { FixedSize = false })
		return tostring(copy.FixedSize) .. " " .. mesh:IdDebugString(copy:AddVertex(Vector3.one))
	end)
	r.empty_copy = h.try(function()
		return AssetService:CreateEditableMeshAsync(Content.fromObject(AssetService:CreateEditableMesh())).FixedSize
	end)
	r.destroyed_copy = h.try(function()
		local gone = AssetService:CreateEditableMesh()
		gone:Destroy()
		return AssetService:CreateEditableMeshAsync(Content.fromObject(gone))
	end)
	r.destroyed_use = h.try(function()
		local gone = AssetService:CreateEditableMesh()
		gone:AddVertex(Vector3.one)
		gone:Destroy()
		return #gone:GetVertices()
	end)

	-- Triangulate on its own: which diagonal, which ids, what order.
	local function split(shapes)
		local m = AssetService:CreateEditableMesh()
		local points = {
			Vector3.new(0, 0, 0), Vector3.new(0, 0, 1), Vector3.new(1, 0, 1), Vector3.new(1, 0, 0),
			Vector3.new(2, 0, 0), Vector3.new(2, 0, 1), Vector3.new(3, 0.5, 1), Vector3.new(3, 0, 0),
		}
		local vs = {}
		for i, p in points do
			vs[i] = m:AddVertex(p)
		end
		for _, shape in shapes do
			local ids = {}
			for i, k in shape do
				ids[i] = vs[k]
			end
			m:AddFace(table.unpack(ids))
		end
		m:Triangulate()
		local out = {}
		for i, f in m:GetFaces() do
			local corners = {}
			for j, v in m:GetFaceVertices(f) do
				corners[j] = m:IdDebugString(v)
			end
			out[i] = m:IdDebugString(f) .. ":" .. table.concat(corners, ",")
		end
		return table.concat(out, " ")
	end
	r.split_one = split({ { 1, 2, 3, 4 } })
	r.split_two = split({ { 1, 2, 3, 4 }, { 4, 3, 6, 5 } })
	r.split_mixed = split({ { 1, 2, 3 }, { 4, 3, 6, 5 }, { 5, 6, 7 } })
	r.split_skew = split({ { 5, 6, 7, 8 } })
	local function splitAt(points)
		local m = AssetService:CreateEditableMesh()
		local vs = {}
		for i, p in points do
			vs[i] = m:AddVertex(p)
		end
		m:AddFace(vs[1], vs[2], vs[3], vs[4])
		m:Triangulate()
		local out = {}
		for i, f in m:GetFaces() do
			local corners = {}
			for j, v in m:GetFaceVertices(f) do
				corners[j] = table.find(vs, v)
			end
			out[i] = table.concat(corners, ",")
		end
		return table.concat(out, " ")
	end
	r.split_kite = splitAt({ Vector3.new(0, 0, 0), Vector3.new(1, 0, 1), Vector3.new(4, 0, 0), Vector3.new(1, 0, -1) })
	r.split_kite_turned = splitAt({ Vector3.new(1, 0, 1), Vector3.new(4, 0, 0), Vector3.new(1, 0, -1), Vector3.new(0, 0, 0) })
	r.split_dart = splitAt({ Vector3.new(-1, 0, 1), Vector3.new(0, 0, 0), Vector3.new(1, 0, 1), Vector3.new(0, 0, -2) })
	r.split_dart_turned = splitAt({ Vector3.new(0, 0, 0), Vector3.new(1, 0, 1), Vector3.new(0, 0, -2), Vector3.new(-1, 0, 1) })
	r.split_twisted = splitAt({ Vector3.new(0, 0, 0), Vector3.new(0, 0, 1), Vector3.new(1, 1, 1), Vector3.new(1, 0, 0) })
	r.split_twisted_other = splitAt({ Vector3.new(0, 1, 0), Vector3.new(0, 0, 1), Vector3.new(1, 0, 1), Vector3.new(1, 0, 0) })
	r.split_attrs = h.try(function()
		local m = AssetService:CreateEditableMesh()
		local a, b, c, d = m:AddVertex(Vector3.zero), m:AddVertex(Vector3.zAxis), m:AddVertex(Vector3.new(1, 0, 1)), m:AddVertex(Vector3.xAxis)
		local f = m:AddFace(a, b, c, d)
		local uvs = m:GetFaceUVs(f)
		m:Triangulate()
		local out = {}
		for i, face in m:GetFaces() do
			local names = {}
			for j, id in m:GetFaceUVs(face) do
				names[j] = m:IdDebugString(id)
			end
			out[i] = table.concat(names, ",")
		end
		return #uvs .. " " .. table.concat(out, " ")
	end)

	-- The documented limits: 60,000 vertices, 20,000 triangles (a quad is two).
	local big = AssetService:CreateEditableMesh()
	local made = 0
	r.vertex_limit = h.try(function()
		for i = 1, 60001 do
			big:AddVertex(Vector3.new(i, 0, 0))
			made = i
		end
	end)
	r.vertex_limit_count = made
	local ids = big:GetVertices()
	local faces = 0
	r.triangle_limit = h.try(function()
		for i = 1, 20001 do
			big:AddTriangle(ids[i], ids[i + 1], ids[i + 2])
			faces = i
		end
	end)
	r.triangle_limit_count = faces
	big:Clear()
	ids = {}
	for i = 1, 4 * 10001 do
		ids[i] = big:AddVertex(Vector3.new(i, 0, 0))
	end
	local quads = 0
	r.quad_limit = h.try(function()
		for i = 0, 10000 do
			big:AddFace(ids[4 * i + 1], ids[4 * i + 2], ids[4 * i + 3], ids[4 * i + 4])
			quads = i + 1
		end
	end)
	r.quad_limit_count = quads
	big:Destroy()
	return r
end
