--!strict
--! The catch-up journal as the diff window lists it: one change per
--! instance, its edited properties under it, each with who edited it and
--! when last. Pure (no Studio services), so it runs in the Luau golden tests.
--!
--! The journal never carries values, and records property edits only, so
--! every change here is an update; the window reads each property's current
--! value off the place itself.

local Paths = require(script.Parent.Paths)

local Changes = {}

export type PropertyEdit = {
	property: string,
	-- Distinct author names, most recent editor first.
	authors: { string },
	edits: number,
	at: number,
}

export type Change = {
	path: string,
	name: string,
	-- The parent's path, `game` for a service.
	parent: string,
	properties: { PropertyEdit },
	at: number,
}

-- Groups `entries` (Catchup.JournalEntry) by instance path. Changes are
-- ordered by their newest edit, newest first; properties by name.
function Changes.fromJournal(entries: { any }): { Change }
	local byPath: { [string]: Change } = {}
	local order: { Change } = {}
	type Tally = { edit: PropertyEdit, latest: { [string]: number } }
	local tallies: { [string]: { [string]: Tally } } = {}

	for _, entry in entries do
		local change = byPath[entry.path]
		if not change then
			local names = Paths.split(entry.path) or { entry.path }
			local name = names[#names] or entry.path
			local parentNames = table.clone(names)
			table.remove(parentNames)
			change = {
				path = entry.path,
				name = name,
				parent = if #parentNames == 0 then "game" else Paths.join(parentNames),
				properties = {},
				at = 0,
			}
			byPath[entry.path] = change
			tallies[entry.path] = {}
			table.insert(order, change)
		end
		local c = change :: Change
		c.at = math.max(c.at, entry.at)
		local tally = tallies[entry.path][entry.property]
		if not tally then
			tally = {
				edit = { property = entry.property, authors = {}, edits = 0, at = 0 },
				latest = {},
			}
			tallies[entry.path][entry.property] = tally
			table.insert(c.properties, tally.edit)
		end
		local t = tally :: Tally
		t.edit.edits += 1
		t.edit.at = math.max(t.edit.at, entry.at)
		t.latest[entry.name] = math.max(t.latest[entry.name] or 0, entry.at)
	end

	for path, byProperty in tallies do
		for _, tally in byProperty do
			local names: { string } = {}
			for name in tally.latest do
				table.insert(names, name)
			end
			table.sort(names, function(a, b)
				if tally.latest[a] ~= tally.latest[b] then
					return tally.latest[a] > tally.latest[b]
				end
				return a < b
			end)
			tally.edit.authors = names
		end
		table.sort(byPath[path].properties, function(a, b)
			return a.property < b.property
		end)
	end

	table.sort(order, function(a, b)
		if a.at ~= b.at then
			return a.at > b.at
		end
		return a.path < b.path
	end)
	return order
end

-- "just now", "4 min ago", "2 h ago", "3 d ago".
function Changes.ago(at: number, now: number): string
	local seconds = math.max(0, (now - at) // 1000)
	if seconds < 60 then
		return "just now"
	elseif seconds < 3600 then
		return `{seconds // 60} min ago`
	elseif seconds < 86400 then
		return `{seconds // 3600} h ago`
	end
	return `{seconds // 86400} d ago`
end

return Changes
