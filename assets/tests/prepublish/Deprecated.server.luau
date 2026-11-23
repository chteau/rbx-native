-- Pre-publish fixture: deprecated API. Every flagged line carries an expect
-- marker naming its check id; the others must stay quiet.
local Players = game:GetService("Players")

wait(1) -- expect: deprecated-api
spawn(function() end) -- expect: deprecated-api
delay(1, function() end) -- expect: deprecated-api
task.wait(1)
task.spawn(function() end)

local part = Instance.new("Part", workspace) -- expect: deprecated-api
local other = Instance.new("Part")
other.Parent = workspace

Players.PlayerAdded:connect(function() end) -- expect: deprecated-api
Players.PlayerAdded:Connect(function() end)

local base = game.Workspace.Baseplate -- expect: deprecated-api
local same = workspace.Baseplate

local hit = workspace:FindPartOnRay(Ray.new(Vector3.zero, Vector3.yAxis)) -- expect: deprecated-api
local result = workspace:Raycast(Vector3.zero, Vector3.yAxis)
local copy = part:clone() -- expect: deprecated-api
local fine = part:Clone()

-- A method the place's own code defines isn't Roblox's deprecated one.
local Mover = {}
function Mover:move() end
Mover:move()
