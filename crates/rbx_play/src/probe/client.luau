local LogService = game:GetService("LogService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local remote = ReplicatedStorage:WaitForChild("__rbxnative_remote")
local t0 = os.clock()
local buffer = {}

local levels = {
	[Enum.MessageType.MessageOutput] = "output",
	[Enum.MessageType.MessageInfo] = "info",
	[Enum.MessageType.MessageWarning] = "warning",
	[Enum.MessageType.MessageError] = "error",
}

LogService.MessageOut:Connect(function(message, messageType)
	table.insert(buffer, {
		level = levels[messageType] or "output",
		message = message,
		time = os.clock() - t0,
	})
end)

while true do
	task.wait(0.2)
	if #buffer > 0 then
		local batch = buffer
		buffer = {}
		remote:FireServer(batch)
	end
end
