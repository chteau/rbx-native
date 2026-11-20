-- The default camera's zoom limits, driven by real wheel input: with
-- Player.CameraMaxZoomDistance 14 and CameraMinZoomDistance 6, where six
-- notches out and twelve notches in stop, then the same past the defaults
-- (0.5 to 400) from the start distance. Needs a player.
return function(h, origin)
	local player = h.player()
	if player.Character then
		player:LoadCharacter()
	end
	local character = player.Character or player.CharacterAdded:Wait()
	character:WaitForChild("HumanoidRootPart")
	h.beats(180)
	local result = {}
	local function gesture(text)
		print("CONFORMANCE_GESTURE " .. text)
		h.beats(60)
	end
	local function distance()
		return h.client("CameraInput", "measure").distance
	end
	result.start = distance()
	player.CameraMaxZoomDistance = 14
	player.CameraMinZoomDistance = 6
	h.beats(30)
	result.clamped_now = distance()
	gesture("wheel out 6")
	result.max_out = distance()
	gesture("wheel in 12")
	result.min_in = distance()
	player.CameraMaxZoomDistance = 400
	player.CameraMinZoomDistance = 0.5
	h.beats(30)
	result.released = distance()
	gesture("wheel out 3")
	result.out_3 = distance()
	gesture("wheel in 3")
	return result
end
