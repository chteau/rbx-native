-- Streaming probe, the server half: waits for the player, then moves
-- their focus around (RequestStreamAroundAsync, a teleport and back,
-- ReplicationFocus) and prints what the client had at each point.
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local HttpService = game:GetService("HttpService")

local call = ReplicatedStorage:WaitForChild("S_Call")
local ready = ReplicatedStorage:WaitForChild("S_Ready")

local function emit(section, value)
	print("STREAMING " .. section .. " " .. HttpService:JSONEncode(value))
end

local readyPlayers = {}
ready.OnServerEvent:Connect(function(p)
	readyPlayers[p] = true
end)

local player = Players:GetPlayers()[1] or Players.PlayerAdded:Wait()
local joined = workspace.DistributedGameTime
local function now()
	return workspace.DistributedGameTime - joined
end
workspace:WaitForChild("S_PerPlayer"):AddPersistentPlayer(player)

local marks = {}
local character = player.Character or player.CharacterAdded:Wait()
marks.character = now()
while not readyPlayers[player] do
	task.wait()
end
marks.ready = now()
task.wait(10)

local function snap(label)
	local s = call:InvokeClient(player, "snapshot")
	s.at -= joined
	emit(label, s)
end
snap("settled")

marks.request = now()
local t = os.clock()
local ok, err = pcall(player.RequestStreamAroundAsync, player, Vector3.new(-3000, 5, 0))
marks.requestTook = os.clock() - t
marks.requestError = not ok and tostring(err) or nil
snap("afterRequest")
task.wait(5)
snap("afterRequest5")

t = os.clock()
ok, err = pcall(player.RequestStreamAroundAsync, player, Vector3.new(0, 5, 6000), 0.05)
marks.requestTimeoutTook = os.clock() - t
marks.requestTimeoutError = not ok and tostring(err) or nil
task.wait(5)
snap("afterTimeoutRequest")

local root = character:WaitForChild("HumanoidRootPart")
root.Anchored = true
marks.teleport = now()
character:PivotTo(CFrame.new(6000, 8, 0))
task.wait(20)
snap("atTarget")

marks.back = now()
character:PivotTo(CFrame.new(0, 8, 0))
task.wait(10)
snap("back")
emit("identity", call:InvokeClient(player, "identity"))

marks.focus = now()
player.ReplicationFocus = workspace:WaitForChild("S_Focus")
task.wait(10)
snap("focused")
player.ReplicationFocus = nil

local count = 0
for _, d in workspace:GetDescendants() do
	if string.sub(d.Name, 1, 2) == "S_" then
		count += 1
	end
end
emit("server", { count = count })

local logged = call:InvokeClient(player, "log")
local entries = logged.log
for _, e in entries do
	e[1] -= joined
end
for i = 1, #entries, 40 do
	emit("log", table.move(entries, i, math.min(i + 39, #entries), 1, {}))
end
local first = logged.first
first.startAt -= joined
first.loadedAt -= joined
emit("first", first)
local result = logged.result
result.scriptStart -= joined
emit("result", result)
for _, w in logged.warnings do
	w[1] -= joined
end
emit("warnings", logged.warnings)
emit("marks", marks)
print("STREAMING_DONE")
