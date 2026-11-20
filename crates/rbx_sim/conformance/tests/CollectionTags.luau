-- CollectionService on a server: when GetInstanceAddedSignal and
-- GetInstanceRemovedSignal fire (tagging in and out of the DataModel,
-- parenting a tagged instance in, Destroy, Instance:AddTag, a clone),
-- in what order against the line after, what GetTagged and GetAllTags
-- list, TagAdded/TagRemoved, and the odd tags (empty, long, repeated).
return function(h)
	local CollectionService = game:GetService("CollectionService")
	local HttpService = game:GetService("HttpService")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local tag = "LocalRunM12_" .. HttpService:GenerateGUID(false):sub(1, 8)
	local log = {}
	local function note(what)
		table.insert(log, what)
	end
	local added = CollectionService:GetInstanceAddedSignal(tag):Connect(function(instance)
		note("added:" .. instance.Name)
	end)
	local removed = CollectionService:GetInstanceRemovedSignal(tag):Connect(function(instance)
		note("removed:" .. instance.Name)
	end)
	local tagAdded = CollectionService.TagAdded:Connect(function(name)
		if name == tag then
			note("TagAdded")
		end
	end)
	local tagRemoved = CollectionService.TagRemoved:Connect(function(name)
		if name == tag then
			note("TagRemoved")
		end
	end)
	result.same_signal = CollectionService:GetInstanceAddedSignal(tag) == CollectionService:GetInstanceAddedSignal(tag)

	-- Tagged outside the DataModel, then parented in and out.
	local loose = Instance.new("Folder")
	loose.Name = "Loose"
	CollectionService:AddTag(loose, tag)
	note("after_add_loose")
	h.beat()
	note("beat")
	result.tagged_loose = #CollectionService:GetTagged(tag)
	result.has_loose = CollectionService:HasTag(loose, tag)
	loose.Parent = workspace
	note("after_parent_in")
	h.beat()
	note("beat")
	result.tagged_parented = #CollectionService:GetTagged(tag)
	loose.Parent = nil
	note("after_parent_out")
	h.beat()
	note("beat")
	result.log_loose = table.concat(log, " ")
	table.clear(log)

	-- Tagged in the DataModel, repeated, removed, destroyed.
	local a = Instance.new("Part")
	a.Name = "A"
	a.Anchored = true
	a.Parent = workspace
	a:AddTag(tag)
	note("after_add")
	a:AddTag(tag)
	note("after_add_again")
	h.beat()
	note("beat")
	local b = Instance.new("Folder")
	b.Name = "B"
	b.Parent = workspace
	CollectionService:AddTag(b, tag)
	h.beat()
	local names = {}
	for _, instance in CollectionService:GetTagged(tag) do
		table.insert(names, instance.Name)
	end
	result.tagged_order = table.concat(names, " ")
	result.get_tags = (table.concat(CollectionService:GetTags(a), ","):gsub(tag, "TAG"))
	result.tags_property = err(function()
		return a.Tags
	end)
	CollectionService:RemoveTag(a, tag)
	note("after_remove")
	CollectionService:RemoveTag(a, tag)
	note("after_remove_again")
	h.beat()
	note("beat")
	b:Destroy()
	note("after_destroy")
	h.beat()
	note("beat")
	result.log_live = table.concat(log, " ")
	table.clear(log)

	-- A clone keeps its tags and is added once parented.
	local c = Instance.new("Folder")
	c.Name = "C"
	c:AddTag(tag)
	c:AddTag("Other_" .. tag)
	local copy = c:Clone()
	copy.Name = "Copy"
	result.clone_tags = #copy:GetTags()
	copy.Parent = workspace
	h.beat()
	-- A tagged descendant comes in with its parent.
	local holder = Instance.new("Model")
	holder.Name = "Holder"
	local inner = Instance.new("Folder")
	inner.Name = "Inner"
	inner:AddTag(tag)
	inner.Parent = holder
	holder.Parent = workspace
	h.beat()
	holder.Parent = nil
	h.beat()
	result.log_clone = table.concat(log, " ")
	table.clear(log)

	local all = CollectionService:GetAllTags()
	result.all_tags_has = table.find(all, tag) ~= nil
	result.all_tags_type = type(all)
	result.tag_empty = err(function()
		a:AddTag("")
	end)
	result.tag_long = err(function()
		a:AddTag(string.rep("t", 101))
	end)
	result.tag_number = err(function()
		CollectionService:AddTag(a, 5)
		return table.concat(a:GetTags(), ",")
	end)
	result.tag_nil = err(function()
		CollectionService:AddTag(a, nil)
	end)
	result.has_nil = err(function()
		return CollectionService:HasTag(nil, tag)
	end)
	result.tagged_bad = err(function()
		return #CollectionService:GetTagged(5)
	end)
	added:Disconnect()
	removed:Disconnect()
	tagAdded:Disconnect()
	tagRemoved:Disconnect()
	a:Destroy()
	copy:Destroy()
	return result
end
