-- How big an UnreliableRemoteEvent payload gets through, each way of
-- filling it sized either side of the documented 1000 byte limit: a
-- string, an array of numbers, an array of Vector3s, a buffer of noise, a
-- buffer of zeros (compressible), a dictionary of short keys. Each `max_*`
-- is the largest that arrived (characters, entries or bytes); Local Run's
-- byte accounting (`Wire::size`, what the Remote inspector shows) has to
-- drop the same ones.
return function(h)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	local kinds = { "string", "numbers", "vector3s", "buffer", "zeros", "dict" }
	local seen = {}
	for _, kind in kinds do
		local remote = Instance.new("UnreliableRemoteEvent")
		remote.Name = "Payload_" .. kind
		remote.Parent = ReplicatedStorage
		seen[kind] = 0
		remote.OnServerEvent:Connect(function(_, value)
			local size = 0
			if type(value) == "buffer" then
				size = buffer.len(value)
			elseif kind == "dict" then
				for _ in value do
					size += 1
				end
			else
				size = #value
			end
			if size > seen[kind] then
				seen[kind] = size
			end
		end)
	end
	h.client("RemotePayload", "fire")
	for _ = 1, 30 do
		h.beat()
	end
	local result = {}
	for _, kind in kinds do
		ReplicatedStorage["Payload_" .. kind]:Destroy()
		result["max_" .. kind] = seen[kind]
	end
	return result
end
