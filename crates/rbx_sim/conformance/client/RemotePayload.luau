-- RemotePayload on the client: every size, three times over, a few a
-- frame so the server's ~500 a second limit never drops one.
return function(ch, step)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	local RunService = game:GetService("RunService")
	-- Bytes that don't compress (a 32-bit LCG's high bytes).
	local seed = 12345
	local function noise(n)
		local b = buffer.create(n)
		for i = 0, n - 1 do
			seed = (seed * 1103515245 + 12345) % 2147483648
			buffer.writeu8(b, i, seed // 65536 % 256)
		end
		return b
	end
	local sizes = {
		{ "string", 960, 1010, 1, function(n)
			return string.rep("x", n)
		end },
		{ "numbers", 40, 130, 1, function(n)
			return table.create(n, 1.5)
		end },
		{ "vector3s", 30, 90, 1, function(n)
			return table.create(n, Vector3.new(1, 2, 3))
		end },
		{ "buffer", 960, 1010, 1, noise },
		{ "zeros", 1000, 4000, 250, buffer.create },
		{ "dict", 60, 160, 1, function(n)
			local t = {}
			for i = 1, n do
				t[string.format("a%03d", i)] = true
			end
			return t
		end },
	}
	local fired = 0
	for _ = 1, 3 do
		for _, entry in sizes do
			local kind, from, to, by, make = table.unpack(entry)
			local event = ReplicatedStorage:WaitForChild("Payload_" .. kind)
			for n = from, to, by do
				event:FireServer(make(n))
				fired += 1
				if fired % 4 == 0 then
					RunService.Heartbeat:Wait()
				end
			end
		end
	end
	return true
end
