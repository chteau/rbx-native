-- JoinTimeline on the client: what FirstProbe saw once the game loaded.
return function()
	return { client_sees_early_mark_when_loaded = _G.rf_early_mark_when_loaded == true }
end
