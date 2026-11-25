-- Runtime StyleSheets on a server: a sheet linked into a ScreenGui, rules
-- by class, tag, name and child selectors, priorities, tokens, derives,
-- GetStyled and the change signals.
return function(h)
	local StarterGui = game:GetService("StarterGui")
	local r = {}
	local function color(c)
		return string.format("%d,%d,%d", math.floor(c.R * 255 + 0.5), math.floor(c.G * 255 + 0.5), math.floor(c.B * 255 + 0.5))
	end
	local function styled(instance, prop)
		return h.try(function()
			local v = instance:GetStyled(prop)
			if typeof(v) == "Color3" then
				return color(v)
			end
			return v
		end)
	end

	local sheet = Instance.new("StyleSheet")
	sheet.Name = "ConfSheet"
	sheet:SetAttribute("Accent", Color3.fromRGB(0, 0, 255))
	sheet:SetAttribute("Gap", 7)
	local function rule(selector, priority, props, parent)
		local rl = Instance.new("StyleRule")
		rl.Selector = selector
		rl.Priority = priority or 0
		for k, v in props do
			rl:SetProperty(k, v)
		end
		rl.Parent = parent or sheet
		return rl
	end
	local base = rule("Frame", 0, { BackgroundColor3 = Color3.fromRGB(255, 0, 0), BackgroundTransparency = 0.25 })
	rule(".Big", 0, { Size = UDim2.fromOffset(200, 100) })
	rule("#Named", 0, { BorderSizePixel = 4 })
	rule("Frame.Big", 5, { BackgroundColor3 = Color3.fromRGB(0, 255, 0) })
	rule("ScreenGui > Frame", 0, { Rotation = 10 })
	rule("TextLabel", 0, { TextColor3 = "$Accent", TextSize = "$Gap" })
	local bad = rule("Frame >>> ", 0, {})
	r.selector_error = bad.SelectorError
	r.selector_ok = base.SelectorError
	r.rule_props = h.try(function()
		local keys = {}
		for k in base:GetProperties() do
			table.insert(keys, k)
		end
		table.sort(keys)
		return table.concat(keys, ",")
	end)
	r.rule_get = h.try(function()
		return color(base:GetProperty("BackgroundColor3"))
	end)
	r.rule_missing = h.try(function()
		return base:GetProperty("Nope")
	end)
	r.rules = #sheet:GetStyleRules()

	local gui = Instance.new("ScreenGui")
	gui.Name = "ConfStyled"
	local link = Instance.new("StyleLink")
	link.StyleSheet = sheet
	link.Parent = gui
	local plain = h.new("Frame", { Name = "Plain" }, gui)
	local big = h.new("Frame", { Name = "Named" }, gui)
	big:AddTag("Big")
	local inner = h.new("Frame", { Name = "Inner" }, plain)
	local label = h.new("TextLabel", { Name = "Label" }, plain)
	sheet.Parent = gui
	gui.Parent = StarterGui
	h.beats(3)
	r.plain_color = styled(plain, "BackgroundColor3")
	r.plain_prop = color(plain.BackgroundColor3)
	r.plain_transparency = styled(plain, "BackgroundTransparency")
	r.plain_rotation = styled(plain, "Rotation")
	r.inner_rotation = styled(inner, "Rotation")
	r.big_color = styled(big, "BackgroundColor3")
	r.big_size = styled(big, "Size")
	r.big_border = styled(big, "BorderSizePixel")
	r.label_color = styled(label, "TextColor3")
	r.label_size = styled(label, "TextSize")
	r.unstyled = styled(plain, "ZIndex")
	r.bad_prop = h.try(function()
		return plain:GetStyled("Nope")
	end)

	-- Changes: a rule's property, a token, a tag, and the signals.
	local changed, styledChanged = 0, 0
	plain:GetStyledPropertyChangedSignal("BackgroundColor3"):Connect(function()
		changed += 1
	end)
	plain.StyledPropertiesChanged:Connect(function()
		styledChanged += 1
	end)
	base:SetProperty("BackgroundColor3", Color3.fromRGB(10, 20, 30))
	h.beats(3)
	r.after_set = styled(plain, "BackgroundColor3")
	sheet:SetAttribute("Accent", Color3.fromRGB(1, 2, 3))
	h.beats(3)
	r.after_token = styled(label, "TextColor3")
	plain:AddTag("Big")
	h.beats(3)
	r.after_tag = styled(plain, "BackgroundColor3")
	r.changed = changed
	r.styled_changed = styledChanged
	plain.BackgroundColor3 = Color3.fromRGB(9, 9, 9)
	h.beats(2)
	r.set_over_style = styled(plain, "BackgroundColor3")
	r.set_over_prop = color(plain.BackgroundColor3)

	-- A derived sheet overriding one rule.
	local theme = Instance.new("StyleSheet")
	rule("TextLabel", 0, { TextColor3 = Color3.fromRGB(200, 100, 50) }, theme)
	local derive = Instance.new("StyleDerive")
	derive.StyleSheet = sheet
	derive.Parent = theme
	theme.Parent = gui
	link.StyleSheet = theme
	h.beats(3)
	r.derived_label = styled(label, "TextColor3")
	r.derived_frame = styled(inner, "BackgroundColor3")
	r.derives = h.try(function()
		return #theme:GetDerives()
	end)
	link.StyleSheet = nil
	h.beats(3)
	r.unlinked = styled(inner, "BackgroundColor3")
	gui:Destroy()
	return r
end
