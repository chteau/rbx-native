--!nonstrict
-- The Audio API members past what the simulation answers (rbx_sim's
-- audio_api.rs): emitters' and listeners' distance and angle attenuation
-- curves, {[distance or angle] = volume} tables kept on the instance as
-- text ("key=volume ...", by key) the renderer reads, and
-- AudioDeviceInput.IsReady, which game scripts can't read. Keys and
-- volumes out of range are clamped, as a real server takes them.
local host = ...

local M = {}

local LIMIT = { DistanceAttenuation = math.huge, AngleAttenuation = 180 }

local function getter(name)
	return function(self)
		local curve = {}
		for key, volume in string.gmatch(host.raw(host.id(self), name) or "", "(%S+)=(%S+)") do
			curve[tonumber(key)] = tonumber(volume)
		end
		return curve
	end
end

local function setter(class, name)
	return function(self, curve)
		local points = {}
		for key, volume in curve do
			if type(key) ~= "number" then
				error(`{class}:Set{name}: numeric key expected.`, 0)
			end
			if type(volume) ~= "number" then
				error(`{class}:Set{name}: numeric value expected.`, 0)
			end
			table.insert(points, { math.clamp(key, 0, LIMIT[name]), math.clamp(volume, 0, 1) })
		end
		table.sort(points, function(a, b)
			return a[1] < b[1]
		end)
		for i, point in points do
			points[i] = string.format("%.17g=%.17g", point[1], point[2])
		end
		host.set_raw(host.id(self), name, table.concat(points, " "))
	end
end

-- A recording lives as long as its recorder; rbx_sim plays it back by the
-- recorder's id.
local function temporaryContent(self)
	return Content.fromUri("rbxtemp://" .. host.id(self))
end

function M.member(class, _, key)
	if class == "AudioRecorder" then
		return if key == "GetTemporaryContent" then temporaryContent else nil, key == "GetTemporaryContent"
	end
	if class == "AudioDeviceInput" then
		if key == "IsReady" then
			return nil, false, "The current thread cannot read 'IsReady' (lacking capability RobloxScript)"
		end
		return nil, false
	end
	local verb, name = string.match(key, "^([GS]et)(%a+Attenuation)$")
	if not LIMIT[name] then
		return nil, false
	end
	return if verb == "Get" then getter(name) else setter(class, name), true
end

return M
