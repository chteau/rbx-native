-- Co-editing's traffic with the relay: sends batches to the op log (sealed
-- chunks, as many per request as fit: Studio allows a plugin 500 HTTP
-- requests a minute) and reads the log back, keeping a copy of it
-- (CoeditKept) that a relay lacking the log is handed.

local CoeditCore = require(script.Parent.CoeditCore)
local Crypto = require(script.Parent.Crypto)
local Json = require(script.Parent.Json)
local Kept = require(script.Parent.CoeditKept)

local POST_TRIES = 5

local Sync = {}

local rng = Random.new()

-- Plugins have no CSPRNG; a v4 GUID mixed with a Random stream gives a nonce
-- that does not repeat.
local function nonceFor(): string
	local hex = string.gsub(game:GetService("HttpService"):GenerateGUID(false), "[^%x]", "")
	local bytes = table.create(12)
	for i = 1, 12 do
		local guidByte = tonumber(string.sub(hex, i * 2 - 1, i * 2), 16) or 0
		bytes[i] = string.char(bit32.bxor(guidByte, rng:NextInteger(0, 255)))
	end
	return table.concat(bytes)
end

-- Posts `body` to the op log (or its fill route), retrying with backoff.
local function post(self: any, body: string, fill: boolean): (boolean, string?)
	local relay = self.options.relay
	local lastError: string? = nil
	for attempt = 1, POST_TRIES do
		if not self.running then
			return false, nil
		end
		local okPost, ok, err = pcall(relay.postOps, relay, body, fill)
		if okPost and ok then
			return true, nil
		end
		lastError = if okPost then err else tostring(ok)
		task.wait(0.5 * 2 ^ (attempt - 1))
	end
	return false, lastError
end

function Sync.send(self: any, ops: { any }): boolean
	local options = self.options
	local okJson, batchJson = pcall(Json.encode, { a = self.actor, ops = ops })
	if not okJson then
		self:note("Co-edit: could not encode an edit batch")
		return false
	end
	self.batchCount += 1
	local id = CoeditCore.session() .. string.format("%x", self.batchCount)
	local aad = Crypto.opsAad(options.room, options.userId, "studio")
	local sealed: { string } = {}
	for _, chunk in CoeditCore.split(id, batchJson) do
		table.insert(sealed, Crypto.sealB64(options.encKey, nonceFor(), aad, Json.encode(chunk)))
	end
	for _, cts in Kept.groups(sealed, Kept.REQUEST_CT_BYTES, string.len) do
		local body = Json.encode({ v = 2, userId = options.userId, client = "studio", cts = cts })
		local sent, why = post(self, body, false)
		if not sent then
			if why ~= nil then
				self:note("Co-edit: could not send edits (" .. why .. ")")
			end
			return false
		end
	end
	return true
end

-- Hands the relay entries of the kept log it lacks.
local function handOver(self: any, entries: { any }): boolean
	for _, group in Kept.groups(entries, Kept.REQUEST_CT_BYTES, function(e)
		return #e.ct
	end) do
		local body = Json.encode({ v = 2, log = self.kept.log, entries = group })
		local sent, why = post(self, body, true)
		if not sent then
			if why ~= nil then
				self:note("Co-edit: could not hand the edit log to the new host (" .. why .. ")")
			end
			return false
		end
	end
	return true
end

function Sync.flush(self: any)
	local ops = self:collect()
	-- Unsent edits go out again with the next flush, to whichever host the
	-- room has by then.
	if #ops > 0 and not self:send(ops) then
		for _, op in ops do
			table.insert(self.resend, op)
		end
	end
end

function Sync.authorOf(_self: any, actor: any): string
	if type(actor) ~= "string" then
		return "a collaborator"
	end
	local userId, client = string.match(actor, "^(%d+):([^:]+)")
	if userId == nil then
		return actor
	end
	return "user " .. userId .. " (" .. client .. ")"
end

function Sync.poll(self: any)
	local options = self.options
	local okPoll, result, err = pcall(options.relay.pollOps, options.relay, self.since)
	if not okPoll or result == nil then
		if not okPoll or err ~= nil then
			self:note("Co-edit: could not read edits (" .. tostring(if okPoll then err else result) .. ")")
		end
		return
	end
	-- A relay may carry on the log we read, lack some or all of it (a new
	-- host: hand it over), or run another (read that from its start; what
	-- the old one decided, last writer per property, is in the place).
	local step, missing = self.kept:step(result)
	if step == "fill" then
		handOver(self, missing :: { any })
		return
	elseif step == "restart" then
		self.kept:restart(result.log)
		self.since = 0
		self.reassembler = CoeditCore.Reassembler.new()
		self.state.registers = CoeditCore.Registers.new()
		return
	end
	local log = if type(result.log) == "string" then result.log else ""
	local entries = type(result.entries) == "table" and result.entries or {}
	table.sort(entries, function(a, b)
		return (a.seq or 0) < (b.seq or 0)
	end)
	if type(result.floor) == "number" and result.floor > self.since + 1 and self.since > 0 then
		self:note("Co-edit: the relay dropped older edits; some changes may be missing")
	end
	for _, entry in entries do
		if type(entry) ~= "table" or type(entry.seq) ~= "number" or type(entry.ct) ~= "string" then
			continue
		end
		if entry.seq <= self.since then
			continue
		end
		self.since = entry.seq
		local client = tostring(entry.client)
		local userId = tonumber(entry.userId) or 0
		self.kept:record(log, { seq = entry.seq, userId = userId, client = client, ct = entry.ct })
		local plain = Crypto.openB64(options.encKey, Crypto.opsAad(options.room, userId, client), entry.ct)
		if plain == nil then
			continue
		end
		local okChunk, chunk = pcall(Json.decode, plain)
		if not okChunk or type(chunk) ~= "table" then
			continue
		end
		local text = self.reassembler:push(entry.seq, string.format("%.0f", userId) .. ":" .. client, chunk)
		if text == nil then
			continue
		end
		local okBatch, batch = pcall(Json.decode, text)
		if okBatch and type(batch) == "table" then
			local outcome = self.apply:batch(batch, batch.a == self.actor, self:authorOf(batch.a))
			for _, line in outcome.notes do
				self:note(line)
			end
			for _, op in outcome.resend do
				table.insert(self.resend, op)
			end
		end
	end
end

return Sync
