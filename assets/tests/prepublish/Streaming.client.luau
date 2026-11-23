-- Pre-publish fixture: client code under StreamingEnabled.
local door = workspace.Map.Door -- expect: streaming-index
local map = workspace.Map
local safe = workspace.Map:WaitForChild("Door")
workspace.Map.Door.Touched:Connect(function() end) -- expect: streaming-index
local lamp = workspace:WaitForChild("Lamp")
