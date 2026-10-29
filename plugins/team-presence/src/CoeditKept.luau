--!strict
-- What this peer keeps of the op log it reads, mirroring
-- crates/rbx_presence/src/coedit/keep.rs: entries exactly as logged, the
-- log's id, so a relay that lacks the log (a new host) is handed it and
-- everyone carries on at the same seqs. Pure Luau, run by the test suite.

-- Most ciphertext kept, the relay's own cap; the oldest goes first.
local KEPT_BYTES = 64 * 1024 * 1024
-- Most ciphertext per request when posting or filling.
local REQUEST_CT_BYTES = 512 * 1024

export type Entry = { seq: number, userId: number, client: string, ct: string }

local Kept = {}
Kept.__index = Kept
Kept.REQUEST_CT_BYTES = REQUEST_CT_BYTES

function Kept.new(cap: number?)
	return setmetatable({ log = "", entries = {} :: { [number]: Entry }, first = 1, last = 0, bytes = 0, cap = cap or KEPT_BYTES }, Kept)
end

-- The newest seq kept; 0 for none.
function Kept.head(self: any): number
	-- Kept as a window `first..last` over the table, so trimming the front
	-- never leaves holes for `#` to trip on.
	return if self.last >= self.first then self.entries[self.last].seq else 0
end

function Kept.restart(self: any, log: string)
	self.log = log
	self.entries = {}
	self.first = 1
	self.last = 0
	self.bytes = 0
end

-- Keeps an entry of log `log` (the reply's), in order.
function Kept.record(self: any, log: string, entry: Entry)
	if self.log ~= log then
		self:restart(log)
	end
	self.last += 1
	self.entries[self.last] = { seq = entry.seq, userId = entry.userId, client = entry.client, ct = entry.ct }
	self.bytes += #entry.ct
	while self.bytes > self.cap and self.first <= self.last do
		self.bytes -= #self.entries[self.first].ct
		self.entries[self.first] = nil :: any
		self.first += 1
	end
end

local function from(self: any, after: number): { Entry }
	local out = {}
	for i = self.first, self.last do
		local entry = self.entries[i]
		if entry.seq > after then
			table.insert(out, entry)
		end
	end
	return out
end

-- What to do with a reply: "read" its entries, "fill" the relay with the
-- returned entries it lacks, or "restart" on the other log it runs.
function Kept.step(self: any, reply: any): (string, { Entry }?)
	local log = if type(reply.log) == "string" then reply.log else ""
	local head = tonumber(reply.head) or 0
	if log == "" then
		if head == 0 and self:head() > 0 then
			return "fill", from(self, 0)
		end
		return "read", nil
	end
	if self.log == "" then
		return "read", nil
	end
	if log ~= self.log then
		return "restart", nil
	end
	local tail = from(self, head)
	if tail[1] ~= nil and tail[1].seq == head + 1 then
		return "fill", tail
	end
	return "read", nil
end

-- `items` cut into runs of at most `max` bytes each (by `size`), never
-- empty, so one request each.
function Kept.groups<T>(items: { T }, max: number, size: (T) -> number): { { T } }
	local out: { { T } } = {}
	local used = 0
	for _, item in items do
		local n = size(item)
		local last = out[#out]
		if last ~= nil and used + n <= max then
			table.insert(last, item)
		else
			table.insert(out, { item })
			used = 0
		end
		used += n
	end
	return out
end

return Kept
