-- `LocalRun`, what an integration spec gets beside TestEZ's names, on the
-- server. Rust passes `native`: `join(name?)`, `leave(name)`,
-- `act(name, input)`, `network(profile)`, `eval(name, source, ...) -> id`,
-- `poll(id)` (nothing yet, or `true, values...`, or `false, message`) and
-- `finish(ok, message?)`. Requests are carried out after the frame they
-- were made in, so every call here that waits for its effect yields.
local native = ...

local Players = game:GetService("Players")

local TIMEOUT = 10

local LocalRun = {}

local function waitFor(check, timeout, message)
	local started = time()
	while true do
		local value = check()
		if value then
			return value
		end
		if time() - started >= timeout then
			error(message, 3)
		end
		task.wait()
	end
end

local function nameOf(player, method)
	if typeof(player) == "Instance" and player:IsA("Player") then
		return player.Name
	end
	error(`LocalRun.{method} expects a Player, got {typeof(player)}`, 3)
end

-- Runs the game `seconds` of simulated time; returns the time it ran.
function LocalRun.runFor(seconds)
	return task.wait(seconds)
end

-- Steps the game until `check()` returns a truthy value, and returns it;
-- an error after `timeout` simulated seconds (10).
function LocalRun.waitUntil(check, timeout)
	timeout = timeout or TIMEOUT
	return waitFor(check, timeout, `LocalRun.waitUntil timed out after {timeout} s`)
end

local Client = {}
Client.__index = Client

-- Runs `source` on this player's client as a chunk of its own, `...`
-- being the arguments, and returns what it returned. It may yield; it
-- takes a frame at least. Values cross as remote arguments do.
function Client:eval(source, ...)
	if type(source) ~= "string" then
		error(`eval expects Luau source as a string, got {typeof(source)}`, 2)
	end
	local id = native.eval(self.name, source, ...)
	local reply
	waitFor(function()
		reply = table.pack(native.poll(id))
		return reply.n > 0
	end, TIMEOUT, `{self.name}'s eval did not return within {TIMEOUT} s`)
	if not reply[1] then
		error(`{self.name}: {reply[2]}`, 2)
	end
	return table.unpack(reply, 2, reply.n)
end

-- A handle on `player`'s client.
function LocalRun.client(player)
	return setmetatable({ name = nameOf(player, "client") }, Client)
end

-- A player joins (`Player1`, `Player2`... without a name); returns their
-- `Player` once their character is in on the server and their client.
function LocalRun.addPlayer(name)
	if name ~= nil and type(name) ~= "string" then
		error(`LocalRun.addPlayer expects a name, got {typeof(name)}`, 2)
	end
	local before = {}
	for _, player in Players:GetPlayers() do
		before[player] = true
	end
	native.join(name)
	local player = waitFor(function()
		for _, player in Players:GetPlayers() do
			if not before[player] and (name == nil or player.Name == name) then
				return player
			end
		end
		return nil
	end, TIMEOUT, `{name or "a new player"} did not join within {TIMEOUT} s`)
	if Players.CharacterAutoLoads then
		waitFor(function()
			return player.Character
		end, TIMEOUT, `{player.Name}'s character did not load within {TIMEOUT} s`)
		LocalRun.client(player):eval([[
			local player = game:GetService("Players").LocalPlayer
			while not player.Character do
				task.wait()
			end
		]])
	end
	return player
end

-- `player` leaves the game; returns once the server has let them go.
function LocalRun.removePlayer(player)
	native.leave(nameOf(player, "removePlayer"))
	waitFor(function()
		return player.Parent == nil
	end, TIMEOUT, `{player.Name} did not leave within {TIMEOUT} s`)
end

local WALK = [[
	local humanoid = game:GetService("Players").LocalPlayer.Character:FindFirstChildOfClass("Humanoid")
	humanoid:MoveTo(...)
	return humanoid.MoveToFinished:Wait()
]]

local CENTER = [[
	local path = ...
	local at = game:GetService("Players").LocalPlayer
	for part in string.gmatch(path, "[^%.]+") do
		at = at:WaitForChild(part, 5)
		if not at then
			error(`no {path} in the player`, 0)
		end
	end
	local center = at.AbsolutePosition + at.AbsoluteSize / 2 + game:GetService("GuiService"):GetGuiInset()
	return center.X, center.Y
]]

local function tap(name, input)
	input.down = true
	native.act(name, input)
	task.wait()
	input.down = false
	native.act(name, input)
	task.wait()
end

-- Does `action` as `player` on their client:
-- `{ keyDown = "W" }`, `{ keyUp = "W" }`, `{ key = "E" }` (a tap),
-- `{ jump = true }`, `{ walkTo = Vector3 }` (returns whether they got
-- there, `Humanoid.MoveToFinished`'s answer), `{ click = "PlayerGui.Shop.Buy" }`
-- (a left click on that GuiObject's middle), `{ chat = "hi" }`,
-- `{ reset = true }` (the menu's Reset Character).
function LocalRun.act(player, action)
	local name = nameOf(player, "act")
	if type(action) ~= "table" then
		error(`LocalRun.act expects an action table, got {typeof(action)}`, 2)
	end
	if action.keyDown then
		native.act(name, { key = action.keyDown, down = true })
	elseif action.keyUp then
		native.act(name, { key = action.keyUp, down = false })
	elseif action.key then
		tap(name, { key = action.key })
	elseif action.jump then
		tap(name, { key = "Space" })
	elseif action.walkTo then
		return LocalRun.client(player):eval(WALK, action.walkTo)
	elseif action.click then
		local x, y = LocalRun.client(player):eval(CENTER, action.click)
		native.act(name, { x = x, y = y })
		tap(name, { button = 1 })
	elseif action.chat then
		native.act(name, { chat = action.chat })
		task.wait()
	elseif action.reset then
		native.act(name, { reset = true })
		task.wait()
	else
		error("LocalRun.act: unknown action (keyDown, keyUp, key, jump, walkTo, click, chat or reset)", 2)
	end
	return nil
end

-- The link every client has to the server, from the next frame: a
-- profile name (`"none"`, `"fiber"`, `"broadband"`, `"wifi"`, `"4g"`,
-- `"3g"`, the Network Simulator's presets) or `{ latency =, jitter =,
-- loss = }` in seconds and 0 to 1, for both ways or as `{ down =, up = }`.
function LocalRun.setNetwork(profile)
	native.network(profile)
	task.wait()
end

-- Runs the harness, then tells Rust it returned.
local function main(harness, ...)
	local ok, err = pcall(harness, ...)
	native.finish(ok, if ok then nil else tostring(err))
end

return table.freeze(LocalRun), main
