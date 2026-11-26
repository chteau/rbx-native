--!strict
-- Health: heals the character's humanoid while it is hurt and alive.
--
-- An original implementation of the regeneration the Humanoid docs
-- describe: 1% of MaxHealth every second, never above MaxHealth, and
-- nothing once the humanoid has died.

local humanoid = script.Parent:WaitForChild("Humanoid") :: Humanoid
local PER_SECOND = 0.01

while humanoid.Health > 0 do
	local waited = task.wait(1)
	local health, max = humanoid.Health, humanoid.MaxHealth
	if health <= 0 then
		break
	end
	if health < max then
		humanoid.Health = math.min(max, health + max * PER_SECOND * waited)
	end
end
