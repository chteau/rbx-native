--!strict
--! The offline catch-up summary, mirrored from
--! crates/rbx_presence/src/catchup.rs: a value-less journal of property
--! edits, aggregated per author and in total, shown as "While you were
--! away" when a collaborator reconnects. Pure (no HttpService), so it runs
--! in the shared Luau golden tests and is pinned by the same
--! fixtures/catchup.json the Rust side uses.
--!
--! Neither transport carries edit *values*, and neither records instance
--! add/remove: a worker journal returns the entries directly, while ntfy's
--! retained messages are the peers' states, whose edits are lifted out here.

local Schema = require(script.Parent.Schema)

local Catchup = {}

export type JournalEntry = {
	userId: number,
	name: string,
	path: string,
	property: string,
	at: number,
}

export type AuthorChange = {
	userId: number,
	name: string,
	instances: number,
	properties: number,
	edits: number,
}

export type Summary = {
	authors: { AuthorChange },
	instances: number,
	properties: number,
	edits: number,
}

-- Aggregates `entries`. Authors are ordered by edits descending, then name
-- case-insensitively, then user id; an author's name is the one on their
-- newest entry. `instances` counts distinct paths, `properties` distinct
-- (path, property) pairs, `edits` the entry count.
function Catchup.summarize(entries: { JournalEntry }): Summary
	local paths: { [string]: boolean } = {}
	local props: { [string]: boolean } = {}
	local pathCount, propCount = 0, 0
	type Tally = {
		name: string,
		latest: number,
		paths: { [string]: boolean },
		pathN: number,
		props: { [string]: boolean },
		propN: number,
		edits: number,
	}
	local byUser: { [number]: Tally } = {}

	local total = 0
	for _, entry in entries do
		total += 1
		if not paths[entry.path] then
			paths[entry.path] = true
			pathCount += 1
		end
		local pair = entry.path .. "\0" .. entry.property
		if not props[pair] then
			props[pair] = true
			propCount += 1
		end
		local tally = byUser[entry.userId]
		if not tally then
			tally = { name = entry.name, latest = entry.at, paths = {}, pathN = 0, props = {}, propN = 0, edits = 0 }
			byUser[entry.userId] = tally
		end
		if tally.edits == 0 or entry.at >= tally.latest then
			tally.name = entry.name
			tally.latest = entry.at
		end
		if not tally.paths[entry.path] then
			tally.paths[entry.path] = true
			tally.pathN += 1
		end
		if not tally.props[pair] then
			tally.props[pair] = true
			tally.propN += 1
		end
		tally.edits += 1
	end

	local authors: { AuthorChange } = {}
	for userId, tally in byUser do
		table.insert(authors, {
			userId = userId,
			name = tally.name,
			instances = tally.pathN,
			properties = tally.propN,
			edits = tally.edits,
		})
	end
	table.sort(authors, function(a, b)
		if a.edits ~= b.edits then
			return a.edits > b.edits
		end
		local an, bn = string.lower(a.name), string.lower(b.name)
		if an ~= bn then
			return an < bn
		end
		return a.userId < b.userId
	end)

	return { authors = authors, instances = pathCount, properties = propCount, edits = total }
end

-- The value-less journal entries lifted out of a list of peer states (the
-- ntfy transport's catch-up: retained messages are the peers' states).
function Catchup.fromStates(states: { any }): { JournalEntry }
	local entries: { JournalEntry } = {}
	for _, state in states do
		for _, edit in state.edits or {} do
			table.insert(entries, {
				userId = state.userId,
				name = state.name,
				path = edit.path,
				property = edit.property,
				at = edit.at,
			})
		end
	end
	return entries
end

-- The journal entries a relay's sealed journal reply holds, opened; entries
-- that fail to authenticate are dropped.
function Catchup.fromSealed(encKey: string, room: string, entries: { any }): { JournalEntry }
	local opened: { JournalEntry } = {}
	for _, entry in entries do
		local plain = Schema.openJournal(encKey, room, entry)
		if plain ~= nil then
			table.insert(opened, plain)
		end
	end
	return opened
end

-- One author's line: "Ada made 3 edits to 2 properties across 1 instance",
-- with singulars, mirroring the rbx-native dock's wording.
function Catchup.authorLine(author: AuthorChange): string
	local function plural(n: number, one: string, many: string): string
		return if n == 1 then one else many
	end
	return string.format(
		"%s made %d %s to %d %s across %d %s",
		author.name,
		author.edits,
		plural(author.edits, "edit", "edits"),
		author.properties,
		plural(author.properties, "property", "properties"),
		author.instances,
		plural(author.instances, "instance", "instances")
	)
end

return Catchup
