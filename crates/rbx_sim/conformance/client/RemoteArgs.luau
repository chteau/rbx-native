-- RemoteArgs on the client: sends every payload, then reports what the
-- server's arrived as.
local state
return function(ch, step, expected)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	local shared = ch.shared
	if step == "send" then
		local remote = ReplicatedStorage:WaitForChild("ArgsRemote")
		state = { seen = {}, count = 0 }
		remote.OnClientEvent:Connect(function(name, ...)
			if type(name) ~= "string" then
				return
			end
			state.seen[name] = shared.describeArgs(...)
			state.count += 1
		end)
		local own = Instance.new("Part")
		own.Name = "ClientOnly"
		own.Anchored = true
		own.Parent = workspace
		local sent, failed = shared.sendAll(function(...)
			remote:FireServer(...)
		end, own, nil)
		local player = game:GetService("Players").LocalPlayer
		local errors = { sent = sent }
		for name, message in failed do
			errors["send_" .. name] = message
		end
		errors.err_fire_client = shared.err(function()
			remote:FireClient(player, 1)
		end)
		errors.err_fire_all = shared.err(function()
			remote:FireAllClients(1)
		end)
		errors.err_on_server_event = shared.err(function()
			remote.OnServerEvent:Connect(function() end)
		end)
		local cyclic = {}
		cyclic.self = cyclic
		errors.err_cycle = shared.err(function()
			remote:FireServer(cyclic)
		end)
		local fn = Instance.new("RemoteFunction")
		errors.err_invoke_client = shared.errOrYield(function()
			fn:InvokeClient(player)
		end, ch.beat)
		errors.err_set_server_callback = shared.err(function()
			fn.OnServerInvoke = function() end
		end)
		fn:Destroy()
		own:Destroy()
		return errors
	end
	for _ = 1, 300 do
		if state.count >= expected then
			break
		end
		ch.beat()
	end
	return state.seen
end
