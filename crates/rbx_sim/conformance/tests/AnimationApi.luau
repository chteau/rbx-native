-- The animation data API as a live server answers it: FloatCurve,
-- ValueCurve, RotationCurve, MarkerCurve, Vector3Curve and
-- EulerRotationCurve with their key datatypes; KeyframeSequence, Keyframe
-- and Pose; Animator's GetTrackByAnimationId and StepAnimations;
-- AnimationTrack's parameters and targets; HumanoidRigDescription,
-- DigitsRigDescription, IKControl and AnimationNodeDefinition.
return function(h, origin)
	local result = {}
	local function show(v)
		if type(v) == "table" then
			local keys = {}
			for k in v do
				table.insert(keys, k)
			end
			table.sort(keys, function(a, b)
				return tostring(a) < tostring(b)
			end)
			local out = {}
			for _, k in keys do
				table.insert(out, (type(k) == "number" and "" or tostring(k) .. "=") .. show(v[k]))
			end
			return "{" .. table.concat(out, ",") .. "}"
		elseif typeof(v) == "FloatCurveKey" or typeof(v) == "ValueCurveKey" then
			return string.format(
				"%s(%s,%s,%s,%s,%s)",
				typeof(v),
				h.round(v.Time, 3),
				show(v.Value),
				v.Interpolation.Name,
				show(v.LeftTangent),
				show(v.RightTangent)
			)
		elseif typeof(v) == "RotationCurveKey" then
			return string.format("RotationCurveKey(%s,%s,%s)", h.round(v.Time, 3), show(v.Value), v.Interpolation.Name)
		elseif typeof(v) == "CFrame" then
			local x, y, z = v:ToEulerAnglesXYZ()
			return "CFrame:" .. h.vec(v.Position, 3) .. "/" .. h.vec(Vector3.new(x, y, z), 3)
		elseif typeof(v) == "Instance" then
			return v.ClassName .. ":" .. v.Name
		elseif typeof(v) == "EnumItem" then
			return tostring(v)
		end
		return tostring(h.show(v))
	end
	local function try(f, ...)
		local ok, value = pcall(f, ...)
		if ok then
			return show(value)
		end
		return "err: " .. (tostring(value):gsub("^.-:%d+: ", ""))
	end

	-- Key datatypes.
	result.fkey_new = try(function()
		return FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Cubic)
	end)
	result.fkey_type = try(function()
		return typeof(FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Linear))
	end)
	result.fkey_default = try(function()
		return FloatCurveKey.new(1, 2)
	end)
	result.fkey_tangent = try(function()
		local k = FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Cubic)
		k.LeftTangent = 0.5
		k.RightTangent = -1
		return k
	end)
	result.fkey_tangent_linear = try(function()
		local k = FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Linear)
		k.RightTangent = 1
		return k
	end)
	result.fkey_eq = try(function()
		return FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Linear)
			== FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Linear)
	end)
	result.fkey_tostring = try(function()
		return tostring(FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Linear))
	end)
	result.fkey_bad = try(function()
		return FloatCurveKey.new("a", 2)
	end)
	result.vkey_new = try(function()
		return ValueCurveKey.new(0.5, Vector3.new(1, 2, 3), Enum.KeyInterpolationMode.Linear)
	end)
	result.rkey_new = try(function()
		return RotationCurveKey.new(0.5, CFrame.Angles(0, math.pi / 2, 0), Enum.KeyInterpolationMode.Linear)
	end)

	-- FloatCurve.
	local curve = Instance.new("FloatCurve")
	result.f_empty_value = try(function()
		return curve:GetValueAtTime(0)
	end)
	result.f_empty_keys = try(function()
		return curve:GetKeys()
	end)
	result.f_insert1 = try(function()
		return curve:InsertKey(FloatCurveKey.new(1, 10, Enum.KeyInterpolationMode.Linear))
	end)
	result.f_insert0 = try(function()
		return curve:InsertKey(FloatCurveKey.new(0, 0, Enum.KeyInterpolationMode.Linear))
	end)
	result.f_insert_same = try(function()
		return curve:InsertKey(FloatCurveKey.new(1, 20, Enum.KeyInterpolationMode.Constant))
	end)
	result.f_insert2 = try(function()
		return curve:InsertKey(FloatCurveKey.new(2, 0, Enum.KeyInterpolationMode.Cubic))
	end)
	result.f_insert3 = try(function()
		return curve:InsertKey(FloatCurveKey.new(3, 5, Enum.KeyInterpolationMode.Cubic))
	end)
	result.f_length = try(function()
		return curve.Length
	end)
	result.f_keys = try(function()
		return curve:GetKeys()
	end)
	for _, t in { -1, 0, 0.25, 1, 1.5, 2, 2.5, 3, 4 } do
		result["f_value_" .. t] = try(function()
			return curve:GetValueAtTime(t)
		end)
		result["f_indices_" .. t] = try(function()
			return curve:GetKeyIndicesAtTime(t)
		end)
	end
	result.f_key_at1 = try(function()
		return curve:GetKeyAtIndex(1)
	end)
	result.f_key_at9 = try(function()
		return curve:GetKeyAtIndex(9)
	end)
	result.f_key_at0 = try(function()
		return curve:GetKeyAtIndex(0)
	end)
	result.f_remove = try(function()
		return curve:RemoveKeyAtIndex(2, 1)
	end)
	result.f_remove_many = try(function()
		return curve:RemoveKeyAtIndex(2, 10)
	end)
	result.f_remove_bad = try(function()
		return curve:RemoveKeyAtIndex(9)
	end)
	result.f_after_remove = try(function()
		return curve:GetKeys()
	end)
	result.f_set = try(function()
		return curve:SetKeys({
			FloatCurveKey.new(2, 1, Enum.KeyInterpolationMode.Linear),
			FloatCurveKey.new(0, 3, Enum.KeyInterpolationMode.Linear),
		})
	end)
	result.f_set_keys = try(function()
		return curve:GetKeys()
	end)
	result.f_set_dupe = try(function()
		return curve:SetKeys({
			FloatCurveKey.new(1, 1, Enum.KeyInterpolationMode.Linear),
			FloatCurveKey.new(1, 3, Enum.KeyInterpolationMode.Linear),
		})
	end)
	result.f_set_bad = try(function()
		return curve:SetKeys({ 1, 2 })
	end)
	result.f_insert_bad = try(function()
		return curve:InsertKey(5)
	end)
	local tangents = Instance.new("FloatCurve")
	local a = FloatCurveKey.new(0, 0, Enum.KeyInterpolationMode.Cubic)
	a.RightTangent = 4
	local b = FloatCurveKey.new(1, 1, Enum.KeyInterpolationMode.Cubic)
	b.LeftTangent = 0
	tangents:SetKeys({ a, b })
	result.f_tan_half = try(function()
		return tangents:GetValueAtTime(0.5)
	end)
	result.f_tan_quarter = try(function()
		return tangents:GetValueAtTime(0.25)
	end)
	result.f_clone = try(function()
		return #curve:Clone():GetKeys()
	end)

	-- ValueCurve.
	local vc = Instance.new("ValueCurve")
	result.v_value_type = try(function()
		return vc.ValueType
	end)
	result.v_insert = try(function()
		return vc:InsertKeyValue(0, 1)
	end)
	result.v_insert2 = try(function()
		return vc:InsertKeyValue(1, 3, Enum.KeyInterpolationMode.Linear)
	end)
	result.v_mid = try(function()
		return vc:GetValueAtTime(0.5)
	end)
	result.v_keys = try(function()
		return vc:GetKeys()
	end)
	result.v_mixed = try(function()
		return vc:InsertKeyValue(2, Vector3.new(1, 1, 1))
	end)
	local vv = Instance.new("ValueCurve")
	result.v_vec = try(function()
		vv:InsertKeyValue(0, Vector3.new(0, 0, 0), Enum.KeyInterpolationMode.Linear)
		vv:InsertKeyValue(2, Vector3.new(2, 4, 6), Enum.KeyInterpolationMode.Linear)
		return vv:GetValueAtTime(1)
	end)
	result.v_vec_type = try(function()
		return vv.ValueType
	end)
	result.v_string = try(function()
		local s = Instance.new("ValueCurve")
		s:InsertKeyValue(0, "a", Enum.KeyInterpolationMode.Linear)
		s:InsertKeyValue(1, "b", Enum.KeyInterpolationMode.Linear)
		return s:GetValueAtTime(0.75)
	end)

	-- RotationCurve.
	local rc = Instance.new("RotationCurve")
	result.r_insert = try(function()
		rc:InsertKey(RotationCurveKey.new(0, CFrame.new(), Enum.KeyInterpolationMode.Linear))
		return rc:InsertKey(RotationCurveKey.new(1, CFrame.Angles(0, math.pi / 2, 0), Enum.KeyInterpolationMode.Linear))
	end)
	result.r_mid = try(function()
		return rc:GetValueAtTime(0.5)
	end)
	result.r_constant = try(function()
		local c = Instance.new("RotationCurve")
		c:InsertKey(RotationCurveKey.new(0, CFrame.new(), Enum.KeyInterpolationMode.Constant))
		c:InsertKey(RotationCurveKey.new(1, CFrame.Angles(1, 0, 0), Enum.KeyInterpolationMode.Constant))
		return c:GetValueAtTime(0.9)
	end)
	result.r_position = try(function()
		local c = Instance.new("RotationCurve")
		return c:InsertKey(RotationCurveKey.new(0, CFrame.new(1, 2, 3), Enum.KeyInterpolationMode.Linear))
	end)

	-- MarkerCurve.
	local mc = Instance.new("MarkerCurve")
	result.m_insert = try(function()
		return mc:InsertMarkerAtTime(1, "Hit")
	end)
	result.m_insert0 = try(function()
		return mc:InsertMarkerAtTime(0.5, "Swing")
	end)
	result.m_insert_same = try(function()
		return mc:InsertMarkerAtTime(1, "Again")
	end)
	result.m_markers = try(function()
		return mc:GetMarkers()
	end)
	result.m_at = try(function()
		return mc:GetMarkerAtIndex(1)
	end)
	result.m_at_bad = try(function()
		return mc:GetMarkerAtIndex(5)
	end)
	result.m_remove = try(function()
		return mc:RemoveMarkerAtIndex(1)
	end)
	result.m_length = try(function()
		return mc.Length
	end)

	-- Vector3Curve and EulerRotationCurve.
	local v3 = Instance.new("Vector3Curve")
	result.v3_x_none = try(function()
		return v3:X()
	end)
	result.v3_value_none = try(function()
		return v3:GetValueAtTime(0)
	end)
	local x = Instance.new("FloatCurve")
	x.Name = "X"
	x:InsertKey(FloatCurveKey.new(0, 0, Enum.KeyInterpolationMode.Linear))
	x:InsertKey(FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Linear))
	x.Parent = v3
	result.v3_x = try(function()
		return v3:X()
	end)
	result.v3_value = try(function()
		return v3:GetValueAtTime(0.5)
	end)
	local euler = Instance.new("EulerRotationCurve")
	local ey = Instance.new("FloatCurve")
	ey.Name = "Y"
	ey:InsertKey(FloatCurveKey.new(0, 0, Enum.KeyInterpolationMode.Linear))
	ey:InsertKey(FloatCurveKey.new(1, math.pi, Enum.KeyInterpolationMode.Linear))
	ey.Parent = euler
	result.e_angles = try(function()
		return euler:GetAnglesAtTime(0.5)
	end)
	result.e_rotation = try(function()
		return euler:GetRotationAtTime(0.5)
	end)
	result.e_order = try(function()
		return euler.RotationOrder
	end)

	-- KeyframeSequence, Keyframe, Pose.
	local seq = Instance.new("KeyframeSequence")
	local k1 = Instance.new("Keyframe")
	k1.Time = 0.5
	result.ks_add = try(function()
		return seq:AddKeyframe(k1)
	end)
	result.ks_parent = try(function()
		return k1.Parent == seq
	end)
	result.ks_add_bad = try(function()
		return seq:AddKeyframe(Instance.new("Folder"))
	end)
	result.ks_add_nil = try(function()
		return seq:AddKeyframe(nil)
	end)
	Instance.new("Folder").Parent = seq
	local k0 = Instance.new("Keyframe")
	k0.Parent = seq
	result.ks_get = try(function()
		local out = {}
		for _, k in seq:GetKeyframes() do
			table.insert(out, k.Time)
		end
		return out
	end)
	result.ks_remove = try(function()
		seq:RemoveKeyframe(k1)
		return k1.Parent
	end)
	result.ks_remove_other = try(function()
		return seq:RemoveKeyframe(Instance.new("Keyframe"))
	end)
	local pose = Instance.new("Pose")
	pose.Name = "LowerTorso"
	result.kf_add_pose = try(function()
		k0:AddPose(pose)
		return pose.Parent == k0
	end)
	result.kf_add_pose_bad = try(function()
		return k0:AddPose(Instance.new("Keyframe"))
	end)
	result.kf_add_number_pose = try(function()
		local n = Instance.new("NumberPose")
		k0:AddPose(n)
		return n.Parent == k0
	end)
	result.kf_poses = try(function()
		return #k0:GetPoses()
	end)
	local marker = Instance.new("KeyframeMarker")
	marker.Name = "Step"
	result.kf_add_marker = try(function()
		k0:AddMarker(marker)
		return marker.Parent == k0
	end)
	result.kf_add_marker_bad = try(function()
		return k0:AddMarker(Instance.new("Pose"))
	end)
	result.kf_markers = try(function()
		return k0:GetMarkers()
	end)
	result.kf_remove_marker = try(function()
		k0:RemoveMarker(marker)
		return marker.Parent
	end)
	result.kf_remove_pose = try(function()
		k0:RemovePose(pose)
		return pose.Parent
	end)
	local sub = Instance.new("Pose")
	sub.Name = "UpperTorso"
	result.pose_add = try(function()
		pose:AddSubPose(sub)
		return sub.Parent == pose
	end)
	result.pose_add_bad = try(function()
		return pose:AddSubPose(Instance.new("Folder"))
	end)
	result.pose_subs = try(function()
		return pose:GetSubPoses()
	end)
	result.pose_remove = try(function()
		pose:RemoveSubPose(sub)
		return sub.Parent
	end)

	-- Providers.
	local ksp = game:GetService("KeyframeSequenceProvider")
	result.ksp_active = try(function()
		return typeof(ksp:RegisterActiveKeyframeSequence(seq))
	end)
	result.ksp_active_prefix = try(function()
		return string.sub(ksp:RegisterActiveKeyframeSequence(seq), 1, 14)
	end)
	result.ksp_get_bad = try(function()
		return ksp:GetKeyframeSequenceAsync("rbxassetid://0")
	end)
	local acp = game:GetService("AnimationClipProvider")
	result.acp_register = try(function()
		return string.sub(acp:RegisterAnimationClip(seq), 1, 14)
	end)
	result.acp_active = try(function()
		return string.sub(acp:RegisterActiveAnimationClip(seq), 1, 14)
	end)
	result.acp_register_bad = try(function()
		return acp:RegisterAnimationClip(Instance.new("Folder"))
	end)
	-- The message names the place; ours runs as place 0.
	result.acp_get_bad = try(function()
		local ok, err = pcall(acp.GetAnimationClipAsync, acp, "rbxassetid://0")
		if ok then
			return err
		end
		local text = string.gsub(tostring(err), "serverplaceid=%d+", "serverplaceid=N")
		error(text, 0)
	end)

	-- Animator on a rig.
	local model, humanoid = h.character(origin + Vector3.new(0, 0, 0), "R15")
	local animator = Instance.new("Animator")
	animator.Parent = humanoid
	local anim = Instance.new("Animation")
	anim.AnimationId = "rbxassetid://507770239"
	local track = animator:LoadAnimation(anim)
	result.an_by_id = try(function()
		return animator:GetTrackByAnimationId(anim.AnimationId) == track
	end)
	result.an_by_id_missing = try(function()
		return animator:GetTrackByAnimationId("rbxassetid://1")
	end)
	result.an_step = try(function()
		return animator:StepAnimations(0.1)
	end)
	result.an_velocities = try(function()
		return animator:ApplyJointVelocities({})
	end)
	result.an_parallel = try(function()
		return animator:RegisterEvaluationParallelCallback(function() end)
	end)
	result.tr_params = try(function()
		return track:GetParameterDefaults()
	end)
	result.tr_param_missing = try(function()
		return track:GetParameter("Speed")
	end)
	result.tr_set_param = try(function()
		return track:SetParameter("Speed", 1)
	end)
	result.tr_targets = try(function()
		return track:GetTargetNames()
	end)
	result.tr_target_missing = try(function()
		return track:GetTargetInstance("Foo")
	end)
	result.tr_set_target = try(function()
		return track:SetTargetInstance("Foo", workspace)
	end)
	model:Destroy()

	-- Rig descriptions.
	local rig = Instance.new("HumanoidRigDescription")
	result.rig_r15 = try(function()
		return rig:GetR15JointNames()
	end)
	result.rig_r15_labels = try(function()
		return rig:GetR15JointLabels()
	end)
	result.rig_r6 = try(function()
		return rig:GetR6JointNames()
	end)
	result.rig_names = try(function()
		return rig:GetJointNames()
	end)
	result.rig_joint = try(function()
		return rig:GetJoint(Enum.RigLabel.Head)
	end)
	result.rig_joint_name = try(function()
		return rig:GetJointFromName("Head")
	end)
	result.rig_set = try(function()
		local b = Instance.new("Bone")
		b.Name = "HeadBone"
		rig:SetJoint(Enum.RigLabel.Head, b)
		return rig:GetJoint(Enum.RigLabel.Head)
	end)
	result.rig_set_bad = try(function()
		return rig:SetJoint(Enum.RigLabel.Head, Instance.new("Folder"))
	end)
	result.rig_tpose = try(function()
		return rig:SetTposeAdjustment(Enum.RigLabel.Head, CFrame.Angles(0.1, 0, 0))
	end)
	local digits = Instance.new("DigitsRigDescription")
	result.dig_tip = try(function()
		return digits:GetFingerTip(1)
	end)
	result.dig_set_tip = try(function()
		digits:SetFingerTip(1, Vector3.new(1, 2, 3))
		return digits:GetFingerTip(1)
	end)
	result.dig_control = try(function()
		return digits:GetFingerControl(2)
	end)
	result.dig_bad = try(function()
		return digits:GetFingerTip(99)
	end)

	-- IKControl without a chain.
	local ik = Instance.new("IKControl")
	result.ik_count = try(function()
		return ik:GetChainCount()
	end)
	result.ik_length = try(function()
		return ik:GetChainLength()
	end)
	result.ik_raw = try(function()
		return ik:GetRawFinalTarget()
	end)
	result.ik_smoothed = try(function()
		return ik:GetSmoothedFinalTarget()
	end)
	result.ik_node = try(function()
		return ik:GetNodeWorldCFrame(1)
	end)

	-- AnimationNodeDefinition pins.
	local ok, node = pcall(Instance.new, "AnimationNodeDefinition")
	if ok then
		local fired = 0
		node.InputPinsChanged:Connect(function()
			fired += 1
		end)
		result.node_pins = try(function()
			return node:GetOrderedInputPinNames()
		end)
		result.node_add = try(function()
			node:AddInputPin("A")
			node:AddInputPin("B")
			return node:GetOrderedInputPinNames()
		end)
		result.node_add_same = try(function()
			return node:AddInputPin("A")
		end)
		result.node_set = try(function()
			node:SetOrderedInputPinNames({ "B", "A" })
			return node:GetOrderedInputPinNames()
		end)
		result.node_set_bad = try(function()
			return node:SetOrderedInputPinNames({ "C" })
		end)
		result.node_remove = try(function()
			node:RemoveInputPin("A")
			return node:GetOrderedInputPinNames()
		end)
		result.node_remove_missing = try(function()
			return node:RemoveInputPin("Z")
		end)
		h.beats(2)
		result.node_fired = fired
	else
		result.node_new = "err: " .. tostring(node)
	end
	return result
end
