-- The order remotes arrive in, both ways: two RemoteEvents and an
-- UnreliableRemoteEvent fired in turn in one frame, property writes
-- between events, an instance made the same frame; the round trips.
-- Fields starting `_` are timings that include the real network.
return function(h)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	local RunService = game:GetService("RunService")
	local player = h.player()
	local function make(class, name)
		local instance = Instance.new(class)
		instance.Name = name
		instance.Parent = ReplicatedStorage
		return instance
	end
	local a = make("RemoteEvent", "OrderA")
	local b = make("RemoteEvent", "OrderB")
	local u = make("UnreliableRemoteEvent", "OrderU")
	local fn = make("RemoteFunction", "OrderF")
	local value = make("IntValue", "OrderValue")
	local log = {}
	local function record(tag)
		return function(_, x)
			if x ~= "ping" then
				table.insert(log, tag .. tostring(x))
			end
		end
	end
	a.OnServerEvent:Connect(record("A"))
	b.OnServerEvent:Connect(record("B"))
	u.OnServerEvent:Connect(record("U"))
	a.OnServerEvent:Connect(function(from, x)
		if x == "ping" then
			a:FireClient(from, "pong")
		end
	end)
	fn.OnServerInvoke = function(_, x)
		return x * 2
	end
	local result = {}
	h.client("RemoteOrder", "setup")
	h.client("RemoteOrder", "burst")
	for _ = 1, 120 do
		if #log >= 9 then
			break
		end
		h.beat()
	end
	result.c2s_order = table.concat(log, " ")

	value.Value = 1
	for i = 1, 3 do
		a:FireClient(player, i)
		b:FireClient(player, i)
		u:FireClient(player, i)
	end
	value.Value = 2
	a:FireClient(player, "v")
	value.Value = 3
	value.Value = 4
	local made = Instance.new("Part")
	made.Name = "MadeThisFrame"
	made.Anchored = true
	made.Parent = workspace
	a:FireClient(player, made)
	for key, text in h.client("RemoteOrder", "collect") do
		result[key] = text
	end

	local beats = 0
	local counter = RunService.Heartbeat:Connect(function()
		beats += 1
	end)
	local started = os.clock()
	result.invoke_client = fn:InvokeClient(player, 21)
	counter:Disconnect()
	result._invoke_client_beats = beats
	result._invoke_client_ms = math.floor((os.clock() - started) * 1000)
	for _, instance in { a, b, u, fn, value, made } do
		instance:Destroy()
	end
	return result
end
