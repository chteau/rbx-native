-- CameraInput on the client: where the default camera is, relative to the
-- character's root and to its focus.
return function(ch, step)
	local camera = workspace.CurrentCamera
	local player = game:GetService("Players").LocalPlayer
	local root = player.Character and player.Character:FindFirstChild("HumanoidRootPart")
	ch.beats(2)
	local cf = camera.CFrame
	local focus = camera.Focus.Position
	local offset = cf.Position - focus
	local look = cf.LookVector
	local r = {
		distance = offset.Magnitude,
		pitch = math.deg(math.asin(look.Y)),
		yaw = math.deg(math.atan2(-look.X, -look.Z)),
		-- Sideways from the root: shift lock's offset.
		eye_side = root and (cf.Position - root.Position):Dot(cf.RightVector) or 0,
		focus_above_root = root and (focus.Y - root.Position.Y) or 0,
		mouse = game:GetService("UserInputService").MouseBehavior.Name,
	}
	if step == "pose" then
		r.x, r.y, r.z = cf.Position.X, cf.Position.Y, cf.Position.Z
		r.lx, r.ly, r.lz = look.X, look.Y, look.Z
	end
	return r
end
