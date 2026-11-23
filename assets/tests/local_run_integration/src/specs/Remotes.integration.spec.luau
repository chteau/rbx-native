-- Remotes from a client to the server and back, over the simulated link.
return function()
	describe("remotes", function()
		local player

		beforeAll(function()
			player = LocalRun.addPlayer()
		end)

		it("echoes a RemoteEvent to the client that fired it", function()
			local reply = LocalRun.client(player):eval(
				[[
					local ping = game:GetService("ReplicatedStorage").Remotes.Ping
					local got
					ping.OnClientEvent:Connect(function(message)
						got = message
					end)
					ping:FireServer(...)
					while not got do
						task.wait()
					end
					return got
				]],
				"hello"
			)
			expect(reply).to.equal("pong: hello")
		end)

		it("answers a RemoteFunction", function()
			local sum = LocalRun.client(player):eval(
				[[return game:GetService("ReplicatedStorage").Remotes.Add:InvokeServer(...)]],
				2,
				3
			)
			expect(sum).to.equal(5)
		end)

		it("takes the link's round trip once latency is added", function()
			LocalRun.setNetwork({ latency = 0.2 })
			local took = LocalRun.client(player):eval([[
				local started = time()
				game:GetService("ReplicatedStorage").Remotes.Add:InvokeServer(1, 1)
				return time() - started
			]])
			expect(took).to.be.near(0.4, 0.05)
			LocalRun.setNetwork("3g")
			expect(player:GetNetworkPing()).to.be.near(0.33, 0.001)
			LocalRun.setNetwork("none")
		end)
	end)
end
