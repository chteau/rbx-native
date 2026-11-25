-- The shapes TerrainHumanoid walks on: a 30 and a 60 degree FillWedge on
-- a slab, their voxels (a slice through the middle) and their collision
-- surface (rays straight down every stud along the slope); and the
-- default colour of every terrain material.
return function(h, origin)
	local terrain = workspace.Terrain
	local function at(x, y, z)
		return origin + Vector3.new(x, y, z)
	end
	local result = {}
	terrain:FillBlock(CFrame.new(at(0, -8, 0)), Vector3.new(96, 16, 96), Enum.Material.Grass)
	for i, angle in { 30, 60 } do
		local x = -40 + i * 20
		local length = 12 / math.tan(math.rad(angle))
		terrain:FillWedge(CFrame.new(at(x, 6, length / 2 + 4)), Vector3.new(12, 12, length), Enum.Material.Rock)
	end
	h.beats(30)
	for i, angle in { 30, 60 } do
		local x = -40 + i * 20
		local heights = {}
		for z = -2, 28 do
			local hit = workspace:Raycast(at(x + 0.5, 30, z + 0.5), Vector3.new(0, -60, 0))
			table.insert(heights, hit and string.format("%.2f", hit.Position.Y - origin.Y) or "miss")
		end
		result["heights" .. angle] = table.concat(heights, " ")
		-- Cells from y 0 up, z 0 up, at the wedge's middle (x + 2).
		local region = Region3.new(at(x, 0, 0), at(x + 4, 16, 28))
		local materials, occupancies = terrain:ReadVoxels(region, 4)
		local rows = {}
		for y = 1, 4 do
			local row = {}
			for z = 1, 7 do
				local o = occupancies[1][y][z]
				local m = materials[1][y][z].Name
				table.insert(row, if m == "Air" then "-" else string.format("%s%d", m:sub(1, 1), math.floor(o * 256 + 0.5) - 1))
			end
			table.insert(rows, table.concat(row, ","))
		end
		result["voxels" .. angle] = table.concat(rows, " / ")
	end
	terrain:Clear()
	local colors = {}
	for _, item in Enum.Material:GetEnumItems() do
		local ok, color = pcall(terrain.GetMaterialColor, terrain, item)
		if ok then
			table.insert(colors, string.format("%s=%d,%d,%d", item.Name, math.floor(color.R * 255 + 0.5), math.floor(color.G * 255 + 0.5), math.floor(color.B * 255 + 0.5)))
		end
	end
	result._colors = table.concat(colors, " ")
	return result
end
