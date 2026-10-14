-- Value codec: Roblox values <-> JSON-able tables, the table in rbx_play::value.
local Codec = {}

local idOf = function(_)
	return 0
end
local byId = function(_)
	return nil
end

-- idOfFn(instance) -> id (0 when unknown); byIdFn(id) -> instance?
function Codec.setIdResolver(idOfFn, byIdFn)
	idOf = idOfFn
	byId = byIdFn
end

local FACES = { "Top", "Bottom", "Left", "Right", "Back", "Front" }
local AXES = { "X", "Y", "Z" }

local function num(n)
	if n ~= n then
		return { t = "n", v = "nan" }
	elseif n == math.huge then
		return { t = "n", v = "inf" }
	elseif n == -math.huge then
		return { t = "n", v = "-inf" }
	end
	return n
end

local function unnum(v)
	if type(v) == "table" and v.t == "n" then
		if v.v == "nan" then
			return 0 / 0
		elseif v.v == "inf" then
			return math.huge
		else
			return -math.huge
		end
	end
	return v
end

local function nums(list)
	local out = table.create(#list)
	for i, n in list do
		out[i] = num(n)
	end
	return out
end

local function unnums(list)
	local out = table.create(#list)
	for i, n in list do
		out[i] = unnum(n)
	end
	return out
end

function Codec.encode(value)
	local kind = typeof(value)
	if value == nil then
		return nil
	elseif kind == "boolean" or kind == "string" then
		return value
	elseif kind == "number" then
		return num(value)
	elseif kind == "Vector2" then
		return { t = "V2", v = nums({ value.X, value.Y }) }
	elseif kind == "Vector3" then
		return { t = "V3", v = nums({ value.X, value.Y, value.Z }) }
	elseif kind == "Vector3int16" then
		return { t = "V3i", v = { value.X, value.Y, value.Z } }
	elseif kind == "CFrame" then
		return { t = "CF", v = nums({ value:GetComponents() }) }
	elseif kind == "Color3" then
		return { t = "C3", v = nums({ value.R, value.G, value.B }) }
	elseif kind == "BrickColor" then
		return { t = "BC", v = value.Number }
	elseif kind == "UDim" then
		return { t = "U", v = nums({ value.Scale, value.Offset }) }
	elseif kind == "UDim2" then
		return { t = "U2", v = nums({ value.X.Scale, value.X.Offset, value.Y.Scale, value.Y.Offset }) }
	elseif kind == "Rect" then
		return { t = "R", v = nums({ value.Min.X, value.Min.Y, value.Max.X, value.Max.Y }) }
	elseif kind == "NumberRange" then
		return { t = "NR", v = nums({ value.Min, value.Max }) }
	elseif kind == "NumberSequence" then
		local out = {}
		for i, k in value.Keypoints do
			out[i] = nums({ k.Time, k.Value, k.Envelope })
		end
		return { t = "NS", v = out }
	elseif kind == "ColorSequence" then
		local out = {}
		for i, k in value.Keypoints do
			out[i] = nums({ k.Time, k.Value.R, k.Value.G, k.Value.B })
		end
		return { t = "CS", v = out }
	elseif kind == "EnumItem" then
		return { t = "E", v = value.Value, n = value.Name }
	elseif kind == "Instance" then
		return { t = "I", v = idOf(value) or 0 }
	elseif kind == "Font" then
		return { t = "F", v = { value.Family, value.Weight.Value, value.Style.Name } }
	elseif kind == "PhysicalProperties" then
		return {
			t = "PP",
			v = nums({
				value.Density,
				value.Friction,
				value.Elasticity,
				value.FrictionWeight,
				value.ElasticityWeight,
			}),
		}
	elseif kind == "Faces" then
		local out = {}
		for _, f in FACES do
			if value[f] then
				table.insert(out, f)
			end
		end
		return { t = "Fa", v = out }
	elseif kind == "Axes" then
		local out = {}
		for _, a in AXES do
			if value[a] then
				table.insert(out, a)
			end
		end
		return { t = "Ax", v = out }
	elseif kind == "Ray" then
		local o, d = value.Origin, value.Direction
		return { t = "Ray", v = nums({ o.X, o.Y, o.Z, d.X, d.Y, d.Z }) }
	elseif kind == "Content" then
		return { t = "Ct", v = value.Uri or "" }
	end
	return { t = "?", v = tostring(value) }
end

local function decodeEnum(json, current)
	local enumType = nil
	if typeof(current) == "EnumItem" then
		enumType = current.EnumType
	end
	if enumType == nil then
		return nil, "no enum type to decode into"
	end
	local item = nil
	if json.v ~= nil then
		item = enumType:FromValue(json.v)
	end
	if item == nil and json.n ~= nil then
		local ok, found = pcall(function()
			return (enumType :: any)[json.n]
		end)
		if ok then
			item = found
		end
	end
	if item == nil then
		return nil, "unknown enum item"
	end
	return item
end

-- `current` is the property's present value; it picks the enum type.
-- Returns (value) or (nil, error).
function Codec.decode(json, current)
	if json == nil then
		return nil
	end
	local kind = type(json)
	if kind ~= "table" then
		return json
	end
	local t, v = json.t, json.v
	if t == "n" then
		return unnum(json)
	elseif t == "V2" then
		v = unnums(v)
		return Vector2.new(v[1], v[2])
	elseif t == "V3" then
		v = unnums(v)
		return Vector3.new(v[1], v[2], v[3])
	elseif t == "V3i" then
		return Vector3int16.new(v[1], v[2], v[3])
	elseif t == "CF" then
		v = unnums(v)
		return CFrame.new(v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11], v[12])
	elseif t == "C3" then
		v = unnums(v)
		return Color3.new(v[1], v[2], v[3])
	elseif t == "BC" then
		return BrickColor.new(v)
	elseif t == "U" then
		v = unnums(v)
		return UDim.new(v[1], v[2])
	elseif t == "U2" then
		v = unnums(v)
		return UDim2.new(v[1], v[2], v[3], v[4])
	elseif t == "R" then
		v = unnums(v)
		return Rect.new(v[1], v[2], v[3], v[4])
	elseif t == "NR" then
		v = unnums(v)
		return NumberRange.new(v[1], v[2])
	elseif t == "NS" then
		local keys = {}
		for i, k in v do
			k = unnums(k)
			keys[i] = NumberSequenceKeypoint.new(k[1], k[2], k[3] or 0)
		end
		return NumberSequence.new(keys)
	elseif t == "CS" then
		local keys = {}
		for i, k in v do
			k = unnums(k)
			keys[i] = ColorSequenceKeypoint.new(k[1], Color3.new(k[2], k[3], k[4]))
		end
		return ColorSequence.new(keys)
	elseif t == "E" then
		return decodeEnum(json, current)
	elseif t == "I" then
		if v == nil or v == 0 then
			return nil
		end
		local inst = byId(v)
		if inst == nil then
			return nil, "instance " .. tostring(v) .. " is gone"
		end
		return inst
	elseif t == "F" then
		local weight = Enum.FontWeight:FromValue(v[2]) or Enum.FontWeight.Regular
		local style = Enum.FontStyle[v[3] or "Normal"]
		return Font.new(v[1], weight, style)
	elseif t == "PP" then
		if v == nil then
			return nil
		end
		v = unnums(v)
		return PhysicalProperties.new(v[1], v[2], v[3], v[4], v[5])
	elseif t == "Fa" then
		local list = {}
		for _, f in v do
			table.insert(list, Enum.NormalId[f])
		end
		return Faces.new(table.unpack(list))
	elseif t == "Ax" then
		local list = {}
		for _, a in v do
			table.insert(list, Enum.Axis[a])
		end
		return Axes.new(table.unpack(list))
	elseif t == "Ray" then
		v = unnums(v)
		return Ray.new(Vector3.new(v[1], v[2], v[3]), Vector3.new(v[4], v[5], v[6]))
	elseif t == "Ct" then
		if v == nil or v == "" then
			return Content.none
		end
		return Content.fromUri(v)
	end
	return nil, "cannot decode value of type " .. tostring(t)
end

return Codec
