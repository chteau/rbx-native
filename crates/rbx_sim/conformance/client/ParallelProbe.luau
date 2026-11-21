-- Under the suite's ConfActor in ServerScriptService: what Parallel Luau
-- lets a script inside an Actor do, left in the Actor's attributes for
-- the ParallelLuau test to read once "done" is set.
local RunService = game:GetService("RunService")
local actor = script:GetActor()
local seen = {}
local function try(f, ...)
	local ok, value = pcall(f, ...)
	if ok then
		return "ok"
	end
	return "err: " .. (tostring(value):gsub("^.-:%d+: ", ""))
end

seen.get_actor = actor ~= nil and actor.Name or "nil"
seen.desync = try(task.desynchronize)
-- Now in the parallel phase.
seen.read_parallel = try(function()
	return workspace.Gravity
end)
seen.write_parallel = try(function()
	workspace.Gravity = workspace.Gravity
end)
seen.attribute_parallel = try(function()
	actor:SetAttribute("probe", 1)
end)
local part
seen.new_parallel = try(function()
	part = Instance.new("Part")
end)
seen.parent_parallel = try(function()
	part.Parent = workspace
end)
seen.desync_again = try(task.desynchronize)
seen.wait_parallel = try(function()
	task.wait()
end)
seen.write_after_wait = try(function()
	workspace.Gravity = workspace.Gravity
end)
seen.desync_before_sync = try(task.desynchronize)
seen.sync = try(task.synchronize)
seen.write_serial = try(function()
	workspace.Gravity = workspace.Gravity
end)
seen.sync_again = try(task.synchronize)

local fired, parallelWrite = 0, nil
local connection = RunService.Heartbeat:ConnectParallel(function()
	fired += 1
	if parallelWrite == nil then
		parallelWrite = try(function()
			workspace.Gravity = workspace.Gravity
		end)
	end
end)
local bound, boundWrite = 0, nil
local binding = RunService:BindToSimulationParallel(function(dt)
	bound += 1
	if boundWrite == nil then
		boundWrite = try(function()
			workspace.Gravity = workspace.Gravity
		end)
	end
end)
seen.bind_type = typeof(binding)
for _ = 1, 30 do
	RunService.Heartbeat:Wait()
end
connection:Disconnect()
if typeof(binding) == "RBXScriptConnection" then
	binding:Disconnect()
end
seen.connect_parallel_fired = fired
seen.connect_parallel_write = parallelWrite
seen.bind_fired = bound
seen.bind_write = boundWrite
seen.registry = try(function()
	local registry = game:GetService("SharedTableRegistry")
	local st = registry:GetSharedTable("confActor")
	st.n = 7
	return st.n
end)
for key, value in seen do
	actor:SetAttribute(key, value)
end
actor:SetAttribute("done", true)
