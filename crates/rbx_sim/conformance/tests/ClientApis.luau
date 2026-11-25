-- The client-side gaps of Milestone 13: ContextActionService's bound
-- action info and touch buttons (none on a desktop), GuiService's
-- selection and menu toggles, UserInputService's device flags and key
-- strings, StarterGui's newer core GUI types, TeleportService's arrival
-- calls for a player who didn't teleport. Needs a player.
return function(h)
	local result = {}
	for key, value in h.client("ClientApis") do
		result[key] = value
	end
	return result
end
