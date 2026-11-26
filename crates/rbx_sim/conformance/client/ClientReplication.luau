-- ClientReplication on the client: plays and stops a sound (writing its
-- volume too), and reads its own character's speed.
return function(ch, step, sound)
	if step == "play" then
		sound.Volume = 0.2
		sound:Play()
		ch.beats(2)
		return {}
	elseif step == "stop" then
		sound:Stop()
		ch.beats(2)
		return {}
	end
	local root = game:GetService("Players").LocalPlayer.Character.HumanoidRootPart
	return { speed = math.floor(root.AssemblyLinearVelocity.Magnitude * 10 + 0.5) / 10 }
end
