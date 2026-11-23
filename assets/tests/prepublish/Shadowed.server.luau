-- Pre-publish fixture: globals the script declares itself aren't Roblox's.
local wait = task.wait
wait(1)

local function spawn(f)
	task.spawn(f)
end
spawn(print)

local game = { Workspace = {} }
local w = game.Workspace
