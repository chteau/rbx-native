-- Ping answers the client that fired it; Add adds.
local remotes = game:GetService("ReplicatedStorage").Remotes

remotes.Ping.OnServerEvent:Connect(function(player, message)
	remotes.Ping:FireClient(player, `pong: {message}`)
end)

remotes.Add.OnServerInvoke = function(_, a, b)
	return a + b
end
