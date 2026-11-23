-- Two players, one coin: whoever walks into it gets it, and both clients
-- see the outcome.
return function()
	describe("coin pickup", function()
		local alice, bob

		beforeAll(function()
			alice = LocalRun.addPlayer()
			bob = LocalRun.addPlayer()
		end)

		it("starts both players on 0 coins", function()
			expect(alice.Name).to.equal("Player1")
			expect(bob.Name).to.equal("Player2")
			expect(alice.leaderstats.Coins.Value).to.equal(0)
			expect(bob.leaderstats.Coins.Value).to.equal(0)
		end)

		it("gives the coin to the player who walks into it", function()
			LocalRun.act(alice, { walkTo = workspace.Coins.CoinA.Position })
			LocalRun.waitUntil(function()
				return alice.leaderstats.Coins.Value == 1
			end)
			expect(bob.leaderstats.Coins.Value).to.equal(0)
			expect(workspace.Coins:FindFirstChild("CoinA")).never.to.be.ok()
		end)

		it("shows the outcome on both clients", function()
			local coins = LocalRun.client(alice):eval([[
				return game:GetService("Players").LocalPlayer.leaderstats.Coins.Value
			]])
			expect(coins).to.equal(1)
			local left = LocalRun.client(bob):eval([[
				return workspace.Coins:FindFirstChild("CoinA") == nil, workspace.Coins:FindFirstChild("CoinB") ~= nil
			]])
			expect(left).to.equal(true)
		end)

		it("lets a player jump", function()
			local root = bob.Character.HumanoidRootPart
			local ground = root.Position.Y
			LocalRun.act(bob, { jump = true })
			local peak = ground
			for _ = 1, 30 do
				task.wait()
				peak = math.max(peak, root.Position.Y)
			end
			expect(peak - ground > 2).to.equal(true)
		end)
	end)
end
