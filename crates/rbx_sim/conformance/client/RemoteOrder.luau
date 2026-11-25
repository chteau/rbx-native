-- RemoteOrder on the client.
local s
return function(ch, step)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	if step == "setup" then
		s = { log = {}, values = {}, arrive = {}, beat = 0 }
		s.a = ReplicatedStorage:WaitForChild("OrderA")
		s.b = ReplicatedStorage:WaitForChild("OrderB")
		s.u = ReplicatedStorage:WaitForChild("OrderU")
		s.fn = ReplicatedStorage:WaitForChild("OrderF")
		s.value = ReplicatedStorage:WaitForChild("OrderValue")
		s.fn.OnClientInvoke = function(x)
			return x * 2
		end
		s.value.Changed:Connect(function(v)
			table.insert(s.values, v)
		end)
		game:GetService("RunService").Heartbeat:Connect(function()
			s.beat += 1
		end)
		local function record(tag)
			return function(x)
				if x == "pong" then
					s.pong = s.beat
					return
				end
				if typeof(x) == "Instance" then
					table.insert(s.log, tag .. "inst:" .. x.Name)
				elseif x == "v" then
					table.insert(s.log, tag .. "v=" .. s.value.Value)
				else
					table.insert(s.log, tag .. tostring(x))
				end
				table.insert(s.arrive, s.beat)
			end
		end
		s.a.OnClientEvent:Connect(record("A"))
		s.b.OnClientEvent:Connect(record("B"))
		s.u.OnClientEvent:Connect(record("U"))
		return true
	elseif step == "burst" then
		for i = 1, 3 do
			s.a:FireServer(i)
			s.b:FireServer(i)
			s.u:FireServer(i)
		end
		return true
	end
	for _ = 1, 300 do
		if #s.log >= 11 then
			break
		end
		ch.beat()
	end
	local r = {}
	r.s2c_order = table.concat(s.log, " ")
	r.s2c_values = table.concat(s.values, ",")
	local beats = {}
	for _, at in s.arrive do
		beats[at] = true
	end
	local distinct = 0
	for _ in beats do
		distinct += 1
	end
	r.s2c_arrival_beats = distinct

	local started, from = os.clock(), s.beat
	s.pong = nil
	s.a:FireServer("ping")
	while not s.pong do
		ch.beat()
	end
	r._event_rtt_beats = s.pong - from
	r._event_rtt_ms = math.floor((os.clock() - started) * 1000)
	started, from = os.clock(), s.beat
	r.invoke_server = s.fn:InvokeServer(21)
	r._invoke_server_beats = s.beat - from
	r._invoke_server_ms = math.floor((os.clock() - started) * 1000)
	return r
end
