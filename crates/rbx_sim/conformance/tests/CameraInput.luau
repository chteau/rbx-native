-- The default camera and controls, driven by real input: the recorder
-- turns each `CONFORMANCE_GESTURE` line into XTest input on the client's
-- window, rbx_sim into Session input. The distance and pitch a character
-- starts at, the focus height, a wheel notch's zoom, a pixel of right-drag's
-- turn, and how far before a wall the camera stops. Shift lock isn't in:
-- under the recorder's Xephyr the press takes only now and then, and the
-- locked mouse turns the camera on its own (it can't warp the pointer
-- back); where it took, the eye sat 1.75 studs right of the root with the
-- mouse at LockCenter (PLAN.md).
-- The character respawns first, which puts the camera back where a new
-- character's starts, and each gesture is undone after. Needs a player.
return function(h, origin)
	local player = h.player()
	if player.Character then
		player:LoadCharacter()
	end
	local character = player.Character or player.CharacterAdded:Wait()
	local root = character:WaitForChild("HumanoidRootPart")
	h.beats(180)
	local result = {}
	local function gesture(text, settle)
		print("CONFORMANCE_GESTURE " .. text)
		h.beats(settle or 60)
	end
	local function measure()
		return h.client("CameraInput", "measure")
	end
	local start = measure()
	result.start_distance = start.distance
	result.start_pitch = start.pitch
	result.focus_above_root = start.focus_above_root

	gesture("wheel in 1")
	result.zoom_in_1 = measure().distance
	gesture("wheel out 1")
	result.zoom_back_1 = measure().distance
	gesture("wheel out 1")
	result.zoom_out_1 = measure().distance
	gesture("wheel in 1")

	local before = measure()
	gesture("drag 100 0")
	local turned = measure()
	result.turn_per_100px = (turned.yaw - before.yaw + 540) % 360 - 180
	gesture("drag -100 0")

	-- A wall six studs behind the focus, square to the view.
	local pose = h.client("CameraInput", "pose")
	local look = Vector3.new(pose.lx, pose.ly, pose.lz)
	local camera = Vector3.new(pose.x, pose.y, pose.z)
	local focus = camera + look * pose.distance
	local center = focus - look * 6
	local wall = Instance.new("Part")
	wall.Name = "CameraWall"
	wall.Anchored = true
	wall.Size = Vector3.new(60, 60, 1)
	wall.CFrame = CFrame.lookAt(center, center + look)
	wall.Parent = workspace
	h.beats(30)
	result.wall_margin = 5.5 - measure().distance
	wall:Destroy()
	h.beats(120)
	return result
end
