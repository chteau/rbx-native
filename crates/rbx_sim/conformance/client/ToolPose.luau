-- ToolPose on the client: the animation tracks playing on the local
-- character (by animation id) and the right shoulder's Transform angles in
-- degrees.
local Players = game:GetService("Players")

return function(ch)
	ch.beats(2)
	local character = Players.LocalPlayer.Character
	local humanoid = character:FindFirstChildOfClass("Humanoid")
	local animator = humanoid:FindFirstChildOfClass("Animator")
	local ids = {}
	for _, track in animator:GetPlayingAnimationTracks() do
		local id = track.Animation and string.match(track.Animation.AnimationId, "%d+$") or "?"
		table.insert(ids, id)
	end
	table.sort(ids)
	local shoulder = character:FindFirstChild("RightShoulder", true) or character:FindFirstChild("Right Shoulder", true)
	local angles = "none"
	if shoulder then
		local x, y, z = shoulder.Transform:ToEulerAnglesXYZ()
		local function r(a)
			return math.floor(math.deg(a) + 0.5)
		end
		angles = string.format("%d,%d,%d", r(x), r(y), r(z))
	end
	return { tracks = table.concat(ids, ","), shoulder = angles }
end
