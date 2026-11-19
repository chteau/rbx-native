-- A LocalScript in ReplicatedFirst: what the client has when it starts.
_G.rf_loaded_at_start = game:IsLoaded()
_G.rf_baseplate_at_start = workspace:FindFirstChild("Baseplate") ~= nil
if not game:IsLoaded() then
	game.Loaded:Wait()
end
_G.rf_baseplate_after_loaded = workspace:FindFirstChild("Baseplate") ~= nil
