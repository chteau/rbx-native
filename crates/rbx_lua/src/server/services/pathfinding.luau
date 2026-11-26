--!nonstrict
-- PathfindingService in Local Run: CreatePath's agent parameters, a Path's
-- ComputeAsync/GetWaypoints/Status/CheckOcclusionAsync and its signals,
-- with the argument errors a live server gives (the Pathfinding golden).
-- The path itself comes from the host's navigation grid
-- (`server/pathfinding.rs`).
local host = ...

local M = {}

local paths = {} -- instance id -> path state

local function fail(message)
	error(message, 0)
end

local function number(value, default)
	if value == nil then
		return default
	end
	if type(value) ~= "number" then
		fail("Unable to cast " .. typeof(value) .. " to float")
	end
	return value
end

local function bool(value, default)
	if value == nil then
		return default
	end
	if type(value) ~= "boolean" then
		fail("Unable to cast " .. typeof(value) .. " to bool")
	end
	return value
end

local function vector(value)
	if typeof(value) ~= "Vector3" then
		fail("Unable to cast " .. (type(value) == "number" and "double" or typeof(value)) .. " to Vector3")
	end
	return value
end

local function newPath(params)
	if params ~= nil and type(params) ~= "table" then
		fail("Unable to cast " .. typeof(params) .. " to Dictionary")
	end
	params = params or {}
	local costs = {}
	if type(params.Costs) == "table" then
		for name, cost in params.Costs do
			costs[tostring(name)] = number(cost, 1)
		end
	end
	local state = {
		radius = number(params.AgentRadius, 2),
		height = number(params.AgentHeight, 5),
		canJump = bool(params.AgentCanJump, true),
		canClimb = bool(params.AgentCanClimb, false),
		spacing = number(params.WaypointSpacing, 4),
		costs = costs,
		status = "NoPath",
		waypoints = {},
	}
	local path, id = host.new("Path", "Path")
	paths[id] = state
	return path
end

local function compute(state, start, finish)
	start, finish = vector(start), vector(finish)
	task.wait()
	local found = host.find_path(
		start.X,
		start.Y,
		start.Z,
		finish.X,
		finish.Y,
		finish.Z,
		state.height,
		state.canJump,
		state.spacing,
		state.costs
	)
	state.waypoints = {}
	if not found then
		state.status = "NoPath"
		return
	end
	for _, w in found do
		table.insert(
			state.waypoints,
			PathWaypoint.new(Vector3.new(w.x, w.y, w.z), Enum.PathWaypointAction[w.action], w.label)
		)
	end
	state.status = "Success"
end

local PATH = {}

function PATH.ComputeAsync(self, start, finish)
	compute(paths[host.id(self)], start, finish)
end

function PATH.GetWaypoints(self)
	return table.clone(paths[host.id(self)].waypoints)
end

function PATH.CheckOcclusionAsync(self, from)
	local state = paths[host.id(self)]
	from = number(from, 1)
	task.wait()
	local points = {}
	for _, w in state.waypoints do
		local p = w.Position
		table.insert(points, p.X)
		table.insert(points, p.Y)
		table.insert(points, p.Z)
	end
	return host.occlusion(points, from, state.height)
end

local SERVICE = {}

function SERVICE.CreatePath(_, params)
	return newPath(params)
end

function SERVICE.FindPathAsync(_, start, finish)
	local path = newPath(nil)
	compute(paths[host.id(path)], start, finish)
	return path
end

function M.member(class, id, key)
	if class == "PathfindingService" then
		return SERVICE[key], SERVICE[key] ~= nil
	end
	local state = paths[id]
	if not state then
		return nil, false
	end
	if key == "Status" then
		return Enum.PathStatus[state.status], true
	elseif key == "Blocked" or key == "Unblocked" then
		return host.signal(id, key), true
	end
	return PATH[key], PATH[key] ~= nil
end

return M
