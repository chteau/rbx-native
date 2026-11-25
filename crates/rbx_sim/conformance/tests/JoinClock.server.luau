-- Runs from the server's start (JoinTimeline): times the first PlayerAdded
-- and marks the workspace 0.1 s in.
local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local join = { beats = 0 }
_G.confJoin = join
local counting = RunService.Heartbeat:Connect(function()
	join.beats += 1
end)
task.delay(0.1, function()
	workspace:SetAttribute("ConfEarlyMark", true)
end)
Players.PlayerAdded:Connect(function(player)
	if join.player_added then
		return
	end
	join.player_added = time()
	join.beats_before_join = join.beats
	counting:Disconnect()
	player.CharacterAdded:Connect(function()
		join.character_added = join.character_added or time()
	end)
end)
