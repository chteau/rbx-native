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
	local ok, behavior = pcall(function()
		return workspace.SignalBehavior.Name
	end)
	return {
		order = table.concat(order, " "),
		behavior = ok and behavior or "unreadable",
	}
end
