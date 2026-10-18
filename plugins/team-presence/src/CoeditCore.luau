--!strict
-- The pure parts of live co-editing, mirroring crates/rbx_presence/src/coedit
-- (chunk.rs, merge.rs and the engine's script-text rules). Nothing here touches
-- a Roblox service, so it runs in plain Luau and is pinned by
-- fixtures/coedit_vectors.json.

local Crypto = require(script.Parent.Crypto)
local Sha256 = require(script.Parent.Sha256)

local CHUNK_BYTES = 7000
local STALE_AFTER = 4096
local MAX_KNOWN_TEXTS = 64

local CoeditCore = {}

function CoeditCore.textHash(text: string): string
	return string.sub(Sha256.hex(text), 1, 16)
end

-- Three-way merge -------------------------------------------------------------

local function splitLines(text: string): {string}
	local lines: {string} = {}
	local pos = 1
	local len = #text
	while pos <= len do
		local nl = string.find(text, "\n", pos, true)
		if nl == nil then
			table.insert(lines, string.sub(text, pos))
			break
		end
		table.insert(lines, string.sub(text, pos, nl))
		pos = nl + 1
	end
	return lines
end

-- For each line of `a`, the index of the line of `b` it is matched with in a
-- longest common subsequence, or nil.
local function matchLines(a: {string}, b: {string}): {[number]: number}
	local map: {[number]: number} = {}
	local na, nb = #a, #b
	local lo = 0
	while lo < na and lo < nb and a[lo + 1] == b[lo + 1] do
		lo += 1
		map[lo] = lo
	end
	local ea, eb = na, nb
	while ea > lo and eb > lo and a[ea] == b[eb] do
		map[ea] = eb
		ea -= 1
		eb -= 1
	end
	local rows = ea - lo
	local cols = eb - lo
	if rows == 0 or cols == 0 then
		return map
	end
	local table2: {{number}} = {}
	for i = 0, rows do
		local row: {number} = table.create(cols + 1, 0)
		table2[i + 1] = row
	end
	for i = rows - 1, 0, -1 do
		local row, below = table2[i + 1], table2[i + 2]
		local ai = a[lo + i + 1]
		for j = cols - 1, 0, -1 do
			if ai == b[lo + j + 1] then
				row[j + 1] = below[j + 2] + 1
			else
				row[j + 1] = math.max(below[j + 1], row[j + 2])
			end
		end
	end
	local i, j = 0, 0
	while i < rows and j < cols do
		if a[lo + i + 1] == b[lo + j + 1] then
			map[lo + i + 1] = lo + j + 1
			i += 1
			j += 1
		elseif table2[i + 2][j + 1] >= table2[i + 1][j + 2] then
			i += 1
		else
			j += 1
		end
	end
	return map
end

local function sameLines(a: {string}, b: {string}): boolean
	if #a ~= #b then
		return false
	end
	for i = 1, #a do
		if a[i] ~= b[i] then
			return false
		end
	end
	return true
end

local function slice(lines: {string}, from: number, to: number): {string}
	local out: {string} = {}
	for i = from, to do
		table.insert(out, lines[i])
	end
	return out
end

local function emit(out: {string}, lines: {string})
	for _, line in lines do
		table.insert(out, line)
	end
end

local function emitTerminated(out: {string}, lines: {string})
	emit(out, lines)
	local last = lines[#lines]
	if last ~= nil and string.sub(last, -1) ~= "\n" then
		table.insert(out, "\n")
	end
end

function CoeditCore.merge3(base: string, ours: string, theirs: string): {text: string, conflicted: boolean}
	local b, o, t = splitLines(base), splitLines(ours), splitLines(theirs)
	local mapO, mapT = matchLines(b, o), matchLines(b, t)
	local out: {string} = {}
	local conflicted = false
	local prevB, prevO, prevT = 0, 0, 0

	local function region(nextB: number, nextO: number, nextT: number)
		local baseChunk = slice(b, prevB + 1, nextB - 1)
		local oursChunk = slice(o, prevO + 1, nextO - 1)
		local theirsChunk = slice(t, prevT + 1, nextT - 1)
		if sameLines(oursChunk, baseChunk) then
			emit(out, theirsChunk)
		elseif sameLines(theirsChunk, baseChunk) or sameLines(oursChunk, theirsChunk) then
			emit(out, oursChunk)
		else
			conflicted = true
			table.insert(out, "<<<<<<< ours\n")
			emitTerminated(out, oursChunk)
			table.insert(out, "=======\n")
			emitTerminated(out, theirsChunk)
			table.insert(out, ">>>>>>> theirs\n")
		end
	end

	for i = 1, #b do
		local oi, ti = mapO[i], mapT[i]
		if oi ~= nil and ti ~= nil then
			region(i, oi, ti)
			table.insert(out, o[oi])
			prevB, prevO, prevT = i, oi, ti
		end
	end
	region(#b + 1, #o + 1, #t + 1)
	return { text = table.concat(out), conflicted = conflicted }
end

-- Chunks ----------------------------------------------------------------------

export type Chunk = { b: string, i: number, n: number, d: string }

function CoeditCore.split(id: string, batchJson: string): {Chunk}
	local len = #batchJson
	if len == 0 then
		return { { b = id, i = 0, n = 1, d = "" } }
	end
	local n = math.ceil(len / CHUNK_BYTES)
	local chunks: {Chunk} = {}
	for i = 0, n - 1 do
		local part = string.sub(batchJson, i * CHUNK_BYTES + 1, (i + 1) * CHUNK_BYTES)
		chunks[i + 1] = { b = id, i = i, n = n, d = Crypto.b64encode(part) }
	end
	return chunks
end

local Reassembler = {}
Reassembler.__index = Reassembler

function Reassembler.new()
	return setmetatable({ pending = {} :: {[string]: any} }, Reassembler)
end

function Reassembler.push(self: any, seq: number, sender: string, chunk: Chunk): string?
	local pending = self.pending :: {[string]: any}
	for key, entry in pending do
		if seq - entry.first >= STALE_AFTER then
			pending[key] = nil
		end
	end
	local n, i = chunk.n, chunk.i
	if type(n) ~= "number" or type(i) ~= "number" or n % 1 ~= 0 or i % 1 ~= 0 then
		return nil
	end
	if n == 0 or n > STALE_AFTER or i < 0 or i >= n then
		return nil
	end
	local data = Crypto.b64decode(chunk.d)
	if data == nil then
		return nil
	end
	local key = sender .. "\0" .. tostring(chunk.b)
	local entry = pending[key]
	if entry == nil then
		entry = { first = seq, n = n, slots = {}, filled = 0 }
		pending[key] = entry
	end
	if entry.n ~= n then
		return nil
	end
	if entry.slots[i + 1] == nil then
		entry.filled += 1
	end
	entry.slots[i + 1] = data
	if entry.filled < n then
		return nil
	end
	pending[key] = nil
	local text = table.concat(entry.slots)
	if utf8.len(text) == nil then
		return nil
	end
	return text
end

CoeditCore.Reassembler = Reassembler

-- Clocks and registers --------------------------------------------------------

export type Clock = { any }

function CoeditCore.clockLess(a: Clock, b: Clock): boolean
	local ca, cb = a[1] :: number, b[1] :: number
	if ca ~= cb then
		return ca < cb
	end
	return (a[2] :: string) < (b[2] :: string)
end

local Registers = {}
Registers.__index = Registers

function Registers.new()
	return setmetatable({ clocks = {} :: {[string]: Clock} }, Registers)
end

function Registers.claim(self: any, key: string, clock: Clock): boolean
	local clocks = self.clocks :: {[string]: Clock}
	local stored = clocks[key]
	if stored ~= nil and not CoeditCore.clockLess(stored, clock) then
		return false
	end
	clocks[key] = clock
	return true
end

CoeditCore.Registers = Registers

-- Script text log -------------------------------------------------------------

local Log = {}
Log.__index = Log

function Log.new()
	return setmetatable({ texts = {} :: {[string]: string}, known = {} :: {[string]: {[string]: string}} }, Log)
end

local function remember(self: any, key: string, text: string): string
	local hash = CoeditCore.textHash(text)
	local known = self.known[key]
	if known == nil then
		known = { count = 0 }
		self.known[key] = known
	end
	if known[hash] == nil then
		if known.count >= MAX_KNOWN_TEXTS then
			known = { count = 0 }
			self.known[key] = known
		end
		known[hash] = text
		known.count += 1
	end
	return hash
end

local function logText(self: any, key: string, current: string): string
	local text = self.texts[key]
	if text == nil then
		text = current
		self.texts[key] = current
		remember(self, key, current)
	end
	return text
end

-- Hash of the log's text for `key` (seeded from `current` when nothing has
-- been logged yet): the `base` a new Src op names.
function Log.hash(self: any, key: string, current: string): string
	return remember(self, key, logText(self, key, current))
end

export type SrcResult = { accepted: boolean, write: string?, conflicted: boolean, resend: string? }

-- Applies a Src op in log order. `current` is the editor's text now.
function Log.src(self: any, key: string, current: string, op: { base: string, text: string }, own: boolean): SrcResult
	local old = logText(self, key, current)
	if op.base ~= CoeditCore.textHash(old) then
		if own then
			local baseText = self.known[key][op.base] or old
			local merged = CoeditCore.merge3(baseText, op.text, old)
			local resend: string? = nil
			if merged.text ~= old then
				resend = merged.text
			end
			return { accepted = false, write = nil, conflicted = merged.conflicted, resend = resend }
		end
		return { accepted = false, write = nil, conflicted = false, resend = nil }
	end
	self.texts[key] = op.text
	remember(self, key, op.text)
	if own or current == op.text then
		return { accepted = true, write = nil, conflicted = false, resend = nil }
	end
	if current == old then
		return { accepted = true, write = op.text, conflicted = false, resend = nil }
	end
	local merged = CoeditCore.merge3(old, current, op.text)
	local write: string? = nil
	if merged.text ~= current then
		write = merged.text
	end
	return { accepted = true, write = write, conflicted = merged.conflicted, resend = nil }
end

CoeditCore.Log = Log

function CoeditCore.session(): string
	local digits: {string} = {}
	for i = 1, 8 do
		digits[i] = string.format("%x", math.random(0, 15))
	end
	return table.concat(digits)
end

return CoeditCore
