--!strict
-- Roblox values <-> value.rs JSON (`{t = "<Type>", v = ...}`). A JSON null
-- `v` arrives in Luau as a missing key, so an absent `v` reads as null.

local CoeditValue = {}

type TargetJson = { id: string, p: string }

local function wrap(t: string, v: any): any
	return { t = t, v = v }
end

local function seqNums(ks: { any }, f: (any) -> { number }): { { number } }
	local out = {}
	for _, k in ks do
		table.insert(out, f(k))
	end
	return out
end

local function encodeInner(value: any, valueType: string?, targetOf: (Instance) -> TargetJson): any?
	local kind = typeof(value)
	if value == nil then
		if valueType == "Ref" or valueType == "Instance" then
			return { t = "Ref" }
		elseif valueType == "PhysicalProperties" then
			return wrap("PhysicalProperties", "Default")
		elseif valueType == "OptionalCFrame" then
			return { t = "OptionalCFrame" }
		end
		return nil
	elseif kind == "string" then
		return wrap("String", value)
	elseif kind == "boolean" then
		return wrap("Bool", value)
	elseif kind == "number" then
		if valueType == "float" or valueType == "Float32" then
			return wrap("Float32", value)
		elseif valueType == "int" or valueType == "Int32" then
			return wrap("Int32", math.floor(value))
		elseif valueType == "int64" or valueType == "Int64" then
			return wrap("Int64", math.floor(value))
		end
		return wrap("Float64", value)
	elseif kind == "EnumItem" then
		return wrap("Enum", value.Value)
	elseif kind == "BrickColor" then
		return wrap("BrickColor", value.Number)
	elseif kind == "Color3" then
		if valueType == "Color3uint8" then
			return wrap("Color3uint8", {
				math.clamp(math.floor(value.R * 255 + 0.5), 0, 255),
				math.clamp(math.floor(value.G * 255 + 0.5), 0, 255),
				math.clamp(math.floor(value.B * 255 + 0.5), 0, 255),
			})
		end
		return wrap("Color3", { value.R, value.G, value.B })
	elseif kind == "Vector2" then
		return wrap("Vector2", { value.X, value.Y })
	elseif kind == "Vector3" then
		return wrap("Vector3", { value.X, value.Y, value.Z })
	elseif kind == "Vector3int16" then
		return wrap("Vector3int16", { value.X, value.Y, value.Z })
	elseif kind == "Ray" then
		local o, d = value.Origin, value.Direction
		return wrap("Ray", { o.X, o.Y, o.Z, d.X, d.Y, d.Z })
	elseif kind == "Faces" then
		return wrap("Faces", { value.Front, value.Bottom, value.Left, value.Back, value.Top, value.Right })
	elseif kind == "Axes" then
		return wrap("Axes", { value.X, value.Y, value.Z })
	elseif kind == "CFrame" then
		return wrap(if valueType == "OptionalCFrame" then "OptionalCFrame" else "CFrame", { value:GetComponents() })
	elseif kind == "NumberSequence" then
		return wrap("NumberSequence", seqNums(value.Keypoints, function(k)
			return { k.Time, k.Value, k.Envelope }
		end))
	elseif kind == "ColorSequence" then
		return wrap("ColorSequence", seqNums(value.Keypoints, function(k)
			return { k.Time, k.Value.R, k.Value.G, k.Value.B, 0 }
		end))
	elseif kind == "NumberRange" then
		return wrap("NumberRange", { value.Min, value.Max })
	elseif kind == "Rect" then
		return wrap("Rect", { value.Min.X, value.Min.Y, value.Max.X, value.Max.Y })
	elseif kind == "PhysicalProperties" then
		return wrap("PhysicalProperties", {
			value.Density,
			value.Friction,
			value.Elasticity,
			value.FrictionWeight,
			value.ElasticityWeight,
		})
	elseif kind == "UDim" then
		return wrap("UDim", { value.Scale, value.Offset })
	elseif kind == "UDim2" then
		return wrap("UDim2", { value.X.Scale, value.X.Offset, value.Y.Scale, value.Y.Offset })
	elseif kind == "Font" then
		return wrap("Font", { family = value.Family, weight = value.Weight.Value, style = value.Style.Value })
	elseif kind == "Instance" then
		local t = targetOf(value)
		return wrap("Ref", { id = t.id, p = t.p })
	end
	return nil
end

function CoeditValue.encode(value: any, valueType: string?, targetOf: (Instance) -> TargetJson): any?
	local ok, result = pcall(encodeInner, value, valueType, targetOf)
	if ok then
		return result
	end
	return nil
end

local function enumByValue(enum: any, n: number): EnumItem?
	for _, item in enum:GetEnumItems() do
		if item.Value == n then
			return item
		end
	end
	return nil
end

local function decodeInner(json: any, current: any, valueType: string?, resolve: (TargetJson) -> Instance?): (boolean, any)
	if type(json) ~= "table" or type(json.t) ~= "string" then
		return false, nil
	end
	local t, v = json.t, json.v
	if t == "Ref" then
		if type(v) ~= "table" then
			return true, nil
		end
		return true, resolve({ id = tostring(v.id or ""), p = tostring(v.p or "") })
	elseif t == "OptionalCFrame" and v == nil then
		return true, nil
	elseif t == "String" then
		return type(v) == "string", if type(v) == "string" then v else nil
	elseif t == "Bool" then
		return type(v) == "boolean", if type(v) == "boolean" then v else nil
	elseif t == "Int32" or t == "Int64" or t == "Float32" or t == "Float64" then
		return type(v) == "number", if type(v) == "number" then v else nil
	elseif t == "Enum" then
		local enum: any = nil
		if typeof(current) == "EnumItem" then
			enum = current.EnumType
		elseif valueType and string.sub(valueType, 1, 5) == "Enum:" then
			enum = (Enum :: any)[string.sub(valueType, 6)]
		end
		if enum == nil or type(v) ~= "number" then
			return false, nil
		end
		local item = enumByValue(enum, v)
		return item ~= nil, item
	elseif t == "BrickColor" then
		return true, BrickColor.new(v)
	elseif t == "PhysicalProperties" then
		if v == "Default" then
			return true, nil
		end
		return true, PhysicalProperties.new(v[1], v[2], v[3], v[4], v[5])
	elseif t == "Font" then
		local weight = enumByValue(Enum.FontWeight, v.weight)
		local style = enumByValue(Enum.FontStyle, v.style)
		if weight == nil or style == nil then
			return false, nil
		end
		return true, Font.new(v.family, weight :: any, style :: any)
	end
	if type(v) ~= "table" then
		return false, nil
	end
	if t == "Color3" then
		return true, Color3.new(v[1], v[2], v[3])
	elseif t == "Color3uint8" then
		return true, Color3.fromRGB(v[1], v[2], v[3])
	elseif t == "Vector2" then
		return true, Vector2.new(v[1], v[2])
	elseif t == "Vector3" then
		return true, Vector3.new(v[1], v[2], v[3])
	elseif t == "Vector3int16" then
		return true, Vector3int16.new(v[1], v[2], v[3])
	elseif t == "Ray" then
		return true, Ray.new(Vector3.new(v[1], v[2], v[3]), Vector3.new(v[4], v[5], v[6]))
	elseif t == "Faces" then
		local ids = {}
		local names = { "Front", "Bottom", "Left", "Back", "Top", "Right" }
		for i, name in names do
			if v[i] == true then
				table.insert(ids, (Enum.NormalId :: any)[name])
			end
		end
		return true, Faces.new(table.unpack(ids))
	elseif t == "Axes" then
		local ids = {}
		local names = { "X", "Y", "Z" }
		for i, name in names do
			if v[i] == true then
				table.insert(ids, (Enum.Axis :: any)[name])
			end
		end
		return true, Axes.new(table.unpack(ids))
	elseif t == "CFrame" or t == "OptionalCFrame" then
		if #v ~= 12 then
			return false, nil
		end
		return true, CFrame.new(table.unpack(v, 1, 12))
	elseif t == "NumberSequence" then
		local ks = {}
		for _, k in v do
			table.insert(ks, NumberSequenceKeypoint.new(k[1], k[2], k[3] or 0))
		end
		return true, NumberSequence.new(ks)
	elseif t == "ColorSequence" then
		local ks = {}
		for _, k in v do
			table.insert(ks, ColorSequenceKeypoint.new(k[1], Color3.new(k[2], k[3], k[4])))
		end
		return true, ColorSequence.new(ks)
	elseif t == "NumberRange" then
		return true, NumberRange.new(v[1], v[2])
	elseif t == "Rect" then
		return true, Rect.new(v[1], v[2], v[3], v[4])
	elseif t == "UDim" then
		return true, UDim.new(v[1], v[2])
	elseif t == "UDim2" then
		return true, UDim2.new(v[1], v[2], v[3], v[4])
	end
	return false, nil
end

function CoeditValue.decode(json: any, current: any, valueType: string?, resolve: (TargetJson) -> Instance?): (boolean, any)
	local ok, good, result = pcall(decodeInner, json, current, valueType, resolve)
	if ok and good then
		return true, result
	end
	return false, nil
end

return CoeditValue
