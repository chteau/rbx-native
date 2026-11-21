-- The 3D world's script API as a live server answers it: Workspace's
-- counters, WorldRoot collision groups, BulkMoveTo and
-- ArePartsTouchingOthers, BasePart surface helpers, Model scaling and
-- moving, Attachment:GetConstraints, joins, terrain extents and channels,
-- and the server Camera's world queries.
return function(h, origin)
	local r = {}
	local made = {}
	local function part(props, class)
		props.Anchored = if props.Anchored == nil then true else props.Anchored
		local p = h.part(props, class)
		table.insert(made, p)
		return p
	end

	-- Workspace
	r.server_time = h.try(function()
		local t = workspace:GetServerTimeNow()
		return math.abs(t - os.time()) < 5 and typeof(t)
	end)
	r.physics_fps = h.try(function()
		local fps = workspace:GetRealPhysicsFPS()
		return fps > 50 and fps < 70
	end)
	r.throttling = h.try(function()
		return workspace:GetPhysicsThrottling()
	end)
	r.pgs = h.try(function()
		return workspace:PGSIsEnabled()
	end)
	r.awake_type = h.try(function()
		return typeof(workspace:GetNumAwakeParts())
	end)
	r.persistent_loaded = h.try(function()
		return typeof(workspace.PersistentLoaded)
	end)

	-- Collision groups through workspace (WorldRoot)
	r.cg_register = h.try(function()
		return workspace:RegisterCollisionGroup("WorldApiA")
	end)
	r.cg_registered = h.try(function()
		return workspace:IsCollisionGroupRegistered("WorldApiA")
	end)
	r.cg_set = h.try(function()
		return workspace:CollisionGroupSetCollidable("WorldApiA", "Default", false)
	end)
	r.cg_are = h.try(function()
		return workspace:CollisionGroupsAreCollidable("WorldApiA", "Default")
	end)
	r.cg_max = h.try(function()
		return workspace:GetMaxCollisionGroups()
	end)
	local tagged = part({ Name = "Grouped", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(0, 30, 0) })
	tagged.CollisionGroup = "WorldApiA"
	r.cg_rename = h.try(function()
		return workspace:RenameCollisionGroup("WorldApiA", "WorldApiB")
	end)
	r.cg_part_after_rename = tagged.CollisionGroup
	r.cg_renamed_are = h.try(function()
		return workspace:CollisionGroupsAreCollidable("WorldApiB", "Default")
	end)
	r.cg_rename_default = h.try(function()
		return workspace:RenameCollisionGroup("Default", "Other")
	end)
	r.cg_rename_missing = h.try(function()
		return workspace:RenameCollisionGroup("NoSuchGroup", "Other")
	end)
	r.cg_list = h.try(function()
		-- Only this test's groups (others may linger from earlier tests),
		-- each with the ones its mask lets it collide with.
		local groups, index = {}, {}
		for i, g in workspace:GetRegisteredCollisionGroups() do
			if g.name == "Default" or g.name:sub(1, 8) == "WorldApi" then
				table.insert(groups, g)
				index[g] = i - 1
			end
		end
		local names = {}
		for _, g in groups do
			local with = {}
			for _, other in groups do
				if bit32.btest(g.mask, bit32.lshift(1, index[other])) then
					table.insert(with, other.name)
				end
			end
			table.insert(names, g.name .. ":" .. table.concat(with, "+"))
		end
		return table.concat(names, " ")
	end)
	r.cg_unregister = h.try(function()
		return workspace:UnregisterCollisionGroup("WorldApiB")
	end)
	r.cg_part_after_unregister = tagged.CollisionGroup
	r.cg_unregister_default = h.try(function()
		return workspace:UnregisterCollisionGroup("Default")
	end)
	r.cg_physics_service_sees = h.try(function()
		game:GetService("PhysicsService"):RegisterCollisionGroup("WorldApiC")
		return workspace:IsCollisionGroupRegistered("WorldApiC")
	end)
	game:GetService("PhysicsService"):UnregisterCollisionGroup("WorldApiC")

	-- BulkMoveTo
	local a = part({ Name = "BulkA", Size = Vector3.new(1, 1, 1), Position = origin })
	local b = part({ Name = "BulkB", Size = Vector3.new(1, 1, 1), Position = origin })
	local changed = 0
	a:GetPropertyChangedSignal("CFrame"):Connect(function()
		changed += 1
	end)
	r.bulk = h.try(function()
		workspace:BulkMoveTo({ a, b }, { CFrame.new(origin + Vector3.new(0, 5, 0)), CFrame.new(origin + Vector3.new(3, 5, 0)) })
		return h.vec(a.Position - origin) .. " " .. h.vec(b.Position - origin)
	end)
	r.bulk_mismatch = h.try(function()
		workspace:BulkMoveTo({ a, b }, { CFrame.new() })
	end)
	r.bulk_not_part = h.try(function()
		workspace:BulkMoveTo({ Instance.new("Folder") }, { CFrame.new() })
	end)
	r.bulk_cframe_only = h.try(function()
		workspace:BulkMoveTo({ a }, { CFrame.new(origin) }, Enum.BulkMoveMode.FireCFrameChanged)
		return h.vec(a.Position - origin)
	end)
	task.wait()
	r.bulk_changed = changed

	-- ArePartsTouchingOthers
	local floor = part({ Name = "TouchFloor", Size = Vector3.new(10, 1, 10), Position = origin + Vector3.new(0, -20, 0) })
	local flush = part({ Name = "Flush", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(0, -19, 0) })
	local sunk = part({ Name = "Sunk", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(3, -19.1, 0) })
	local apart = part({ Name = "Apart", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(0, -15, 0) })
	r.touch_flush = h.try(function()
		return workspace:ArePartsTouchingOthers({ flush })
	end)
	r.touch_sunk = h.try(function()
		return workspace:ArePartsTouchingOthers({ sunk })
	end)
	r.touch_sunk_ignored = h.try(function()
		return workspace:ArePartsTouchingOthers({ sunk }, 0.2)
	end)
	r.touch_apart = h.try(function()
		return workspace:ArePartsTouchingOthers({ apart })
	end)
	r.touch_with_floor = h.try(function()
		return workspace:ArePartsTouchingOthers({ sunk, floor })
	end)

	-- BasePart surface helpers
	local box = part({ Name = "Box", Size = Vector3.new(2, 4, 6), CFrame = CFrame.new(origin + Vector3.new(0, 40, 0)) * CFrame.Angles(0, math.rad(30), 0) })
	local ball = part({ Name = "Ball", Shape = Enum.PartType.Ball, Size = Vector3.new(4, 4, 4), Position = origin + Vector3.new(10, 40, 0) })
	local cyl = part({ Name = "Cyl", Shape = Enum.PartType.Cylinder, Size = Vector3.new(4, 2, 2), Position = origin + Vector3.new(20, 40, 0) })
	local wedge = part({ Name = "Wedge", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(30, 40, 0) }, "WedgePart")
	local function closest(p, offset)
		return h.try(function()
			return h.vec(p:GetClosestPointOnSurface(p.Position + offset) - p.Position, 3)
		end)
	end
	r.closest_box_out = closest(box, Vector3.new(5, 1, 1))
	r.closest_box_in = closest(box, Vector3.new(0.1, 0.2, 0.3))
	r.closest_ball_out = closest(ball, Vector3.new(4, 3, 0))
	r.closest_ball_in = closest(ball, Vector3.new(0.5, 0, 0))
	r.closest_cyl_out = closest(cyl, Vector3.new(0, 5, 5))
	r.closest_cyl_end = closest(cyl, Vector3.new(6, 0.2, 0))
	r.closest_wedge = closest(wedge, Vector3.new(0, 3, 3))
	r.faces_part = h.try(function()
		return tostring(box.ResizeableFaces)
	end)
	r.faces_ball = h.try(function()
		return tostring(ball.ResizeableFaces)
	end)
	r.faces_cyl = h.try(function()
		return tostring(cyl.ResizeableFaces)
	end)
	r.faces_truss = h.try(function()
		local t = Instance.new("TrussPart")
		return tostring(t.ResizeableFaces)
	end)
	r.resize_increment = h.try(function()
		return box.ResizeIncrement
	end)
	r.physprops = h.try(function()
		local p = box.CurrentPhysicalProperties
		return string.format("%s %s %s %s %s", h.round(p.Density, 3), h.round(p.Friction, 3), h.round(p.Elasticity, 3), p.FrictionWeight, p.ElasticityWeight)
	end)
	r.physprops_ice = h.try(function()
		box.Material = Enum.Material.Ice
		local p = box.CurrentPhysicalProperties
		box.Material = Enum.Material.Plastic
		return string.format("%s %s %s", h.round(p.Density, 3), h.round(p.Friction, 3), h.round(p.Elasticity, 3))
	end)
	r.physprops_custom = h.try(function()
		box.CustomPhysicalProperties = PhysicalProperties.new(3, 0.5, 0.25)
		local p = box.CurrentPhysicalProperties
		box.CustomPhysicalProperties = nil
		return string.format("%s %s %s", h.round(p.Density, 3), h.round(p.Friction, 3), h.round(p.Elasticity, 3))
	end)
	local free = part({ Name = "Resizer", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(0, 60, 0) })
	r.resize_top = h.try(function()
		local ok = free:Resize(Enum.NormalId.Top, 3)
		return tostring(ok) .. " " .. h.vec(free.Size) .. " " .. h.vec(free.Position - origin)
	end)
	r.resize_left_neg = h.try(function()
		local ok = free:Resize(Enum.NormalId.Left, -1)
		return tostring(ok) .. " " .. h.vec(free.Size) .. " " .. h.vec(free.Position - origin)
	end)
	r.resize_too_small = h.try(function()
		local ok = free:Resize(Enum.NormalId.Front, -5)
		return tostring(ok) .. " " .. h.vec(free.Size) .. " " .. h.vec(free.Position - origin)
	end)
	local blocker = part({ Name = "Blocker", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(4, 60, 0) })
	r.resize_into = h.try(function()
		local ok = free:Resize(Enum.NormalId.Right, 3)
		return tostring(ok) .. " " .. h.vec(free.Size) .. " " .. h.vec(free.Position - origin)
	end)
	blocker:Destroy()

	-- Centre of mass
	local heavy = part({ Name = "Heavy", Anchored = false, Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(0, 80, 0) })
	local light = part({ Name = "Light", Anchored = false, Size = Vector3.new(4, 1, 1), Position = origin + Vector3.new(3, 80, 0), CustomPhysicalProperties = PhysicalProperties.new(0.35, 0.3, 0.5) })
	heavy.Anchored, light.Anchored = true, true
	h.new("WeldConstraint", { Part0 = heavy, Part1 = light }, heavy)
	r.com_assembly = h.try(function()
		return h.vec(light.AssemblyCenterOfMass - origin, 3)
	end)
	r.com_part = h.try(function()
		return h.vec(light.CenterOfMass, 3)
	end)
	r.extents = h.try(function()
		return h.vec(box.ExtentsSize, 3) .. " " .. h.vec(box.ExtentsCFrame.Position - origin, 3)
	end)

	-- Model
	local model = Instance.new("Model")
	model.Name = "Scaled"
	local m1 = part({ Name = "M1", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(0, 100, 0) })
	local m2 = part({ Name = "M2", Size = Vector3.new(1, 3, 1), Position = origin + Vector3.new(4, 100, 0) })
	local att = h.attach(m2, CFrame.new(0, 1, 0))
	local att2 = h.attach(m1, CFrame.new(0.5, 0, 0))
	local weld = h.new("Weld", { Part0 = m1, Part1 = m2, C0 = CFrame.new(4, 0, 0) }, m1)
	local rope = h.new("RopeConstraint", { Attachment0 = att2, Attachment1 = att, Length = 5 }, m1)
	local spring = h.new("SpringConstraint", { Attachment0 = att2, Attachment1 = att, FreeLength = 3, MinLength = 1, MaxLength = 6 }, m1)
	local light2 = h.new("PointLight", { Range = 10 }, m1)
	local fire = h.new("Fire", { Size = 4 }, m1)
	local mesh = h.new("SpecialMesh", { Scale = Vector3.new(1, 2, 1), Offset = Vector3.new(0, 1, 0) }, m2)
	local emitter = h.new("ParticleEmitter", { Size = NumberSequence.new(1), Speed = NumberRange.new(5, 10) }, m1)
	m1.Parent, m2.Parent = model, model
	model.PrimaryPart = m1
	model.Parent = workspace
	table.insert(made, model)
	r.scale_initial = h.try(function()
		return model:GetScale()
	end)
	r.scale_to = h.try(function()
		model:ScaleTo(2)
		return model:GetScale()
	end)
	r.scale_parts = h.vec(m1.Position - origin) .. " " .. h.vec(m2.Position - origin) .. " " .. h.vec(m1.Size) .. " " .. h.vec(m2.Size)
	r.scale_attachment = h.vec(att.Position)
	r.scale_weld = h.vec(weld.C0.Position)
	r.scale_rope = h.show(rope.Length)
	r.scale_spring = string.format("%s %s %s", spring.FreeLength, spring.MinLength, spring.MaxLength)
	r.scale_light = h.show(light2.Range)
	r.scale_fire = h.show(fire.Size)
	r.scale_mesh = h.vec(mesh.Scale) .. " " .. h.vec(mesh.Offset)
	r.scale_emitter = h.show(emitter.Size.Keypoints[1].Value) .. " " .. h.show(emitter.Speed.Min) .. " " .. h.show(emitter.Speed.Max)
	r.scale_back = h.try(function()
		model:ScaleTo(0.5)
		return h.vec(m1.Size) .. " " .. h.vec(m2.Position - origin)
	end)
	r.scale_zero = h.try(function()
		model:ScaleTo(0)
	end)
	r.scale_negative = h.try(function()
		model:ScaleTo(-1)
	end)
	r.translate = h.try(function()
		model:TranslateBy(Vector3.new(1, 2, 3))
		return h.vec(m1.Position - origin) .. " " .. h.vec(model.WorldPivot.Position - origin)
	end)
	r.translate_empty = h.try(function()
		local empty = Instance.new("Model")
		empty:TranslateBy(Vector3.new(1, 0, 0))
		return h.vec(empty.WorldPivot.Position)
	end)
	r.persistent = h.try(function()
		return #model:GetPersistentPlayers()
	end)
	r.persistent_add_nil = h.try(function()
		model:AddPersistentPlayer()
		return #model:GetPersistentPlayers()
	end)
	r.persistent_remove_nil = h.try(function()
		model:RemovePersistentPlayer()
		return #model:GetPersistentPlayers()
	end)

	-- Attachment:GetConstraints
	r.constraints = h.try(function()
		local names = {}
		for _, c in att:GetConstraints() do
			table.insert(names, c.ClassName)
		end
		table.sort(names)
		return table.concat(names, " ")
	end)
	r.constraints_none = h.try(function()
		return #Instance.new("Attachment"):GetConstraints()
	end)

	-- Joins
	local base = part({ Name = "JoinBase", Size = Vector3.new(4, 1, 4), Position = origin + Vector3.new(0, 120, 0) })
	local top = part({ Name = "JoinTop", Size = Vector3.new(2, 1, 2), Position = origin + Vector3.new(0, 121, 0) })
	local far = part({ Name = "JoinFar", Size = Vector3.new(2, 1, 2), Position = origin + Vector3.new(0, 125, 0) })
	r.join_all = h.try(function()
		workspace:JoinToOutsiders({ top, far }, Enum.JointCreationMode.All)
		local found = {}
		for _, p in { base, top, far } do
			for _, j in p:GetJoints() do
				table.insert(found, p.Name .. ">" .. j.ClassName .. "@" .. j.Parent.Name)
			end
		end
		table.sort(found)
		return table.concat(found, " ")
	end)
	r.unjoin = h.try(function()
		workspace:UnjoinFromOutsiders({ top })
		return #top:GetJoints()
	end)
	r.join_surface = h.try(function()
		workspace:JoinToOutsiders({ top }, Enum.JointCreationMode.Surface)
		return #top:GetJoints()
	end)
	r.join_none = h.try(function()
		workspace:JoinToOutsiders({ top }, Enum.JointCreationMode.None)
		return #top:GetJoints()
	end)

	-- Terrain
	local terrain = workspace.Terrain
	r.terrain_extents = h.try(function()
		return tostring(terrain.MaxExtents)
	end)
	r.terrain_count_empty = h.try(function()
		return typeof(terrain:CountCells())
	end)
	local cornerCell = Vector3.new(-2000, -100, -2000)
	local region = Region3.new(cornerCell, cornerCell + Vector3.new(8, 4, 4))
	terrain:FillBlock(CFrame.new(cornerCell + Vector3.new(2, 2, 2)), Vector3.new(4, 4, 4), Enum.Material.Rock)
	r.terrain_channels = h.try(function()
		local ch = terrain:ReadVoxelChannels(region, 4, { "SolidMaterial", "SolidOccupancy", "LiquidOccupancy" })
		local keys = {}
		for k in ch do
			table.insert(keys, tostring(k))
		end
		table.sort(keys)
		return table.concat(keys, ",")
			.. " "
			.. tostring(ch.SolidMaterial[1][1][1])
			.. " "
			.. h.round(ch.SolidOccupancy[1][1][1], 3)
			.. " "
			.. tostring(ch.SolidMaterial[2][1][1])
			.. " "
			.. h.round(ch.LiquidOccupancy[2][1][1], 3)
			.. " "
			.. tostring(ch.SolidMaterial.Size)
	end)
	r.terrain_channels_bad = h.try(function()
		return terrain:ReadVoxelChannels(region, 4, { "Nope" })
	end)
	r.terrain_write_channels = h.try(function()
		terrain:WriteVoxelChannels(region, 4, {
			SolidMaterial = { { { Enum.Material.Grass } }, { { Enum.Material.Air } } },
			SolidOccupancy = { { { 0.5 } }, { { 0 } } },
			LiquidOccupancy = { { { 0.5 } }, { { 1 } } },
		})
		local m, o = terrain:ReadVoxels(region, 4)
		return tostring(m[1][1][1]) .. " " .. h.round(o[1][1][1], 3) .. " " .. tostring(m[2][1][1]) .. " " .. h.round(o[2][1][1], 3)
	end)
	r.terrain_copy = h.try(function()
		local cell = terrain:WorldToCell(cornerCell + Vector3.new(2, 2, 2))
		local copy = terrain:CopyRegion(Region3int16.new(Vector3int16.new(cell.X, cell.Y, cell.Z), Vector3int16.new(cell.X + 1, cell.Y, cell.Z)))
		terrain:PasteRegion(copy, Vector3int16.new(cell.X, cell.Y + 2, cell.Z), true)
		local m = terrain:ReadVoxels(Region3.new(cornerCell + Vector3.new(0, 8, 0), cornerCell + Vector3.new(8, 12, 4)), 4)
		return copy.ClassName .. " " .. tostring(copy.SizeInCells) .. " " .. tostring(m[1][1][1]) .. " " .. tostring(m[2][1][1])
	end)
	r.terrain_count = h.try(function()
		return terrain:CountCells() > 0
	end)
	r.terrain_slot = h.try(function()
		return table.concat({ tostring(select(1, terrain:GetMaterialSlot(0))), tostring(select(2, terrain:GetMaterialSlot(0))), tostring(select(3, terrain:GetMaterialSlot(0))) }, " ")
	end)
	r.terrain_slot_count = h.try(function()
		return select("#", terrain:GetMaterialSlot(1))
	end)
	r.terrain_slot_bad = h.try(function()
		return terrain:GetMaterialSlot(-1)
	end)
	terrain:FillBlock(CFrame.new(cornerCell + Vector3.new(4, 6, 2)), Vector3.new(8, 12, 4), Enum.Material.Air)

	-- Camera
	local camera = workspace.CurrentCamera
	r.camera = camera and camera.ClassName or "nil"
	if camera then
		local wall = part({ Name = "Wall", Size = Vector3.new(10, 10, 1), Position = origin + Vector3.new(0, 200, -10) })
		local wall2 = part({ Name = "Wall2", Size = Vector3.new(10, 10, 1), Position = origin + Vector3.new(0, 200, -20) })
		camera.CFrame = CFrame.new(origin + Vector3.new(0, 200, 0))
		r.obscuring = h.try(function()
			return h.names(camera:GetPartsObscuringTarget({ origin + Vector3.new(0, 200, -30) }, {}))
		end)
		r.obscuring_ignore = h.try(function()
			return h.names(camera:GetPartsObscuringTarget({ origin + Vector3.new(0, 200, -30) }, { wall }))
		end)
		r.zoom = h.try(function()
			camera.CFrame = CFrame.lookAt(origin + Vector3.new(0, 200, 0), origin + Vector3.new(0, 200, -1))
			camera:ZoomToExtents(CFrame.new(origin + Vector3.new(0, 200, -30)), Vector3.new(4, 4, 4))
			return h.vec(camera.CFrame.Position - origin, 2) .. " " .. h.vec(camera.CFrame.LookVector, 3) .. " " .. h.vec(camera.Focus.Position - origin, 2)
		end)
		r.zoom_fov = h.try(function()
			camera.FieldOfView = 30
			camera:ZoomToExtents(CFrame.new(origin + Vector3.new(0, 200, -30)), Vector3.new(4, 8, 2))
			camera.FieldOfView = 70
			return h.vec(camera.CFrame.Position - origin, 2)
		end)
	end

	for _, p in made do
		p:Destroy()
	end
	return r
end
