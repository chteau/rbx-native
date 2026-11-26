-- EditableMesh bones, skinning and FACS poses on a server: what AddBone
-- takes and refuses, the hierarchy rules, per-vertex bones and weights,
-- base and corrective poses.
return function(h)
	local AssetService = game:GetService("AssetService")
	local F = Enum.FacsActionUnit
	local r = {}
	local mesh = AssetService:CreateEditableMesh()
	local function cf(c)
		return h.vec(c.Position, 3) .. "/" .. h.vec(c.LookVector, 3)
	end

	r.bones_empty = #mesh:GetBones()
	local root = mesh:AddBone({ Name = "Root", CFrame = CFrame.new(0, 1, 0), Virtual = false })
	r.bone_id = mesh:IdDebugString(root)
	r.bone_type = typeof(root)
	local child = mesh:AddBone({ Name = "Child", ParentId = root, CFrame = CFrame.new(0, 2, 0) * CFrame.Angles(0, math.pi / 2, 0), Virtual = false })
	r.no_virtual = h.try(function()
		return mesh:AddBone({ Name = "NoVirtual", CFrame = CFrame.new() })
	end)
	r.no_cframe = h.try(function()
		return mesh:IdDebugString(mesh:AddBone({ Name = "NoCFrame", Virtual = true }))
	end)
	local face = mesh:AddBone({ Name = "Jaw", Virtual = true, CFrame = CFrame.new() })
	r.bones = #mesh:GetBones()
	r.bone_name = mesh:GetBoneName(child)
	r.bone_parent = mesh:GetBoneParent(child) == root
	r.root_parent = mesh:GetBoneParent(root)
	r.bone_cframe = cf(mesh:GetBoneCFrame(child))
	r.default_cframe = cf(mesh:GetBoneCFrame(face))
	r.virtual = tostring(mesh:GetBoneIsVirtual(face)) .. " " .. tostring(mesh:GetBoneIsVirtual(root))
	r.by_name = mesh:GetBoneByName("Child") == child
	r.by_name_missing = h.try(function()
		return mesh:GetBoneByName("Nope")
	end)
	r.duplicate_name = h.try(function()
		return mesh:AddBone({ Name = "Root", Virtual = false, CFrame = CFrame.new() })
	end)
	r.long_name = h.try(function()
		return mesh:AddBone({ Name = string.rep("a", 101), Virtual = false, CFrame = CFrame.new() })
	end)
	r.name_100 = h.try(function()
		return #mesh:GetBoneName(mesh:AddBone({ Name = string.rep("b", 100), Virtual = false, CFrame = CFrame.new() }))
	end)
	r.no_name = h.try(function()
		return mesh:IdDebugString(mesh:AddBone({ Virtual = false, CFrame = CFrame.new() }))
	end)
	r.bad_parent = h.try(function()
		return mesh:AddBone({ Name = "Orphan", ParentId = 4242, Virtual = false, CFrame = CFrame.new() })
	end)
	r.unknown_key = h.try(function()
		return typeof(mesh:AddBone({ Name = "Extra", Bogus = 1, Virtual = false, CFrame = CFrame.new() }))
	end)
	r.set_name = h.try(function()
		mesh:SetBoneName(child, "Arm")
		return mesh:GetBoneName(child)
	end)
	r.set_name_taken = h.try(function()
		mesh:SetBoneName(child, "Root")
	end)
	r.set_cframe = h.try(function()
		mesh:SetBoneCFrame(root, CFrame.new(1, 2, 3))
		return cf(mesh:GetBoneCFrame(root))
	end)
	r.set_virtual = h.try(function()
		mesh:SetBoneIsVirtual(root, true)
		local v = mesh:GetBoneIsVirtual(root)
		mesh:SetBoneIsVirtual(root, false)
		return v
	end)
	r.set_parent_cycle = h.try(function()
		mesh:SetBoneParent(root, child)
	end)
	r.set_parent_self = h.try(function()
		mesh:SetBoneParent(root, root)
	end)
	r.set_parent_zero = h.try(function()
		mesh:SetBoneParent(child, 0)
		local p = mesh:GetBoneParent(child)
		mesh:SetBoneParent(child, root)
		return p
	end)

	-- Skinning.
	local v1 = mesh:AddVertex(Vector3.new(0, 0, 0))
	local v2 = mesh:AddVertex(Vector3.new(0, 1, 0))
	r.vertex_bones_empty = #mesh:GetVertexBones(v1)
	r.vertex_weights_empty = #mesh:GetVertexBoneWeights(v1)
	r.set_bones = h.try(function()
		mesh:SetVertexBones(v1, { root, child })
		return #mesh:GetVertexBones(v1)
	end)
	r.set_weights = h.try(function()
		mesh:SetVertexBoneWeights(v1, { 0.75, 0.25 })
		return table.concat(mesh:GetVertexBoneWeights(v1), " ")
	end)
	r.weights_unnormalized = h.try(function()
		mesh:SetVertexBoneWeights(v1, { 3, 1 })
		return table.concat(mesh:GetVertexBoneWeights(v1), " ")
	end)
	r.weights_rounding = h.try(function()
		mesh:SetVertexBoneWeights(v1, { 0.6, 0.1 })
		local w = mesh:GetVertexBoneWeights(v1)
		return string.format("%d %d", math.round(w[1] * 255), math.round(w[2] * 255))
	end)
	r.weights_mismatch = h.try(function()
		mesh:SetVertexBoneWeights(v1, { 1 })
	end)
	r.weights_before_bones = h.try(function()
		mesh:SetVertexBoneWeights(v2, { 1 })
		return table.concat(mesh:GetVertexBoneWeights(v2), " ")
	end)
	r.five_bones = h.try(function()
		local extra = {}
		for i = 1, 5 do
			extra[i] = mesh:AddBone({ Name = "B" .. i, Virtual = false, CFrame = CFrame.new() })
		end
		mesh:SetVertexBones(v2, extra)
		return #mesh:GetVertexBones(v2)
	end)
	r.bad_bone = h.try(function()
		mesh:SetVertexBones(v2, { 9999 })
	end)

	-- FACS poses need virtual bones.
	r.poses_empty = #mesh:GetFacsPoses()
	r.set_pose = h.try(function()
		mesh:SetFacsPose(F.JawDrop, { face }, { CFrame.new(0, -0.5, 0) })
		local bones, frames = mesh:GetFacsPose(F.JawDrop)
		return #bones .. " " .. cf(frames[1])
	end)
	r.poses = h.try(function()
		local out = {}
		for i, p in mesh:GetFacsPoses() do
			out[i] = p.Name
		end
		return table.concat(out, " ")
	end)
	r.set_pose_real_bone = h.try(function()
		mesh:SetFacsPose(F.LipsTogether, { root }, { CFrame.new() })
	end)
	r.set_pose_mismatch = h.try(function()
		mesh:SetFacsPose(F.LipsTogether, { face }, {})
	end)
	r.bone_pose = h.try(function()
		mesh:SetFacsBonePose(F.Funneler, face, CFrame.new(0, 0, 1))
		local bones, frames = mesh:GetFacsPose(F.Funneler)
		return #bones .. " " .. cf(frames[1])
	end)
	r.unused_pose = h.try(function()
		local bones, frames = mesh:GetFacsPose(F.Pucker)
		return #bones .. " " .. #frames
	end)
	r.corrective = h.try(function()
		mesh:SetFacsCorrectivePose({ F.JawDrop, F.Funneler }, { face }, { CFrame.new(0, 1, 0) })
		local bones, frames = mesh:GetFacsCorrectivePose({ F.JawDrop, F.Funneler })
		return #bones .. " " .. cf(frames[1])
	end)
	r.correctives = h.try(function()
		local out = {}
		for i, set in mesh:GetFacsCorrectivePoses() do
			local n = {}
			for j, a in set do
				n[j] = a.Name
			end
			out[i] = table.concat(n, "+")
		end
		return table.concat(out, " ")
	end)
	r.corrective_one = h.try(function()
		mesh:SetFacsCorrectivePose({ F.JawDrop }, { face }, { CFrame.new() })
	end)
	r.corrective_sides = h.try(function()
		mesh:SetFacsCorrectivePose({ F.LeftCheekPuff, F.RightEyeClosed }, { face }, { CFrame.new() })
	end)
	r.corrective_four = h.try(function()
		mesh:SetFacsCorrectivePose({ F.JawDrop, F.Funneler, F.Pucker, F.LipsTogether }, { face }, { CFrame.new() })
	end)

	-- Removing a bone clears it from skins and poses; children lose it.
	r.remove_bone = h.try(function()
		mesh:RemoveBone(root)
		return string.format(
			"%d %d %s",
			#mesh:GetBones(),
			#mesh:GetVertexBones(v1),
			tostring(mesh:GetBoneParent(child))
		)
	end)
	r.removed_get = h.try(function()
		return mesh:GetBoneName(root)
	end)
	r.remove_face_bone = h.try(function()
		mesh:RemoveBone(face)
		return #mesh:GetFacsPoses() .. " " .. #mesh:GetFacsCorrectivePoses()
	end)
	r.clear = h.try(function()
		mesh:Clear()
		return #mesh:GetBones()
	end)
	local count = 0
	r.bone_limit = h.try(function()
		local m = AssetService:CreateEditableMesh()
		for i = 1, 1000 do
			m:AddBone({ Name = "L" .. i, Virtual = false, CFrame = CFrame.new() })
			count = i
		end
	end)
	r.bone_limit_count = count
	return r
end
