-- EditableBudget on the client: editables made until the budget says no,
-- then all released, then one more to see the budget came back.
local AssetService = game:GetService("AssetService")

return function(_ch)
	local r = {}
	local function fill(make, cap)
		local made, failed = {}, nil
		for _ = 1, cap do
			local ok, value = pcall(make)
			if not ok then
				failed = (tostring(value):gsub("^.-:%d+: ", ""))
				break
			elseif value == nil then
				failed = "nil"
				break
			end
			table.insert(made, value)
		end
		return made, failed
	end
	local function release(list)
		for _, e in list do
			e:Destroy()
		end
	end

	local images, why = fill(function()
		return AssetService:CreateEditableImage({ Size = Vector2.new(1024, 1024) })
	end, 400)
	r.images_1024 = #images
	r.images_1024_end = why or "cap"
	release(images)
	r.image_after_release = AssetService:CreateEditableImage({ Size = Vector2.new(1024, 1024) }) ~= nil

	local small, whySmall = fill(function()
		return AssetService:CreateEditableImage({ Size = Vector2.new(64, 64) })
	end, 4000)
	r.images_64 = #small
	r.images_64_end = whySmall or "cap"
	release(small)

	local meshes, whyMesh = fill(function()
		return AssetService:CreateEditableMesh()
	end, 2000)
	r.empty_meshes = #meshes
	r.empty_meshes_end = whyMesh or "cap"
	release(meshes)

	local source = AssetService:CreateEditableMesh()
	local ids = {}
	for i = 1, 300 do
		ids[i] = source:AddVertex(Vector3.new(i, i % 7, i % 3))
	end
	for i = 1, 298 do
		source:AddTriangle(ids[i], ids[i + 1], ids[i + 2])
	end
	local fixed, whyFixed = fill(function()
		return AssetService:CreateEditableMeshAsync(Content.fromObject(source), { FixedSize = true })
	end, 2000)
	r.fixed_300 = #fixed
	r.fixed_300_end = whyFixed or "cap"
	release(fixed)
	local growable, whyGrow = fill(function()
		return AssetService:CreateEditableMeshAsync(Content.fromObject(source), { FixedSize = false })
	end, 2000)
	r.growable_300 = #growable
	r.growable_300_end = whyGrow or "cap"
	release(growable)
	source:Destroy()
	r.mesh_after_release = AssetService:CreateEditableMesh() ~= nil
	return r
end
