-- A dead player comes back after Players.RespawnTime, a reset does the
-- same, and a player who leaves is gone.
return function()
	local Players = game:GetService("Players")

	describe("respawning", function()
		local player

		beforeAll(function()
			player = LocalRun.addPlayer()
		end)

		it("gives a dead player a new character after RespawnTime", function()
			local old = player.Character
			old.Humanoid.Health = 0
			local died = time()
			local new = LocalRun.waitUntil(function()
				return player.Character ~= old and player.Character
			end)
			expect(time() - died).to.be.near(Players.RespawnTime, 0.1)
			expect(new.Humanoid.Health).to.equal(new.Humanoid.MaxHealth)
		end)

		it("respawns after the menu's Reset Character", function()
			local old = player.Character
			local died = false
			old.Humanoid.Died:Connect(function()
				died = true
			end)
			LocalRun.act(player, { reset = true })
			LocalRun.waitUntil(function()
				return player.Character ~= old and player.Character
			end)
			expect(died).to.equal(true)
		end)

		it("lets the player leave", function()
			LocalRun.removePlayer(player)
			expect(#Players:GetPlayers()).to.equal(0)
		end)
	end)
end
