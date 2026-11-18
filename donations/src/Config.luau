--!strict
-- The donation place's settings. The only part to fill in after publishing:
-- the developer product ids from Creator Dashboard, Monetization, Developer
-- Products. Prices come from Roblox at run time, so they're set there only.
return {
	-- Developer product ids, cheapest first. One button each.
	products = {} :: { number },
	-- How many donors the board lists.
	boardSize = 20,
	-- How often the server re-reads the board, in seconds.
	boardEvery = 60,
}
