-- The Terrain API's voxel edits read back with ReadVoxels: what each Fill*
-- leaves in the cells it partly covers, ReplaceMaterial, WriteVoxels, the
-- cell conversions and the material colours. Occupancies along a line of
-- cells through each shape (`<case>_<i>`), the materials as text.
return function(h, origin)
	local terrain = workspace.Terrain
	local result = {}
	-- A 6x6x6 cell box at `origin`, cleared before each case.
	local box = Region3.new(origin, origin + Vector3.new(24, 24, 24))
	local function clear()
		terrain:FillRegion(box, 4, Enum.Material.Air)
	end
	local function read()
		return terrain:ReadVoxels(box, 4)
	end
	-- Occupancy and material of cells (1..6, y, z), y and z in cells.
	local function line(name, y, z)
		local materials, occupancies = read()
		local names = {}
		for x = 1, 6 do
			result[name .. "_" .. x] = h.round(occupancies[x][y][z], 3)
			table.insert(names, materials[x][y][z].Name)
		end
		result[name .. "_mat"] = table.concat(names, ",")
	end
	local function at(x, y, z)
		return origin + Vector3.new(x, y, z)
	end

	clear()
	-- Studs 6..18 on X, cells 2..5 partly or fully covered.
	terrain:FillBlock(CFrame.new(at(12, 12, 12)), Vector3.new(11, 6, 6), Enum.Material.Grass)
	line("block", 4, 4)
	local _, occ = read()
	result.block_corner = h.round(occ[2][3][3], 3)
	result.block_edge = h.round(occ[2][3][4], 3)

	clear()
	terrain:FillBlock(CFrame.new(at(12, 12, 12)) * CFrame.Angles(0, math.rad(45), 0), Vector3.new(8, 8, 8), Enum.Material.Rock)
	line("rotated", 4, 4)

	clear()
	terrain:FillBall(at(12, 12, 12), 7, Enum.Material.Sand)
	line("ball", 4, 4)
	_, occ = read()
	result.ball_diag = h.round(occ[5][5][5], 3)

	clear()
	terrain:FillCylinder(CFrame.new(at(12, 12, 12)), 8, 6, Enum.Material.Slate)
	line("cylinder", 4, 4)
	_, occ = read()
	result.cylinder_top = h.round(occ[4][5][4], 3)

	-- Occupancy bytes of a slice through x = 4, y rows from the bottom.
	local function slice(name, axis)
		local _, o = read()
		local rows = {}
		for a = 1, 6 do
			local row = {}
			for b = 1, 6 do
				local v = if axis == "yz" then o[4][a][b] else o[b][a][4]
				table.insert(row, string.format("%d", math.floor(v * 256 + 0.5) - 1))
			end
			table.insert(rows, table.concat(row, ","))
		end
		result["_" .. name] = table.concat(rows, " / ")
	end
	slice("cylinder_xy", "xy")
	clear()
	terrain:FillCylinder(CFrame.new(at(12, 12, 12)), 16, 4, Enum.Material.Slate)
	line("cyl_tall", 4, 4)
	slice("cyl_tall_xy", "xy")
	clear()
	terrain:FillCylinder(CFrame.new(at(12, 12, 12)), 4, 8, Enum.Material.Slate)
	line("cyl_flat", 4, 4)
	slice("cyl_flat_xy", "xy")
	clear()
	terrain:FillCylinder(CFrame.new(at(12, 12, 12)) * CFrame.Angles(0, 0, math.rad(90)), 8, 6, Enum.Material.Slate)
	slice("cyl_turned_xy", "xy")

	clear()
	terrain:FillWedge(CFrame.new(at(12, 12, 12)), Vector3.new(16, 16, 16), Enum.Material.Ice)
	line("wedge_low", 2, 4)
	line("wedge_mid", 4, 4)
	-- A wedge's slope climbs along its -Z: along Z at the middle height.
	_, occ = read()
	local wedgeZ = {}
	for z = 1, 6 do
		table.insert(wedgeZ, h.round(occ[4][4][z], 3))
	end
	result.wedge_z = table.concat(wedgeZ, ",")
	slice("wedge_yz", "yz")
	clear()
	terrain:FillWedge(CFrame.new(at(12, 12, 12)), Vector3.new(16, 8, 16), Enum.Material.Ice)
	slice("wedge_low_yz", "yz")
	clear()
	terrain:FillWedge(CFrame.new(at(12, 12, 12)) * CFrame.Angles(0, math.rad(180), 0), Vector3.new(16, 16, 16), Enum.Material.Ice)
	slice("wedge_turned_yz", "yz")

	clear()
	terrain:FillBlock(CFrame.new(at(12, 12, 12)), Vector3.new(8, 8, 8), Enum.Material.Water)
	line("water", 4, 4)

	clear()
	terrain:FillRegion(Region3.new(at(4, 4, 4), at(12, 12, 12)), 4, Enum.Material.Rock)
	terrain:ReplaceMaterial(box, 4, Enum.Material.Rock, Enum.Material.Sand)
	line("replaced", 2, 2)

	-- Air carved out of a full block.
	clear()
	terrain:FillRegion(box, 4, Enum.Material.Mud)
	terrain:FillBlock(CFrame.new(at(12, 12, 12)), Vector3.new(6, 6, 6), Enum.Material.Air)
	line("carved", 4, 4)

	clear()
	local materials, occupancies = read()
	for x = 1, 6 do
		materials[x][2][2] = Enum.Material.Basalt
		occupancies[x][2][2] = x / 6 - 0.05
	end
	materials[3][2][2] = Enum.Material.Air
	terrain:WriteVoxels(box, 4, materials, occupancies)
	line("written", 2, 2)
	result.size = h.vec(materials.Size, 0)

	result.cell_center = h.vec(terrain:CellCenterToWorld(1, -2, 3), 2)
	result.cell_corner = h.vec(terrain:CellCornerToWorld(1, -2, 3), 2)
	result.world_cell = h.vec(terrain:WorldToCell(Vector3.new(5, -1, 9.5)), 2)
	result.world_cell_solid = h.vec(terrain:WorldToCellPreferSolid(Vector3.new(4, 8, -4)), 2)
	result.world_cell_empty = h.vec(terrain:WorldToCellPreferEmpty(Vector3.new(4, 8, -4)), 2)
	local original = terrain:GetMaterialColor(Enum.Material.Grass)
	local grass = original
	result.grass_color = h.vec(Vector3.new(grass.R, grass.G, grass.B) * 255, 0)
	terrain:SetMaterialColor(Enum.Material.Grass, Color3.fromRGB(10, 200, 30))
	grass = terrain:GetMaterialColor(Enum.Material.Grass)
	result.grass_set = h.vec(Vector3.new(grass.R, grass.G, grass.B) * 255, 0)
	terrain:SetMaterialColor(Enum.Material.Grass, original)
	terrain:FillBlock(CFrame.new(at(12, 12, 12)), Vector3.new(8, 8, 8), Enum.Material.Grass)
	terrain:Clear()
	_, occ = read()
	result.cleared = h.round(occ[3][3][3], 3)
	return result
end
