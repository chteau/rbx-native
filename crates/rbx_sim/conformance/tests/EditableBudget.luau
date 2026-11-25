-- The client's editable memory budget: how many of each kind a player's
-- client makes before AssetService hands back nil. Needs a player.
return function(h)
	local result = {}
	for key, value in h.client("EditableBudget") do
		result[key] = value
	end
	return result
end
