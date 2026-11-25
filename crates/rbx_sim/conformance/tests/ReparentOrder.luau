-- The order of the events one Parent write fires, as a live server runs
-- them: the moved instance's AncestryChanged, Changed and Parent signal,
-- its descendant's AncestryChanged, the old and new parents' ChildRemoved
-- and ChildAdded and their DescendantRemoving/DescendantAdded; then the
-- same for a Destroy.
return function(h, origin)
	local result = {}
	local log = {}
	local connections = {}
	local function on(signal, text)
		table.insert(
			connections,
			signal:Connect(function()
				table.insert(log, text)
			end)
		)
	end
	local a = h.new("Folder", { Name = "A" })
	local b = h.new("Folder", { Name = "B" })
	local x = h.new("Folder", { Name = "X" }, a)
	local inner = h.new("Folder", { Name = "Inner" }, x)
	h.beats(1)
	on(x.AncestryChanged, "x.Ancestry")
	on(inner.AncestryChanged, "inner.Ancestry")
	on(x.Changed, "x.Changed")
	on(x:GetPropertyChangedSignal("Parent"), "x.Parent")
	on(a.ChildRemoved, "a.ChildRemoved")
	on(b.ChildAdded, "b.ChildAdded")
	on(a.DescendantRemoving, "a.DescendantRemoving")
	on(b.DescendantAdded, "b.DescendantAdded")
	x.Parent = b
	table.insert(log, "|")
	h.beats(1)
	result.moved = table.concat(log, " ")
	log = {}
	on(x.Destroying, "x.Destroying")
	on(inner.Destroying, "inner.Destroying")
	on(b.ChildRemoved, "b.ChildRemoved")
	on(b.DescendantRemoving, "b.DescendantRemoving")
	x:Destroy()
	table.insert(log, "|")
	h.beats(1)
	result.destroyed = table.concat(log, " ")
	for _, c in connections do
		c:Disconnect()
	end
	a:Destroy()
	b:Destroy()
	return result
end
