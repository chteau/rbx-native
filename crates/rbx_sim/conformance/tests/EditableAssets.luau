-- Editables made from assets on a server: a mesh and an image the place's
-- owner made, the same from Roblox's own account (not shared with the
-- owner), CreateMeshPartAsync from an asset id and from a mesh, ApplyMesh,
-- and opaque DataModel content.
return function(h)
	local AssetService = game:GetService("AssetService")
	local r = {}
	-- Made by the sandbox place's owner, and by Roblox (the traffic cone).
	local OWNED_MESH = "rbxassetid://122810350731801"
	local OWNED_IMAGE = "rbxassetid://126400952100520"
	local ROBLOX_MESH = "rbxassetid://1082802"
	local ROBLOX_IMAGE = "rbxassetid://1082804"
	local function counts(mesh)
		local quads = 0
		for _, f in mesh:GetFaces() do
			if #mesh:GetFaceVertices(f) == 4 then
				quads += 1
			end
		end
		return string.format(
			"%d %d %d %d %d q%d",
			#mesh:GetVertices(),
			#mesh:GetFaces(),
			#mesh:GetNormals(),
			#mesh:GetUVs(),
			#mesh:GetColors(),
			quads
		)
	end

	local okMesh, owned = pcall(function()
		return AssetService:CreateEditableMeshAsync(Content.fromUri(OWNED_MESH))
	end)
	r.owned_mesh = okMesh and "ok" or tostring(owned):gsub("^.-:%d+: ", "")
	if okMesh then
		r.owned_fixed = owned.FixedSize
		r.owned_counts = counts(owned)
		r.owned_size = h.vec(owned:GetSize(), 3)
		r.owned_center = h.vec(owned:GetCenter(), 3)
		local v = owned:GetVertices()[1]
		r.owned_first_vertex = owned:IdDebugString(v) .. " " .. h.vec(owned:GetPosition(v), 3)
		local f = owned:GetFaces()[1]
		r.owned_first_face = owned:IdDebugString(f)
		local uv = owned:GetUV(owned:GetFaceUVs(f)[1])
		r.owned_first_uv = string.format("%.4f,%.4f", uv.X, uv.Y)
		r.owned_first_normal = h.vec(owned:GetNormal(owned:GetFaceNormals(f)[1]), 3)
		r.owned_first_color = h.show(owned:GetColor(owned:GetFaceColors(f)[1])) .. " " .. owned:GetColorAlpha(owned:GetFaceColors(f)[1])
		r.owned_bones = #owned:GetBones()
	end
	r.owned_growable = h.try(function()
		local m = AssetService:CreateEditableMeshAsync(Content.fromUri(OWNED_MESH), { FixedSize = false })
		return tostring(m.FixedSize) .. " " .. counts(m)
	end)
	r.owned_asset_id = h.try(function()
		return AssetService:CreateEditableMeshAsync(Content.fromAssetId(122810350731801)).FixedSize
	end)
	r.roblox_mesh = h.try(function()
		return AssetService:CreateEditableMeshAsync(Content.fromUri(ROBLOX_MESH))
	end)
	r.missing_mesh = h.try(function()
		return AssetService:CreateEditableMeshAsync(Content.fromUri("rbxassetid://1"))
	end)
	r.image_as_mesh = h.try(function()
		return AssetService:CreateEditableMeshAsync(Content.fromUri(OWNED_IMAGE))
	end)
	r.empty_content = h.try(function()
		return AssetService:CreateEditableMeshAsync(Content.none)
	end)
	r.not_content = h.try(function()
		return AssetService:CreateEditableMeshAsync(OWNED_MESH)
	end)

	-- MeshParts from an asset id.
	local okPart, part = pcall(function()
		return AssetService:CreateMeshPartAsync(Content.fromUri(OWNED_MESH))
	end)
	r.part_uri = okPart and "ok" or tostring(part):gsub("^.-:%d+: ", "")
	if okPart then
		r.part_size = h.vec(part.Size, 3)
		r.part_mesh_size = h.vec(part.MeshSize, 3)
		r.part_mesh_id = part.MeshId
		r.part_content = h.show(part.MeshContent.SourceType) .. " " .. tostring(part.MeshContent.Uri)
		r.part_fidelity = h.show(part.CollisionFidelity) .. " " .. h.show(part.RenderFidelity) .. " " .. h.show(part.FluidFidelity)
		r.part_parent = tostring(part.Parent)
		r.part_name = part.Name
		r.part_anchored = part.Anchored
		r.from_part_content = h.try(function()
			return counts(AssetService:CreateEditableMeshAsync(part.MeshContent))
		end)
	end
	r.part_roblox_uri = h.try(function()
		return h.vec(AssetService:CreateMeshPartAsync(Content.fromUri(ROBLOX_MESH)).MeshSize, 3)
	end)
	r.part_options = h.try(function()
		local p = AssetService:CreateMeshPartAsync(Content.fromUri(OWNED_MESH), {
			CollisionFidelity = Enum.CollisionFidelity.Box,
			RenderFidelity = Enum.RenderFidelity.Performance,
			FluidFidelity = Enum.FluidFidelity.UseCollisionGeometry,
		})
		return h.show(p.CollisionFidelity) .. " " .. h.show(p.RenderFidelity) .. " " .. h.show(p.FluidFidelity)
	end)
	r.part_bad_option = h.try(function()
		return AssetService:CreateMeshPartAsync(Content.fromUri(OWNED_MESH), { CollisionFidelity = "Nope" })
	end)
	r.part_missing = h.try(function()
		return AssetService:CreateMeshPartAsync(Content.fromUri("rbxassetid://1"))
	end)

	-- From an EditableMesh: off-centre vertices, then ApplyMesh onto a part.
	local em = AssetService:CreateEditableMesh()
	local a = em:AddVertex(Vector3.new(2, 0, 0))
	local b = em:AddVertex(Vector3.new(4, 0, 0))
	local c = em:AddVertex(Vector3.new(2, 3, 1))
	em:AddTriangle(a, c, b)
	local okEm, fromEm = pcall(function()
		return AssetService:CreateMeshPartAsync(Content.fromObject(em))
	end)
	r.part_editable = okEm and "ok" or tostring(fromEm):gsub("^.-:%d+: ", "")
	if okEm then
		r.part_editable_size = h.vec(fromEm.Size, 3) .. " " .. h.vec(fromEm.MeshSize, 3)
		r.part_editable_id = fromEm.MeshId
		r.part_editable_content = h.show(fromEm.MeshContent.SourceType)
		r.part_editable_same = fromEm.MeshContent.Object == em
		r.apply_mesh = h.try(function()
			local target = Instance.new("MeshPart")
			target.Size = Vector3.new(5, 5, 5)
			target:ApplyMesh(fromEm)
			return h.vec(target.Size, 3) .. " " .. h.vec(target.MeshSize, 3) .. " " .. h.show(target.MeshContent.SourceType)
		end)
		r.apply_mesh_uri = okPart and h.try(function()
			local target = Instance.new("MeshPart")
			target:ApplyMesh(part)
			return target.MeshId .. " " .. h.vec(target.MeshSize, 3)
		end) or "nil"
		r.apply_mesh_part = h.try(function()
			Instance.new("MeshPart"):ApplyMesh(Instance.new("Part"))
		end)
		r.write_mesh_content = h.try(function()
			fromEm.MeshContent = Content.fromObject(em)
		end)
		r.edit_after_part = h.try(function()
			em:SetPosition(c, Vector3.new(2, 9, 1))
			return h.vec(fromEm.Size, 3)
		end)
	end
	r.part_empty_editable = h.try(function()
		return AssetService:CreateMeshPartAsync(Content.fromObject(AssetService:CreateEditableMesh()))
	end)
	r.part_no_faces = h.try(function()
		local m = AssetService:CreateEditableMesh()
		m:AddVertex(Vector3.zero)
		m:AddVertex(Vector3.one)
		return AssetService:CreateMeshPartAsync(Content.fromObject(m))
	end)
	r.part_image = h.try(function()
		return AssetService:CreateMeshPartAsync(Content.fromObject(AssetService:CreateEditableImage()))
	end)

	-- Images.
	local okImage, image = pcall(function()
		return AssetService:CreateEditableImageAsync(Content.fromUri(OWNED_IMAGE))
	end)
	r.owned_image = okImage and "ok" or tostring(image):gsub("^.-:%d+: ", "")
	if okImage then
		r.owned_image_size = h.show(image.Size)
		local px = image:ReadPixelsBuffer(Vector2.new(0, 0), Vector2.new(1, 1))
		r.owned_image_pixel = string.format("%d,%d,%d,%d", buffer.readu8(px, 0), buffer.readu8(px, 1), buffer.readu8(px, 2), buffer.readu8(px, 3))
		local all = image:ReadPixelsBuffer(Vector2.zero, image.Size)
		local total = 0
		for i = 0, buffer.len(all) - 1, 97 do
			total += buffer.readu8(all, i)
		end
		r.owned_image_sample = total
	end
	r.roblox_image = h.try(function()
		return AssetService:CreateEditableImageAsync(Content.fromUri(ROBLOX_IMAGE))
	end)
	r.thumb_image = h.try(function()
		return h.show(AssetService:CreateEditableImageAsync(Content.fromUri("rbxthumb://type=Asset&id=1082802&w=150&h=150")).Size)
	end)
	r.missing_image = h.try(function()
		return AssetService:CreateEditableImageAsync(Content.fromUri("rbxassetid://1"))
	end)
	r.mesh_as_image = h.try(function()
		return AssetService:CreateEditableImageAsync(Content.fromUri(OWNED_MESH))
	end)
	r.image_from_mesh_object = h.try(function()
		return AssetService:CreateEditableImageAsync(Content.fromObject(em))
	end)
	r.mesh_from_image_object = h.try(function()
		return AssetService:CreateEditableMeshAsync(Content.fromObject(AssetService:CreateEditableImage()))
	end)

	-- Opaque content.
	r.datamodel_content = h.try(function()
		local result, content = AssetService:CreateDataModelContentAsync(Content.fromObject(em))
		return h.show(result) .. " " .. h.show(content.SourceType) .. " " .. tostring(content.Uri) .. " " .. tostring(content.Object)
	end)
	r.datamodel_roundtrip = h.try(function()
		local _, content = AssetService:CreateDataModelContentAsync(Content.fromObject(em))
		em:SetPosition(a, Vector3.new(-5, 0, 0))
		local copy = AssetService:CreateEditableMeshAsync(content)
		return tostring(copy.FixedSize) .. " " .. h.vec(copy:GetPosition(copy:GetVertices()[1]), 3)
	end)
	r.datamodel_part = h.try(function()
		local _, content = AssetService:CreateDataModelContentAsync(Content.fromObject(em))
		local p = AssetService:CreateMeshPartAsync(content)
		return h.show(p.MeshContent.SourceType) .. " " .. h.vec(p.MeshSize, 3)
	end)
	r.datamodel_image = h.try(function()
		local img = AssetService:CreateEditableImage({ Size = Vector2.new(3, 2) })
		local result, content = AssetService:CreateDataModelContentAsync(Content.fromObject(img))
		img:Destroy()
		return h.show(result) .. " " .. h.show(AssetService:CreateEditableImageAsync(content).Size)
	end)
	r.datamodel_uri = h.try(function()
		return AssetService:CreateDataModelContentAsync(Content.fromUri(OWNED_MESH))
	end)
	r.datamodel_label = h.try(function()
		local img = AssetService:CreateEditableImage({ Size = Vector2.new(3, 2) })
		local _, content = AssetService:CreateDataModelContentAsync(Content.fromObject(img))
		local label = Instance.new("ImageLabel")
		label.ImageContent = content
		return h.show(label.ImageContent.SourceType) .. " " .. label.Image
	end)
	return r
end
