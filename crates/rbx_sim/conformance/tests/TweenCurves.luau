-- TweenService:GetValue for every EasingStyle and EasingDirection: 21
-- alphas to four decimals (compared), 101 to six (traces for the plot),
-- what it does outside 0..1, and the styles it refuses.
return function(h)
	local TweenService = game:GetService("TweenService")
	local result = {}
	local unsupported = {}
	for _, style in Enum.EasingStyle:GetEnumItems() do
		local ok, message = pcall(function()
			return TweenService:GetValue(0.5, style, Enum.EasingDirection.In)
		end)
		if not ok then
			table.insert(unsupported, style.Name .. ": " .. (tostring(message):gsub("^.-:%d+: ", "")))
			continue
		end
		for _, direction in Enum.EasingDirection:GetEnumItems() do
			local coarse, fine = {}, {}
			for i = 0, 100 do
				local v = TweenService:GetValue(i / 100, style, direction)
				if i % 5 == 0 then
					table.insert(coarse, string.format("%.4f", v))
				end
				table.insert(fine, string.format("%.6f", v))
			end
			local name = style.Name .. "_" .. direction.Name
			result[name] = table.concat(coarse, " ")
			result["_" .. name] = table.concat(fine, " ")
		end
	end
	table.sort(unsupported)
	result.unsupported = table.concat(unsupported, " | ")
	local function outside(style, direction)
		return string.format(
			"%.4f %.4f",
			TweenService:GetValue(-0.5, style, direction),
			TweenService:GetValue(1.5, style, direction)
		)
	end
	result.outside_linear = outside(Enum.EasingStyle.Linear, Enum.EasingDirection.In)
	result.outside_back = outside(Enum.EasingStyle.Back, Enum.EasingDirection.Out)
	result.outside_elastic = outside(Enum.EasingStyle.Elastic, Enum.EasingDirection.InOut)
	local ok, message = pcall(function()
		return TweenService:GetValue("x", Enum.EasingStyle.Linear, Enum.EasingDirection.In)
	end)
	result.bad_alpha = ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	h.beat()
	return result
end
