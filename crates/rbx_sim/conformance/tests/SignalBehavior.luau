-- Whether handlers run inside Fire and inside a property write, or after
-- the code that caused them, under the place's Workspace.SignalBehavior.
return function(h)
	local order = {}
	local event = Instance.new("BindableEvent")
	event.Event:Connect(function()
		table.insert(order, "handler")
	end)
	event:Fire()
	table.insert(order, "after_fire")
	local value = Instance.new("IntValue")
	value.Changed:Connect(function()
		table.insert(order, "changed")
	end)
	value.Value = 1
	table.insert(order, "after_set")
	h.beat()
	table.insert(order, "beat")
	-- Which handler runs first: A and B connected before a waiting thread
	-- W, C after it.
	local seq = {}
	local second = Instance.new("BindableEvent")
	second.Event:Connect(function()
		table.insert(seq, "A")
	end)
	second.Event:Connect(function()
		table.insert(seq, "B")
	end)
	task.spawn(function()
		second.Event:Wait()
		table.insert(seq, "W")
	end)
	second.Event:Connect(function()
		table.insert(seq, "C")
	end)
	second:Fire()
	table.insert(seq, "after")
	h.beat()
	local ok, behavior = pcall(function()
		return workspace.SignalBehavior.Name
	end)
	return {
		order = table.concat(order, " "),
		connection_order = table.concat(seq, " "),
		behavior = ok and behavior or "unreadable",
	}
end
