-- Teams: Player.Team, TeamColor and Neutral kept in step, Team.PlayerAdded
-- and PlayerRemoved, GetPlayers, spawning on the team's SpawnLocation, a
-- team destroyed under its player. Needs a player.
return function(h, origin)
	local Teams = game:GetService("Teams")
	local player = h.player()
	local result = {}
	local log = {}
	local function state()
		return string.format(
			"%s,%s,%s",
			tostring(player.Neutral),
			player.Team and player.Team.Name or "nil",
			player.TeamColor.Name
		)
	end
	local function team(name, color, auto)
		local t = Instance.new("Team")
		t.Name = name
		t.TeamColor = BrickColor.new(color)
		t.AutoAssignable = auto
		t.PlayerAdded:Connect(function(p)
			table.insert(log, name .. "+" .. (p == player and "p" or p.Name))
		end)
		t.PlayerRemoved:Connect(function(p)
			table.insert(log, name .. "-" .. (p == player and "p" or p.Name))
		end)
		t.Parent = Teams
		return t
	end
	result.defaults = state()
	local red = team("ConfRed", "Bright red", true)
	local blue = team("ConfBlue", "Bright blue", false)
	h.beats(2)
	result.after_teams = state()
	player.Team = red
	table.insert(log, "set_team")
	result.set_team_now = state()
	h.beats(2)
	result.after_team = state()
	player.TeamColor = BrickColor.new("Bright blue")
	table.insert(log, "set_color")
	h.beats(2)
	result.after_color = state()
	result.get_players = #red:GetPlayers() .. "," .. #blue:GetPlayers()
	player.TeamColor = BrickColor.new("Lime green")
	h.beats(2)
	result.no_team_color = state()

	local floor = h.floor(origin)
	local function spawn(color, x)
		return h.part({
			Anchored = true,
			Size = Vector3.new(6, 1, 6),
			Position = origin + Vector3.new(x, 0.5, 0),
			Neutral = false,
			TeamColor = BrickColor.new(color),
		}, "SpawnLocation")
	end
	local redSpawn = spawn("Bright red", -20)
	local blueSpawn = spawn("Bright blue", 20)
	player.Team = blue
	h.beats(2)
	local onBlue, onRed = 0, 0
	for _ = 1, 3 do
		player:LoadCharacter()
		h.beats(5)
		local root = player.Character:WaitForChild("HumanoidRootPart")
		if (root.Position - blueSpawn.Position).Magnitude < 8 then
			onBlue += 1
		elseif (root.Position - redSpawn.Position).Magnitude < 8 then
			onRed += 1
		end
	end
	-- Roblox picks among the team's and the neutral spawns at random.
	result._spawned_on_blue = onBlue
	result.spawned_on_red = onRed

	player.Neutral = true
	table.insert(log, "neutral")
	h.beats(2)
	result.after_neutral = state()
	player.Team = red
	h.beats(2)
	red:Destroy()
	table.insert(log, "destroyed")
	h.beats(2)
	result.after_destroy = state()
	result.log = table.concat(log, " ")

	player.Neutral = true
	player.TeamColor = BrickColor.new("White")
	blue:Destroy()
	for _, p in { floor, redSpawn, blueSpawn } do
		p:Destroy()
	end
	player:LoadCharacter()
	h.beats(30)
	return result
end
