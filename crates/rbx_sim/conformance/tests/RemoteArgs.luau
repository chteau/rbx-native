-- What survives a remote, both ways: tables (mixed, holes, odd keys,
-- metatables), datatypes, instances only one side has, trailing nils; and
-- the errors each side gets for the other side's calls. Needs a player.
return function(h)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	local ServerStorage = game:GetService("ServerStorage")
	local shared = require(ReplicatedStorage.ConformanceClient.Shared)
	local player = h.player()
	local remote = Instance.new("RemoteEvent")
	remote.Name = "ArgsRemote"
	remote.Parent = ReplicatedStorage
	local hidden = Instance.new("Folder")
	hidden.Name = "Hidden"
	hidden.Parent = ServerStorage
	local own = Instance.new("Part")
	own.Name = "ServerOnly"
	local result = {}
	local got, count = {}, 0
	remote.OnServerEvent:Connect(function(from, name, ...)
		if type(name) ~= "string" then
			return
		end
		got[name] = (from == player and "" or "notplayer ") .. shared.describeArgs(...)
		count += 1
	end)
	local clientErrors = h.client("RemoteArgs", "send")
	for _ = 1, 300 do
		if count >= clientErrors.sent then
			break
		end
		h.beat()
	end
	clientErrors.sent = nil
	for name, text in got do
		result["c2s_" .. name] = text
	end
	for key, text in clientErrors do
		result["client_" .. key] = text
	end
	local sent, failed = shared.sendAll(function(...)
		remote:FireClient(player, ...)
	end, own, hidden)
	for name, message in failed do
		result["send_" .. name] = message
	end
	for name, text in h.client("RemoteArgs", "collect", sent) do
		result["s2c_" .. name] = text
	end

	result.err_fire_server = shared.err(function()
		remote:FireServer(1)
	end)
	result.err_fire_client_nil = shared.err(function()
		remote:FireClient(nil, 1)
	end)
	result.err_fire_client_part = shared.err(function()
		remote:FireClient(workspace, 1)
	end)
	local cyclic = {}
	cyclic.self = cyclic
	result.err_cycle = shared.err(function()
		remote:FireClient(player, cyclic)
	end)
	local fn = Instance.new("RemoteFunction")
	result.err_invoke_server = shared.errOrYield(function()
		fn:InvokeServer()
	end, h.beat)
	result.err_read_callback = shared.err(function()
		return fn.OnServerInvoke
	end)
	result.err_set_client_callback = shared.err(function()
		fn.OnClientInvoke = function() end
	end)
	fn:Destroy()
	remote:Destroy()
	hidden:Destroy()
	return result
end
