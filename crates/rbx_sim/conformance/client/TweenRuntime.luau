-- TweenRuntime on the client: a part the server tweens, as replicated here
-- ("watch"), and a tween of the client's own GUI: where in the client's
-- frame it writes, and where it ends ("gui").
local RunService = game:GetService("RunService")
local TweenService = game:GetService("TweenService")

return function(ch, step, part, goalX)
	if step == "watch" then
		local startX = part.Position.X
		local between, steps, last = false, 0, startX
		for _ = 1, 120 do
			ch.beat()
			local x = part.Position.X
			if x ~= last then
				steps += 1
				last = x
			end
			if x > startX + 1 and x < goalX - 1 then
				between = true
			end
			if math.abs(x - goalX) < 1e-4 then
				break
			end
		end
		ch.beats(5)
		return { between = between, final = math.abs(part.Position.X - goalX) < 1e-4, steps = steps }
	end
	local screen = Instance.new("ScreenGui")
	screen.Name = "ConfTween"
	screen.IgnoreGuiInset = true
	local frame = Instance.new("Frame")
	frame.Size = UDim2.new(0, 50, 0, 50)
	frame.Position = UDim2.new(0, 0, 0, 0)
	frame.Parent = screen
	screen.Parent = game:GetService("Players").LocalPlayer.PlayerGui
	ch.beats(2)
	local marks, last = {}, frame.Position
	local watch = {}
	for name, letter in { PreRender = "R", PreSimulation = "P", PostSimulation = "Q", Heartbeat = "H" } do
		table.insert(
			watch,
			RunService[name]:Connect(function()
				local changed = frame.Position ~= last
				last = frame.Position
				table.insert(marks, letter .. (changed and "+" or "-"))
			end)
		)
	end
	ch.beat()
	marks = {}
	local tween = TweenService:Create(frame, TweenInfo.new(0.5, Enum.EasingStyle.Linear), { Position = UDim2.new(0, 200, 0, 0) })
	tween:Play()
	table.insert(marks, "play")
	ch.beats(3)
	for _, c in watch do
		c:Disconnect()
	end
	tween.Completed:Wait()
	ch.beats(2)
	local ending = string.format("%d,%d", math.floor(frame.AbsolutePosition.X + 0.5), frame.Position.X.Offset)
	screen:Destroy()
	return { phase = table.concat(marks, " "), ending = ending }
end
