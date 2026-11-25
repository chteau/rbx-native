-- EditableImage drawing on a server beyond the basics: aliasing off, the
-- alpha lock modes, NormalMapBlend and the newer combine types,
-- DrawImageTransformed, and projecting through an EditableMesh.
return function(h)
	local AssetService = game:GetService("AssetService")
	local r = {}
	local function pixel(image, x, y)
		if image == nil then
			return "nil"
		end
		local b = image:ReadPixelsBuffer(Vector2.new(x, y), Vector2.new(1, 1))
		return string.format("%d,%d,%d,%d", buffer.readu8(b, 0), buffer.readu8(b, 1), buffer.readu8(b, 2), buffer.readu8(b, 3))
	end
	local function sum(image)
		if image == nil then
			return -1
		end
		local b = image:ReadPixelsBuffer(Vector2.zero, image.Size)
		local total = 0
		for i = 0, buffer.len(b) - 1 do
			total += buffer.readu8(b, i)
		end
		return total
	end
	local function canvas(size, color, transparency)
		local image = AssetService:CreateEditableImage({ Size = size })
		image:DrawRectangle(Vector2.zero, size, color, transparency, Enum.ImageCombineType.Overwrite)
		return image
	end

	-- Aliasing off: hard rims.
	local hard = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
	hard:DrawCircle(Vector2.new(8, 8), 4, Color3.new(1, 1, 1), 0, Enum.ImageCombineType.Overwrite, Enum.AntiAliasing.Disabled)
	r.circle_hard_rim = pixel(hard, 12, 8)
	r.circle_hard_out = pixel(hard, 13, 8)
	r.circle_hard_diag = pixel(hard, 11, 11)
	r.circle_hard_sum = sum(hard)
	local soft = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
	soft:DrawCircle(Vector2.new(8, 8), 4, Color3.new(1, 1, 1), 0, Enum.ImageCombineType.Overwrite, Enum.AntiAliasing.Enabled)
	r.circle_soft_sum = sum(soft)
	local line = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
	line:DrawLine(Vector2.new(0, 0), Vector2.new(15, 5), Color3.new(1, 1, 1), 0, Enum.ImageCombineType.Overwrite, Enum.AntiAliasing.Disabled)
	r.line_hard_sum = sum(line)
	r.line_hard_mid = pixel(line, 8, 3)
	r.line_hard_mid_above = pixel(line, 8, 2)

	-- Alpha locks.
	local base = canvas(Vector2.new(4, 4), Color3.new(1, 0, 0), 0.5)
	base:DrawRectangle(Vector2.zero, Vector2.new(2, 2), Color3.new(0, 0, 1), 0, Enum.ImageCombineType.Overwrite, Enum.ImageAlphaType.LockCanvasAlpha)
	r.lock_alpha = pixel(base, 0, 0)
	base:DrawRectangle(Vector2.new(2, 2), Vector2.new(2, 2), Color3.new(0, 1, 0), 0.2, Enum.ImageCombineType.Overwrite, Enum.ImageAlphaType.LockCanvasColor)
	r.lock_color = pixel(base, 3, 3)
	base:DrawRectangle(Vector2.new(0, 2), Vector2.new(1, 1), Color3.new(0, 1, 0), 0.5, Enum.ImageCombineType.BlendSourceOver, Enum.ImageAlphaType.LockCanvasAlpha)
	r.lock_alpha_blend = pixel(base, 0, 2)
	base:DrawRectangle(Vector2.new(1, 2), Vector2.new(1, 1), Color3.new(0, 1, 0), 0.5, Enum.ImageCombineType.BlendSourceOver, Enum.ImageAlphaType.LockCanvasColor)
	r.lock_color_blend = pixel(base, 1, 2)

	-- Combine types.
	for _, name in { "NormalMapBlend", "Darken", "Lighten", "Screen", "Overlay", "SoftLight", "HardLight", "Difference" } do
		local image = canvas(Vector2.new(2, 2), Color3.fromRGB(100, 150, 200), 0)
		r["combine_" .. name] = h.try(function()
			image:DrawRectangle(Vector2.zero, Vector2.new(1, 1), Color3.fromRGB(200, 100, 50), 0.25, Enum.ImageCombineType[name])
			return pixel(image, 0, 0)
		end)
	end
	local function normalBlend(dest, src, transparency, destTransparency)
		local image = canvas(Vector2.new(1, 1), Color3.fromRGB(dest[1], dest[2], dest[3]), destTransparency or 0)
		image:DrawRectangle(Vector2.zero, Vector2.one, Color3.fromRGB(src[1], src[2], src[3]), transparency, Enum.ImageCombineType.NormalMapBlend)
		return pixel(image, 0, 0)
	end
	r.nmb_flat_dest = normalBlend({ 128, 128, 255 }, { 200, 128, 230 }, 0)
	r.nmb_flat_src = normalBlend({ 180, 100, 240 }, { 128, 128, 255 }, 0)
	r.nmb_both = normalBlend({ 180, 100, 240 }, { 90, 160, 220 }, 0)
	r.nmb_half = normalBlend({ 180, 100, 240 }, { 90, 160, 220 }, 0.5)
	r.nmb_clear = normalBlend({ 180, 100, 240 }, { 90, 160, 220 }, 1)
	r.nmb_dest_alpha = normalBlend({ 180, 100, 240 }, { 90, 160, 220 }, 0, 0.5)
	r.nmb_tilted = normalBlend({ 60, 200, 200 }, { 220, 70, 180 }, 0)
	r.nmb_image = h.try(function()
		local dest = canvas(Vector2.new(1, 1), Color3.fromRGB(180, 100, 240), 0)
		dest:DrawImage(Vector2.zero, canvas(Vector2.new(1, 1), Color3.fromRGB(90, 160, 220), 0), Enum.ImageCombineType.NormalMapBlend)
		return pixel(dest, 0, 0)
	end)
	local blend = canvas(Vector2.new(2, 2), Color3.fromRGB(100, 150, 200), 0.5)
	local over = canvas(Vector2.new(2, 2), Color3.fromRGB(200, 100, 50), 0.25)
	for _, name in { "BlendSourceOver", "AlphaBlend", "Add", "Multiply", "Subtract" } do
		local image = AssetService:CreateEditableImageAsync(Content.fromObject(blend))
		image:DrawImage(Vector2.zero, over, Enum.ImageCombineType[name])
		r["image_" .. name] = pixel(image, 0, 0)
	end
	r.draw_image_default = h.try(function()
		local image = AssetService:CreateEditableImageAsync(Content.fromObject(blend))
		image:DrawImage(Vector2.zero, over)
		return pixel(image, 0, 0)
	end)
	r.draw_image_lock_alpha = h.try(function()
		local image = AssetService:CreateEditableImageAsync(Content.fromObject(blend))
		image:DrawImage(Vector2.zero, over, Enum.ImageCombineType.Overwrite, Enum.ImageAlphaType.LockCanvasAlpha)
		return pixel(image, 0, 0)
	end)
	r.rect_default = h.try(function()
		local image = AssetService:CreateEditableImageAsync(Content.fromObject(blend))
		image:DrawRectangle(Vector2.zero, Vector2.one, Color3.fromRGB(200, 100, 50), 0.25)
		return pixel(image, 0, 0)
	end)

	-- DrawImageTransformed: a 4x4 quadrant pattern scaled, rotated, cropped.
	local pattern = AssetService:CreateEditableImage({ Size = Vector2.new(4, 4) })
	pattern:DrawRectangle(Vector2.new(0, 0), Vector2.new(2, 2), Color3.new(1, 0, 0), 0, Enum.ImageCombineType.Overwrite)
	pattern:DrawRectangle(Vector2.new(2, 0), Vector2.new(2, 2), Color3.new(0, 1, 0), 0, Enum.ImageCombineType.Overwrite)
	pattern:DrawRectangle(Vector2.new(0, 2), Vector2.new(2, 2), Color3.new(0, 0, 1), 0, Enum.ImageCombineType.Overwrite)
	pattern:DrawRectangle(Vector2.new(2, 2), Vector2.new(2, 2), Color3.new(1, 1, 1), 0, Enum.ImageCombineType.Overwrite)
	local function transformed(position, scale, rotation, options)
		local target = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
		local ok, err = pcall(function()
			target:DrawImageTransformed(position, scale, rotation, pattern, options)
		end)
		if not ok then
			return nil, "err: " .. tostring(err):gsub("^.-:%d+: ", "")
		end
		return target
	end
	local t = transformed(Vector2.new(8, 8), Vector2.new(2, 2), 0, { SamplingMode = Enum.ResamplerMode.Pixelated })
	r.tx_scale_tl = pixel(t, 4, 4)
	r.tx_scale_tr = pixel(t, 11, 4)
	r.tx_scale_bl = pixel(t, 4, 11)
	r.tx_scale_br = pixel(t, 11, 11)
	r.tx_scale_out = pixel(t, 3, 3)
	r.tx_scale_sum = sum(t)
	t = transformed(Vector2.new(8, 8), Vector2.new(2, 2), 90, { SamplingMode = Enum.ResamplerMode.Pixelated })
	r.tx_rot90_tl = pixel(t, 5, 5)
	r.tx_rot90_tr = pixel(t, 10, 5)
	r.tx_rot90_bl = pixel(t, 5, 10)
	r.tx_rot90_sum = sum(t)
	t = transformed(Vector2.new(8, 8), Vector2.new(2, 2), 45, { SamplingMode = Enum.ResamplerMode.Pixelated })
	r.tx_rot45_top = pixel(t, 8, 4)
	r.tx_rot45_corner = pixel(t, 3, 3)
	r.tx_rot45_sum = sum(t)
	t = transformed(Vector2.new(8, 8), Vector2.new(2, 2), 0)
	r.tx_bilinear_seam = pixel(t, 7, 4)
	r.tx_bilinear_inner = pixel(t, 5, 5)
	r.tx_bilinear_sum = sum(t)
	t = transformed(Vector2.new(0, 0), Vector2.new(1, 1), 0, { PivotPoint = Vector2.new(0, 0), SamplingMode = Enum.ResamplerMode.Pixelated })
	r.tx_pivot_corner = pixel(t, 0, 0)
	r.tx_pivot_sum = sum(t)
	t = transformed(Vector2.new(15, 15), Vector2.new(1, 1), 0, { SamplingMode = Enum.ResamplerMode.Pixelated })
	r.tx_clipped_sum = sum(t)
	t = transformed(Vector2.new(8, 8), Vector2.new(-1, 1), 0, { SamplingMode = Enum.ResamplerMode.Pixelated })
	r.tx_mirror = t and pixel(t, 6, 6) or "nil"
	t = transformed(Vector2.new(8, 8), Vector2.new(1, 1), 0, { CombineType = Enum.ImageCombineType.Add, SamplingMode = Enum.ResamplerMode.Pixelated })
	r.tx_add = pixel(t, 7, 7)
	local _, err = transformed(Vector2.new(8, 8), Vector2.new(0, 1), 0)
	r.tx_zero_scale = err or "ok"
	_, err = transformed(Vector2.new(8, 8), Vector2.new(1, 1), 0, { Bogus = true })
	r.tx_bad_option = err or "ok"
	_, err = transformed(Vector2.new(8, 8), Vector2.new(1, 1), 0, { CombineType = "x" })
	r.tx_bad_combine = err or "ok"
	r.tx_self = h.try(function()
		pattern:DrawImageTransformed(Vector2.new(2, 2), Vector2.one, 0, pattern)
		return pixel(pattern, 0, 0)
	end)

	-- Projection onto a unit quad lying in XZ, UVs (0,0)-(1,1) across it.
	local quad = AssetService:CreateEditableMesh()
	local corners = { Vector3.new(-1, 0, -1), Vector3.new(-1, 0, 1), Vector3.new(1, 0, 1), Vector3.new(1, 0, -1) }
	local uvs = { Vector2.new(0, 0), Vector2.new(0, 1), Vector2.new(1, 1), Vector2.new(1, 0) }
	local vids, uvids = {}, {}
	for i = 1, 4 do
		vids[i] = quad:AddVertex(corners[i])
		uvids[i] = quad:AddUV(uvs[i])
	end
	local f1 = quad:AddTriangle(vids[1], vids[2], vids[3])
	local f2 = quad:AddTriangle(vids[1], vids[3], vids[4])
	quad:SetFaceUVs(f1, { uvids[1], uvids[2], uvids[3] })
	quad:SetFaceUVs(f2, { uvids[1], uvids[3], uvids[4] })
	local decal = canvas(Vector2.new(8, 8), Color3.new(1, 0, 0), 0)
	decal:DrawRectangle(Vector2.new(4, 0), Vector2.new(4, 8), Color3.new(0, 0, 1), 0, Enum.ImageCombineType.Overwrite)
	local projection = {
		Position = Vector3.new(0, 2, 0),
		Direction = Vector3.new(0, -1, 0),
		Up = Vector3.new(0, 0, -1),
		Size = Vector3.new(1, 1, 4),
	}
	local brush = {
		Decal = decal,
		ColorBlendType = Enum.ImageCombineType.Overwrite,
		AlphaBlendType = Enum.ImageAlphaType.Default,
		FadeAngle = 90,
		BlendIntensity = 1,
	}
	local target = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
	r.project = h.try(function()
		target:DrawImageProjected(quad, projection, brush)
	end)
	r.project_center = pixel(target, 8, 8)
	r.project_left = pixel(target, 6, 8)
	r.project_right = pixel(target, 9, 8)
	r.project_outside = pixel(target, 2, 2)
	r.project_edge = pixel(target, 4, 8)
	r.project_sum = sum(target)
	r.project_half = h.try(function()
		local half = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
		local b = table.clone(brush)
		b.BlendIntensity = 0.5
		b.ColorBlendType = Enum.ImageCombineType.AlphaBlend
		half:DrawImageProjected(quad, projection, b)
		return pixel(half, 6, 8)
	end)
	r.project_slanted = h.try(function()
		local slant = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
		local p = table.clone(projection)
		p.Direction = Vector3.new(1, -1, 0).Unit
		p.Position = Vector3.new(-2, 2, 0)
		local b = table.clone(brush)
		b.FadeAngle = 30
		slant:DrawImageProjected(quad, p, b)
		return sum(slant)
	end)
	local function slanted(fade, combine, intensity, base)
		local slant = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), base or 0)
		local p = table.clone(projection)
		p.Direction = Vector3.new(1, -1, 0).Unit
		p.Position = Vector3.new(-1, 1, 0)
		p.Up = Vector3.new(0, 0, -1)
		local b = table.clone(brush)
		b.FadeAngle = fade
		b.ColorBlendType = combine or b.ColorBlendType
		b.BlendIntensity = intensity or 1
		slant:DrawImageProjected(quad, p, b)
		return sum(slant) .. " " .. pixel(slant, 8, 8)
	end
	r.project_fade_90 = h.try(slanted, 90)
	r.project_fade_60 = h.try(slanted, 60)
	r.project_fade_50 = h.try(slanted, 50)
	r.project_fade_46 = h.try(slanted, 46)
	r.project_fade_44 = h.try(slanted, 44)
	r.project_fade_0 = h.try(slanted, 0)
	r.project_far = h.try(function()
		local c = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
		local p = table.clone(projection)
		p.Position = Vector3.new(0, 2.5, 0)
		c:DrawImageProjected(quad, p, brush)
		return sum(c)
	end)
	r.project_deep = h.try(function()
		local c = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
		local p = table.clone(projection)
		p.Position = Vector3.new(0, 2.5, 0)
		p.Size = Vector3.new(1, 1, 6)
		c:DrawImageProjected(quad, p, brush)
		return sum(c)
	end)
	r.project_wide = h.try(function()
		local c = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
		local p = table.clone(projection)
		p.Size = Vector3.new(2, 1, 4)
		c:DrawImageProjected(quad, p, brush)
		return sum(c) .. " " .. pixel(c, 1, 8) .. " " .. pixel(c, 8, 5)
	end)
	r.project_intensity_over = h.try(function()
		local c = canvas(Vector2.new(16, 16), Color3.new(0, 1, 0), 0.5)
		local b = table.clone(brush)
		b.ColorBlendType = Enum.ImageCombineType.BlendSourceOver
		b.BlendIntensity = 0.5
		c:DrawImageProjected(quad, projection, b)
		return pixel(c, 6, 8) .. " " .. pixel(c, 2, 2)
	end)
	r.project_intensity_overwrite = h.try(function()
		local c = canvas(Vector2.new(16, 16), Color3.new(0, 1, 0), 0.5)
		local b = table.clone(brush)
		b.BlendIntensity = 0.25
		c:DrawImageProjected(quad, projection, b)
		return pixel(c, 6, 8)
	end)
	r.project_translucent_decal = h.try(function()
		local c = canvas(Vector2.new(16, 16), Color3.new(0, 1, 0), 0)
		local b = table.clone(brush)
		b.Decal = canvas(Vector2.new(4, 4), Color3.new(1, 0, 0), 0.5)
		b.ColorBlendType = Enum.ImageCombineType.BlendSourceOver
		c:DrawImageProjected(quad, projection, b)
		return pixel(c, 6, 8)
	end)
	r.project_back = h.try(function()
		local back = canvas(Vector2.new(16, 16), Color3.new(0, 0, 0), 0)
		local p = table.clone(projection)
		p.Position = Vector3.new(0, -2, 0)
		p.Direction = Vector3.new(0, 1, 0)
		back:DrawImageProjected(quad, p, brush)
		return sum(back)
	end)
	r.project_no_decal = h.try(function()
		local b = table.clone(brush)
		b.Decal = nil
		target:DrawImageProjected(quad, projection, b)
	end)
	r.project_bad_source = h.try(function()
		target:DrawImageProjected(decal, projection, brush)
	end)
	r.project_missing_key = h.try(function()
		target:DrawImageProjected(quad, { Position = Vector3.zero }, brush)
	end)
	r.sample = h.try(function()
		local into = canvas(Vector2.new(8, 8), Color3.new(0, 0, 0), 0)
		local texture = canvas(Vector2.new(16, 16), Color3.new(0, 1, 0), 0)
		texture:DrawRectangle(Vector2.new(0, 0), Vector2.new(8, 16), Color3.new(1, 1, 0), 0, Enum.ImageCombineType.Overwrite)
		into:SampleImageProjected(quad, texture, projection, {
			ColorBlendType = Enum.ImageCombineType.Overwrite,
			AlphaBlendType = Enum.ImageAlphaType.Default,
			FadeAngle = 180,
		})
		return pixel(into, 1, 4) .. " " .. pixel(into, 6, 4) .. " " .. sum(into)
	end)
	return r
end
