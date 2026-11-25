-- What Destroy does to connections, against a nil parent: those on the
-- destroyed instance and its descendants, Destroying's included, and one
-- made after the Destroy; a nil-parented instance's stay connected until
-- it is destroyed in turn. The Leak Finder dock leans on this.
return function(h)
	local r = {}
	local a = Instance.new("Folder")
	local child = Instance.new("Folder", a)
	local ca = a.ChildRemoved:Connect(function() end)
	local cc = child.Changed:Connect(function() end)
	local cd = a.Destroying:Connect(function() end)
	local kept = Instance.new("Folder", workspace)
	local ck = kept.Changed:Connect(function() end)
	kept.Parent = nil
	a:Destroy()
	r.destroyed = ca.Connected
	r.destroyed_child = cc.Connected
	r.destroying = cd.Connected
	r.nil_parent = ck.Connected
	h.beats(1)
	r.nil_parent_later = ck.Connected
	r.connect_after_destroy = a.Changed:Connect(function() end).Connected
	kept:Destroy()
	r.nil_parent_destroyed = ck.Connected
	return r
end
