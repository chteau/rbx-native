-- When a player arrives on a fresh server: how long the server ran before
-- PlayerAdded (the JoinClock script counts from its start), and whether the
-- client's first loaded frame sees an attribute the server set 0.1 s in.
return function(h)
	local join = _G.confJoin
	local result = h.client("JoinTimeline", "report")
	result.player_added_s = h.round(join.player_added, 2)
	result.server_beats_before_join = join.beats_before_join
	result.server_ran_before_join = join.player_added > 0.1
	if join.character_added then
		result.character_after_join_s = h.round(join.character_added - join.player_added, 2)
	end
	return result
end
