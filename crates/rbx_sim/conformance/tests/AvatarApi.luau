-- The avatar API as a live server answers it: Humanoid's accessories, body
-- parts and limbs, move velocity, BuildRigFromAttachments, descriptions and
-- emotes; HumanoidDescription's emotes and accessories; Player and Players
-- members a script can call without changing anyone's account (the ban
-- calls only with configs Roblox refuses). Needs a player.
return function(h, origin)
	local Players = game:GetService("Players")
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
	h.floor(origin)

	-- Accessories on an R15 rig.
	local model, humanoid, root = h.character(origin + Vector3.new(0, 0, 0), "R15")
	local head = model.Head
	h.attach(head, CFrame.new(0, 0.6, 0)).Name = "HatAttachment"
	local function accessory(name, attachmentName, class)
		local a = Instance.new(class or "Accessory")
		a.Name = name
		local handle = Instance.new("Part")
		handle.Name = "Handle"
		handle.Size = Vector3.new(1, 1, 1)
		handle.Parent = a
		if attachmentName then
			h.attach(handle, CFrame.new(0, -0.5, 0)).Name = attachmentName
		end
		return a
	end
	result.acc_none = try(function()
		return humanoid:GetAccessories()
	end)
	local hat = accessory("Hat", "HatAttachment")
	local before = {}
	for _, d in model:GetDescendants() do
		before[d] = true
	end
	for _, d in hat:GetDescendants() do
		before[d] = true
	end
	result.acc_add = try(function()
		return humanoid:AddAccessory(hat)
	end)
	h.beats(2)
	result.acc_new = try(function()
		local out = {}
		for _, d in model:GetDescendants() do
			if not before[d] and d ~= hat then
				local entry = d.ClassName .. ":" .. d.Name .. "@" .. d.Parent.Name
				if d:IsA("RigidConstraint") or d:IsA("WeldConstraint") or d:IsA("JointInstance") then
					local a0 = d:IsA("RigidConstraint") and d.Attachment0 or d.Part0
					local a1 = d:IsA("RigidConstraint") and d.Attachment1 or d.Part1
					entry ..= "/" .. show(a0) .. ">" .. show(a1)
				end
				table.insert(out, entry)
			end
		end
		table.sort(out)
		return out
	end)
	result.acc_parent = try(function()
		return hat.Parent
	end)
	local function welds(a)
		local out = {}
		for _, d in model:GetDescendants() do
			if (d:IsA("JointInstance") or d:IsA("WeldConstraint")) and (d.Part0 == a.Handle or d.Part1 == a.Handle) then
				local w = { d.ClassName, d.Name, show(d.Parent), show(d.Part0), show(d.Part1) }
				if d:IsA("JointInstance") then
					table.insert(w, show(d.C0))
					table.insert(w, show(d.C1))
				end
				table.insert(out, w)
			end
		end
		return out
	end
	result.acc_weld_now = try(welds, hat)
	h.beats(2)
	result.acc_weld = try(welds, hat)
	result.acc_handle_pos = try(function()
		return hat.Handle.Position - head.Position
	end)
	local legacy = accessory("Legacy", nil, "Hat")
	result.acc_add_legacy = try(function()
		humanoid:AddAccessory(legacy)
		h.beats(2)
		return { welds(legacy), show(legacy.Handle.Position - head.Position) }
	end)
	local loose = accessory("Loose", "NoSuchAttachment")
	result.acc_add_unmatched = try(function()
		humanoid:AddAccessory(loose)
		h.beats(2)
		return { show(loose.Parent), welds(loose) }
	end)
	result.acc_add_part = try(function()
		return humanoid:AddAccessory(Instance.new("Part"))
	end)
	result.acc_add_nil = try(function()
		return humanoid:AddAccessory(nil)
	end)
	result.acc_list = try(function()
		local names = {}
		for _, a in humanoid:GetAccessories() do
			table.insert(names, a.ClassName .. ":" .. a.Name)
		end
		table.sort(names)
		return names
	end)
	result.acc_remove = try(function()
		humanoid:RemoveAccessories()
		return { #humanoid:GetAccessories(), show(hat.Parent) }
	end)

	-- Body parts and limbs.
	for _, name in { "Head", "LeftUpperArm", "RightFoot", "HumanoidRootPart", "UpperTorso" } do
		result["bp_" .. name] = try(function()
			return humanoid:GetBodyPartR15(model[name])
		end)
		result["limb_" .. name] = try(function()
			return humanoid:GetLimb(model[name])
		end)
	end
	result.bp_other = try(function()
		return humanoid:GetBodyPartR15(workspace)
	end)
	result.limb_nil = try(function()
		return humanoid:GetLimb(nil)
	end)
	local r6, r6h = h.character(origin + Vector3.new(12, 0, 0), "R6")
	for _, name in { "Head", "Torso", "Left Arm", "Right Leg", "HumanoidRootPart" } do
		result["r6limb_" .. name] = try(function()
			return r6h:GetLimb(r6[name])
		end)
		result["r6bp_" .. name] = try(function()
			return r6h:GetBodyPartR15(r6[name])
		end)
	end
	r6:Destroy()
	result.replace = try(function()
		local hand = Instance.new("Part")
		hand.Name = "NewHand"
		hand.Size = Vector3.new(1, 0.3, 1)
		local old = model.LeftHand
		local ok = humanoid:ReplaceBodyPartR15(Enum.BodyPartR15.LeftHand, hand)
		local wrist
		for _, d in model:GetDescendants() do
			if d:IsA("Motor6D") and d.Name == "LeftWrist" then
				wrist = d
			end
		end
		return {
			ok,
			hand.Name,
			show(hand.Parent),
			show(old.Parent),
			wrist and show(wrist.Part1) or "no wrist",
			wrist and show(wrist.Parent) or "",
			wrist and show(wrist.C1) or "",
			show(hand.Position - model.LeftLowerArm.Position),
		}
	end)
	result.replace_root = try(function()
		return humanoid:ReplaceBodyPartR15(Enum.BodyPartR15.RootPart, Instance.new("Part"))
	end)
	result.replace_unknown = try(function()
		return humanoid:ReplaceBodyPartR15(Enum.BodyPartR15.Unknown, Instance.new("Part"))
	end)

	-- Move velocity.
	h.beats(30)
	result.move_still = try(function()
		return humanoid:GetMoveVelocity()
	end)
	result.floor_still = try(function()
		return humanoid:GetRelativeVelocityAtFloor()
	end)
	humanoid:Move(Vector3.new(1, 0, 0))
	h.beats(20)
	result.move_walk = try(function()
		return humanoid:GetMoveVelocity()
	end)
	result.floor_walk = try(function()
		local v = humanoid:GetRelativeVelocityAtFloor()
		return h.vec(v, 0)
	end)
	humanoid.WalkSpeed = 8
	h.beats(2)
	result.move_slow = try(function()
		return humanoid:GetMoveVelocity()
	end)
	humanoid:Move(Vector3.zero)
	result.signals = try(function()
		return { typeof(humanoid.Strafing), typeof(humanoid.Touched), typeof(humanoid.ApplyDescriptionFinished) }
	end)
	local touched = {}
	humanoid.Touched:Connect(function(other, mine)
		table.insert(touched, other.Name .. "/" .. mine.Name)
	end)
	h.part({ Name = "Toucher", Size = Vector3.new(2, 2, 2), Position = root.Position + Vector3.new(0, 12, 0) })
	h.beats(60)
	result.touched = #touched > 0
	model:Destroy()

	-- BuildRigFromAttachments: parts with matching RigAttachments.
	local built = Instance.new("Model")
	local function part(name, y)
		local p = Instance.new("Part")
		p.Name = name
		p.Anchored = true
		p.Position = origin + Vector3.new(30, y, 0)
		p.Size = Vector3.new(2, 1, 1)
		p.Parent = built
		return p
	end
	local broot = part("HumanoidRootPart", 3)
	local lower = part("LowerTorso", 2)
	local torso = part("UpperTorso", 3)
	local bhead = part("Head", 4.5)
	local hand = part("LeftHand", 3)
	h.attach(broot, CFrame.new(0, -0.5, 0)).Name = "RootRigAttachment"
	h.attach(lower, CFrame.new(0, 0.5, 0)).Name = "RootRigAttachment"
	h.attach(lower, CFrame.new(0, 0.5, 0)).Name = "WaistRigAttachment"
	h.attach(torso, CFrame.new(0, -0.5, 0)).Name = "WaistRigAttachment"
	h.attach(torso, CFrame.new(0, 0.5, 0)).Name = "NeckRigAttachment"
	h.attach(bhead, CFrame.new(0, -0.5, 0)).Name = "NeckRigAttachment"
	h.attach(torso, CFrame.new(1, 0, 0)).Name = "SpareRigAttachment"
	h.attach(hand, CFrame.new(-1, 0, 0)).Name = "SpareRigAttachment"
	for _, p in built:GetChildren() do
		p.Anchored = p.Name == "HumanoidRootPart"
	end
	local bh = Instance.new("Humanoid")
	bh.RigType = Enum.HumanoidRigType.R15
	bh.Parent = built
	built.Parent = workspace
	local function rigged()
		local joints = {}
		for _, d in built:GetDescendants() do
			if d:IsA("JointInstance") then
				table.insert(
					joints,
					d.ClassName .. ":" .. d.Name .. ":" .. show(d.Part0) .. ">" .. show(d.Part1) .. "@" .. d.Parent.Name
				)
			elseif not d:IsA("BasePart") and not d:IsA("Attachment") and d ~= bh then
				table.insert(joints, d.ClassName .. ":" .. d.Name .. "@" .. d.Parent.Name)
			end
		end
		table.sort(joints)
		return joints
	end
	result.build = try(function()
		bh:BuildRigFromAttachments()
		return rigged()
	end)
	h.beats(2)
	result.build_later = try(rigged)
	-- The same with the root part set by hand (a fresh Humanoid finds it on
	-- its own only once it's parented).
	bh.Parent = nil
	bh.Parent = built
	result.build_root = try(function()
		return show(bh.RootPart)
	end)
	result.build_again = try(function()
		bh:BuildRigFromAttachments()
		return rigged()
	end)
	built:Destroy()

	-- Descriptions on a rig.
	local dmodel, dh = h.character(origin + Vector3.new(-12, 0, 0), "R15")
	local finished = {}
	dh.ApplyDescriptionFinished:Connect(function(d)
		table.insert(finished, typeof(d) .. ":" .. d.ClassName)
	end)
	result.apply_bad = try(function()
		return dh:ApplyDescriptionAsync(Instance.new("Folder"))
	end)
	result.apply_nil = try(function()
		return dh:ApplyDescriptionAsync(nil)
	end)
	result.apply_reset_bad = try(function()
		return dh:ApplyDescriptionResetAsync(nil)
	end)
	result.applied_none = try(function()
		local d = dh:GetAppliedDescription()
		return { d.ClassName, d.Parent == nil, d.HeadColor.R }
	end)
	result.emote_none = try(function()
		return dh:PlayEmoteAsync("wave")
	end)
	result.avatar_rules = try(function()
		return dh:ApplyAvatarRulesAsync(nil)
	end)
	h.beats(2)
	result.finished_count = #finished
	dmodel:Destroy()

	-- HumanoidDescription data.
	local desc = Instance.new("HumanoidDescription")
	local emoteEvents, equipEvents = {}, {}
	desc.EmotesChanged:Connect(function(e)
		table.insert(emoteEvents, show(e))
	end)
	desc.EquippedEmotesChanged:Connect(function(e)
		table.insert(equipEvents, show(e))
	end)
	result.d_emotes = try(function()
		return desc:GetEmotes()
	end)
	result.d_equipped = try(function()
		return desc:GetEquippedEmotes()
	end)
	result.d_add = try(function()
		desc:AddEmote("Wave", 507770239)
		desc:AddEmote("Wave", 507770818)
		desc:AddEmote("Point", 507770453)
		return desc:GetEmotes()
	end)
	result.d_add_bad = try(function()
		return desc:AddEmote("Bad", "x")
	end)
	result.d_equip = try(function()
		desc:SetEquippedEmotes({ "Wave", "Point" })
		return desc:GetEquippedEmotes()
	end)
	result.d_equip_slots = try(function()
		desc:SetEquippedEmotes({ { Slot = 3, Name = "Wave" }, { Slot = 8, Name = "Point" } })
		return desc:GetEquippedEmotes()
	end)
	result.d_equip_missing = try(function()
		desc:SetEquippedEmotes({ "Dance" })
		return desc:GetEquippedEmotes()
	end)
	result.d_equip_bad = try(function()
		return desc:SetEquippedEmotes("Wave")
	end)
	result.d_remove = try(function()
		desc:RemoveEmote("Point")
		return { desc:GetEmotes(), desc:GetEquippedEmotes() }
	end)
	result.d_set = try(function()
		desc:SetEmotes({ Salute = { 3360689775 }, Shrug = { 3576968026, 1 } })
		return desc:GetEmotes()
	end)
	result.d_set_bad = try(function()
		return desc:SetEmotes({ Salute = 5 })
	end)
	result.d_emote_props = try(function()
		return desc.EmotesDataInternal
	end)
	h.beats(2)
	result.d_emote_events = emoteEvents
	result.d_equip_events = equipEvents

	local adesc = Instance.new("HumanoidDescription")
	result.a_none = try(function()
		return adesc:GetAccessories(true)
	end)
	result.a_props = try(function()
		adesc.HatAccessory = "1,2"
		adesc.BackAccessory = "3"
		return adesc:GetAccessories(true)
	end)
	result.a_props_layered_only = try(function()
		return adesc:GetAccessories(false)
	end)
	result.a_set = try(function()
		adesc:SetAccessories({
			{ AssetId = 10, AccessoryType = Enum.AccessoryType.Hat },
			{ AssetId = 11, AccessoryType = Enum.AccessoryType.Face },
			{ AssetId = 12, AccessoryType = Enum.AccessoryType.Jacket, IsLayered = true, Order = 2, Puffiness = 0.5 },
		}, true)
		return { adesc.HatAccessory, adesc.FaceAccessory, adesc.BackAccessory, adesc:GetAccessories(true) }
	end)
	result.a_set_layered_only = try(function()
		adesc:SetAccessories({
			{ AssetId = 13, AccessoryType = Enum.AccessoryType.Shirt, IsLayered = true, Order = 1 },
		}, false)
		return { adesc.HatAccessory, adesc:GetAccessories(true) }
	end)
	result.a_set_rigid_refused = try(function()
		return adesc:SetAccessories({ { AssetId = 14, AccessoryType = Enum.AccessoryType.Hat } }, false)
	end)
	result.a_set_bad = try(function()
		return adesc:SetAccessories({ { AssetId = 15 } }, true)
	end)
	result.a_children = try(function()
		local out = {}
		for _, c in adesc:GetChildren() do
			table.insert(out, c.ClassName)
		end
		table.sort(out)
		return out
	end)
	local aDesc = Instance.new("AccessoryDescription")
	result.ad_applied = try(function()
		return aDesc:GetAppliedInstance()
	end)

	-- Player and Players.
	local player = h.player()
	result.p_camera_state = try(function()
		return player:GetCameraState()
	end)
	result.p_signals = try(function()
		return { typeof(player.Idled), typeof(player.OnTeleport), typeof(player.EmotesLoaded) }
	end)
	result.p_clear_cached = try(function()
		return player:ClearCachedAvatarAppearance()
	end)
	result.p_age = try(function()
		return player:PromptAgeCheck()
	end)
	result.p_load_bad = try(function()
		return player:LoadCharacterWithHumanoidDescriptionAsync(nil)
	end)
	result.p_rules = try(function()
		return player:LoadCharacterWithAvatarRulesAsync(nil)
	end)
	result.ps_signals = try(function()
		return { typeof(Players.PlayerMembershipChanged), typeof(Players.UserSubscriptionStatusChanged) }
	end)
	result.ban_empty = try(function()
		return Players:BanAsync({})
	end)
	result.ban_no_ids = try(function()
		return Players:BanAsync({ Duration = 60, DisplayReason = "x", PrivateReason = "y" })
	end)
	result.ban_bad_duration = try(function()
		return Players:BanAsync({ UserIds = { player.UserId }, Duration = 0, DisplayReason = "x", PrivateReason = "y" })
	end)
	result.ban_string = try(function()
		return Players:BanAsync("x")
	end)
	result.ban_long_reason = try(function()
		return Players:BanAsync({
			UserIds = { player.UserId },
			Duration = 60,
			DisplayReason = string.rep("a", 401),
			PrivateReason = "y",
		})
	end)
	result.unban_empty = try(function()
		return Players:UnbanAsync({})
	end)
	result.unban_string = try(function()
		return Players:UnbanAsync("x")
	end)
	result.ban_history_type = try(function()
		return typeof(Players:GetBanHistoryAsync(1))
	end)
	result.ban_history_page = try(function()
		local pages = Players:GetBanHistoryAsync(1)
		return { pages.ClassName, pages.IsFinished, #pages:GetCurrentPage() }
	end)
	result.ban_history_bad = try(function()
		return Players:GetBanHistoryAsync("x")
	end)
	result.outfit_bad = try(function()
		return Players:GetHumanoidDescriptionFromOutfitIdAsync(0)
	end)
	result.profile = try(function()
		local p = Players:GetProfileConfigurationFromUserIdAsync(1)
		local keys = {}
		for k in p do
			table.insert(keys, k)
		end
		table.sort(keys)
		return keys
	end)

	-- Teams and AvatarCreationService.
	result.teams = try(function()
		return #game:GetService("Teams"):GetTeams()
	end)
	-- The rest of AvatarCreationService, with arguments it refuses (no
	-- prompt or generation is ever started).
	local acs = game:GetService("AvatarCreationService")
	local refused = {
		AutoSetupAvatarAsync = {},
		GenerateAvatar2DPreviewAsync = {},
		GenerateAvatarAsync = {},
		GetBatchTokenDetailsAsync = { {} },
		LoadAvatar2DPreviewAsync = { "" },
		LoadGeneratedAvatarAsync = { "" },
		PrepareAvatarForPreviewAsync = {},
		PromptCreateAvatarAssetAsync = {},
		PromptCreateAvatarAsync = {},
		PromptCreateMakeupAsync = {},
		PromptSelectAvatarGenerationImageAsync = {},
		RequestAvatarGenerationSessionAsync = {},
		ValidateUGCAccessoryAsync = {},
		ValidateUGCBodyPartAsync = {},
		ValidateUGCFullBodyAsync = {},
	}
	for name, args in refused do
		result["acs_" .. name] = try(function()
			return acs[name](acs, table.unpack(args))
		end)
	end
	result.acs_signals = try(function()
		return {
			typeof(acs.AvatarAssetModerationCompleted),
			typeof(acs.AvatarModerationCompleted),
			typeof(acs.AvatarOutfitModerationCompleted),
		}
	end)
	result.acp_evaluator = try(function()
		local acp = game:GetService("AnimationClipProvider")
		return acp:GetClipEvaluatorAsync(nil)
	end)
	result.aes_bundles = try(function()
		local list = game:GetService("AvatarEditorService"):GetBundlesByAssetIdAsync(nil)
		return typeof(list)
	end)
	result.acs_rules = try(function()
		local rules = game:GetService("AvatarCreationService"):GetValidationRules()
		local keys = {}
		for k in rules do
			table.insert(keys, k)
		end
		table.sort(keys)
		return keys
	end)
	local aes = game:GetService("AvatarEditorService")
	result.aes_rules = try(function()
		local rules = aes:GetAvatarRulesAsync()
		local keys = {}
		for k in rules do
			table.insert(keys, k)
		end
		table.sort(keys)
		return keys
	end)
	result.aes_inventory = try(function()
		return aes:GetInventoryAsync({ Enum.AvatarAssetType.Hat })
	end)
	result.aes_outfits = try(function()
		return aes:GetOutfitsAsync()
	end)
	result.aes_conform = try(function()
		return aes:ConformToAvatarRulesAsync(Instance.new("HumanoidDescription")).ClassName
	end)
	result.aes_default_clothing = try(function()
		local d = aes:CheckApplyDefaultClothingAsync(Instance.new("HumanoidDescription"))
		return { d.ClassName, d.Shirt, d.Pants }
	end)
	result.aes_head_shapes = try(function()
		return typeof(aes:GetHeadShapesAsync())
	end)
	result.aes_item = try(function()
		local d = aes:GetItemDetailsAsync(1028606, Enum.AvatarItemType.Asset)
		return { d.Id, d.Name, d.AssetType, d.ItemType }
	end)
	result.aes_search = try(function()
		local params = CatalogSearchParams.new()
		params.SearchKeyword = "hat"
		local pages = aes:SearchCatalogAsync(params)
		return { pages.ClassName, typeof(pages:GetCurrentPage()) }
	end)
	-- The shapes of live catalog answers: each key with its value's type.
	local function shape(t)
		if type(t) ~= "table" then
			return typeof(t)
		end
		local list = {}
		for k, v in t do
			table.insert(list, tostring(k) .. ":" .. typeof(v))
		end
		table.sort(list)
		return table.concat(list, ",")
	end
	result.aes_item_shape = try(function()
		return shape(aes:GetItemDetailsAsync(1028606, Enum.AvatarItemType.Asset))
	end)
	result.aes_bundle_shape = try(function()
		return shape(aes:GetItemDetailsAsync(192, Enum.AvatarItemType.Bundle))
	end)
	result.aes_bundles_of = try(function()
		local got = aes:GetBundlesByAssetIdAsync(139607570)
		if typeof(got) == "Instance" then
			local page = got:GetCurrentPage()
			return { got.ClassName, got.IsFinished, #page > 0, shape(page[1]) }
		end
		return { typeof(got), shape(got[1]) }
	end)
	result.aes_search_shape = try(function()
		local params = CatalogSearchParams.new()
		params.SearchKeyword = "hat"
		return shape(aes:SearchCatalogAsync(params):GetCurrentPage()[1])
	end)
	return result
end
