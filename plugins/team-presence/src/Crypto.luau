--!strict
-- Payload encryption, mirroring crates/rbx_presence/src/crypt.rs:
-- HKDF-SHA256 key derivation and ChaCha20-Poly1305 (RFC 8439), using only
-- bit32 so it runs in Studio and in plain Luau alike. Both sides are pinned
-- by fixtures/crypto_vectors.json.

local Sha256 = require(script.Parent.Sha256)

local Crypto = {}

local NONCE_LEN = 12
local TAG_LEN = 16
local BLOCK = 64

local function hmacRaw(key: string, msg: string): string
	if #key > BLOCK then
		key = Sha256.raw(key)
	end
	key = key .. string.rep("\0", BLOCK - #key)
	local inner: {string} = table.create(BLOCK)
	local outer: {string} = table.create(BLOCK)
	for i = 1, BLOCK do
		local b = string.byte(key, i)
		inner[i] = string.char(bit32.bxor(b, 0x36))
		outer[i] = string.char(bit32.bxor(b, 0x5c))
	end
	return Sha256.raw(table.concat(outer) .. Sha256.raw(table.concat(inner) .. msg))
end

function Crypto.hex(s: string): string
	return (string.gsub(s, ".", function(c: string)
		return string.format("%02x", string.byte(c))
	end))
end

function Crypto.unhex(s: string): string?
	if #s % 2 ~= 0 or string.find(s, "[^0-9a-fA-F]") then
		return nil
	end
	return (string.gsub(s, "..", function(pair: string)
		return string.char(tonumber(pair, 16) :: number)
	end))
end

function Crypto.hkdf(ikm: string, salt: string, info: string, len: number): string
	if salt == "" then
		salt = string.rep("\0", 32)
	end
	local prk = hmacRaw(salt, ikm)
	local out: {string} = {}
	local prev = ""
	local have = 0
	local i = 1
	while have < len do
		prev = hmacRaw(prk, prev .. info .. string.char(i))
		table.insert(out, prev)
		have += #prev
		i += 1
	end
	return string.sub(table.concat(out), 1, len)
end

function Crypto.encKey(secret: string, placeId: number): string
	return Crypto.hkdf(secret, "", "rbx-presence/enc/v1/" .. string.format("%.0f", placeId), 32)
end

-- ChaCha20 -------------------------------------------------------------------

local function leWord(s: string, i: number): number
	local a, b, c, d = string.byte(s, i, i + 3)
	return a + b * 0x100 + c * 0x10000 + d * 0x1000000
end

local function chachaBlock(key: string, counter: number, nonce: string): {number}
	local s: {number} = {
		0x61707865, 0x3320646e, 0x79622d32, 0x6b206574,
		leWord(key, 1), leWord(key, 5), leWord(key, 9), leWord(key, 13),
		leWord(key, 17), leWord(key, 21), leWord(key, 25), leWord(key, 29),
		counter, leWord(nonce, 1), leWord(nonce, 5), leWord(nonce, 9),
	}
	local x0, x1, x2, x3, x4, x5, x6, x7 = s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8]
	local x8, x9, x10, x11, x12, x13, x14, x15 = s[9], s[10], s[11], s[12], s[13], s[14], s[15], s[16]
	local band, bxor, rot = bit32.band, bit32.bxor, bit32.lrotate
	local M = 0xffffffff
	for _ = 1, 10 do
		-- columns
		x0 = band(x0 + x4, M); x12 = rot(bxor(x12, x0), 16); x8 = band(x8 + x12, M); x4 = rot(bxor(x4, x8), 12)
		x0 = band(x0 + x4, M); x12 = rot(bxor(x12, x0), 8); x8 = band(x8 + x12, M); x4 = rot(bxor(x4, x8), 7)
		x1 = band(x1 + x5, M); x13 = rot(bxor(x13, x1), 16); x9 = band(x9 + x13, M); x5 = rot(bxor(x5, x9), 12)
		x1 = band(x1 + x5, M); x13 = rot(bxor(x13, x1), 8); x9 = band(x9 + x13, M); x5 = rot(bxor(x5, x9), 7)
		x2 = band(x2 + x6, M); x14 = rot(bxor(x14, x2), 16); x10 = band(x10 + x14, M); x6 = rot(bxor(x6, x10), 12)
		x2 = band(x2 + x6, M); x14 = rot(bxor(x14, x2), 8); x10 = band(x10 + x14, M); x6 = rot(bxor(x6, x10), 7)
		x3 = band(x3 + x7, M); x15 = rot(bxor(x15, x3), 16); x11 = band(x11 + x15, M); x7 = rot(bxor(x7, x11), 12)
		x3 = band(x3 + x7, M); x15 = rot(bxor(x15, x3), 8); x11 = band(x11 + x15, M); x7 = rot(bxor(x7, x11), 7)
		-- diagonals
		x0 = band(x0 + x5, M); x15 = rot(bxor(x15, x0), 16); x10 = band(x10 + x15, M); x5 = rot(bxor(x5, x10), 12)
		x0 = band(x0 + x5, M); x15 = rot(bxor(x15, x0), 8); x10 = band(x10 + x15, M); x5 = rot(bxor(x5, x10), 7)
		x1 = band(x1 + x6, M); x12 = rot(bxor(x12, x1), 16); x11 = band(x11 + x12, M); x6 = rot(bxor(x6, x11), 12)
		x1 = band(x1 + x6, M); x12 = rot(bxor(x12, x1), 8); x11 = band(x11 + x12, M); x6 = rot(bxor(x6, x11), 7)
		x2 = band(x2 + x7, M); x13 = rot(bxor(x13, x2), 16); x8 = band(x8 + x13, M); x7 = rot(bxor(x7, x8), 12)
		x2 = band(x2 + x7, M); x13 = rot(bxor(x13, x2), 8); x8 = band(x8 + x13, M); x7 = rot(bxor(x7, x8), 7)
		x3 = band(x3 + x4, M); x14 = rot(bxor(x14, x3), 16); x9 = band(x9 + x14, M); x4 = rot(bxor(x4, x9), 12)
		x3 = band(x3 + x4, M); x14 = rot(bxor(x14, x3), 8); x9 = band(x9 + x14, M); x4 = rot(bxor(x4, x9), 7)
	end
	local x = { x0, x1, x2, x3, x4, x5, x6, x7, x8, x9, x10, x11, x12, x13, x14, x15 }
	for i = 1, 16 do
		x[i] = band(x[i] + s[i], M)
	end
	return x
end

local function chachaXor(key: string, counter: number, nonce: string, data: string): string
	local out: {string} = {}
	local bytes: {number} = table.create(BLOCK, 0)
	local n = #data
	local pos = 1
	while pos <= n do
		local ks = chachaBlock(key, counter, nonce)
		local count = math.min(BLOCK, n - pos + 1)
		for j = 0, count - 1 do
			local w = ks[bit32.rshift(j, 2) + 1]
			bytes[j + 1] = bit32.bxor(string.byte(data, pos + j), bit32.extract(w, (j % 4) * 8, 8))
		end
		table.insert(out, string.char(table.unpack(bytes, 1, count)))
		pos += BLOCK
		counter += 1
	end
	return table.concat(out)
end

-- Poly1305 with ten 13-bit limbs (130 bits), so every product sum stays far
-- below 2^53 in a double.

local LIMB = 8192

local function limbsOf(bytes: {number}): {number}
	local limbs: {number} = table.create(10, 0)
	for i = 0, 9 do
		local off = i * 13
		local bi = bit32.rshift(off, 3) + 1
		local v = (bytes[bi] or 0) + (bytes[bi + 1] or 0) * 256 + (bytes[bi + 2] or 0) * 65536
		limbs[i + 1] = bit32.extract(v, off % 8, 13)
	end
	return limbs
end

local function poly1305(key: string, msg: string): string
	local rb = { string.byte(key, 1, 16) }
	rb[4] = bit32.band(rb[4], 15)
	rb[8] = bit32.band(rb[8], 15)
	rb[12] = bit32.band(rb[12], 15)
	rb[16] = bit32.band(rb[16], 15)
	rb[5] = bit32.band(rb[5], 252)
	rb[9] = bit32.band(rb[9], 252)
	rb[13] = bit32.band(rb[13], 252)
	local r = limbsOf(rb)
	local r5: {number} = table.create(10, 0)
	for i = 1, 10 do
		r5[i] = r[i] * 5
	end
	local h: {number} = table.create(10, 0)
	local d: {number} = table.create(10, 0)
	local n = #msg
	local pos = 1
	while pos <= n do
		local count = math.min(16, n - pos + 1)
		local blk = { string.byte(msg, pos, pos + count - 1) }
		blk[count + 1] = 1
		for k = count + 2, 17 do
			blk[k] = 0
		end
		local m = limbsOf(blk)
		for i = 1, 10 do
			h[i] += m[i]
		end
		for k = 0, 9 do
			local sum = 0
			for i = 0, k do
				sum += h[i + 1] * r[k - i + 1]
			end
			for i = k + 1, 9 do
				sum += h[i + 1] * r5[k - i + 11]
			end
			d[k + 1] = sum
		end
		local c = 0
		for i = 1, 10 do
			local v = d[i] + c
			c = math.floor(v / LIMB)
			h[i] = v - c * LIMB
		end
		h[1] += c * 5
		c = math.floor(h[1] / LIMB)
		h[1] -= c * LIMB
		h[2] += c
		pos += 16
	end
	-- Fully carry, then subtract p once if h >= p.
	for _ = 1, 2 do
		local c = 0
		for i = 1, 10 do
			local v = h[i] + c
			c = math.floor(v / LIMB)
			h[i] = v - c * LIMB
		end
		h[1] += c * 5
	end
	local g: {number} = table.create(10, 0)
	local c = 5
	for i = 1, 10 do
		local v = h[i] + c
		c = math.floor(v / LIMB)
		g[i] = v - c * LIMB
	end
	if c > 0 then
		h = g
	end
	local hb: {number} = {}
	local acc, bits = 0, 0
	for i = 1, 10 do
		acc += h[i] * 2 ^ bits
		bits += 13
		while bits >= 8 do
			table.insert(hb, acc % 256)
			acc = math.floor(acc / 256)
			bits -= 8
		end
	end
	local out: {number} = table.create(16, 0)
	local carry = 0
	for i = 1, 16 do
		local v = hb[i] + string.byte(key, 16 + i) + carry
		out[i] = v % 256
		carry = math.floor(v / 256)
	end
	return string.char(table.unpack(out))
end

local function pad16(len: number): string
	return string.rep("\0", (16 - len % 16) % 16)
end

local function le64(n: number): string
	local t: {number} = table.create(8, 0)
	for i = 1, 8 do
		t[i] = n % 256
		n = math.floor(n / 256)
	end
	return string.char(table.unpack(t))
end

local function tagFor(key: string, nonce: string, aad: string, ct: string): string
	local polyKey = string.sub(chachaXor(key, 0, nonce, string.rep("\0", 32)), 1, 32)
	return poly1305(polyKey, aad .. pad16(#aad) .. ct .. pad16(#ct) .. le64(#aad) .. le64(#ct))
end

function Crypto.seal(key: string, nonce: string, aad: string, plaintext: string): string
	local ct = chachaXor(key, 1, nonce, plaintext)
	return nonce .. ct .. tagFor(key, nonce, aad, ct)
end

function Crypto.open(key: string, aad: string, sealed: string): string?
	if #sealed < NONCE_LEN + TAG_LEN then
		return nil
	end
	local nonce = string.sub(sealed, 1, NONCE_LEN)
	local ct = string.sub(sealed, NONCE_LEN + 1, #sealed - TAG_LEN)
	local tag = string.sub(sealed, #sealed - TAG_LEN + 1)
	local expect = tagFor(key, nonce, aad, ct)
	local diff = 0
	for i = 1, TAG_LEN do
		diff = bit32.bor(diff, bit32.bxor(string.byte(tag, i), string.byte(expect, i)))
	end
	if diff ~= 0 then
		return nil
	end
	return chachaXor(key, 1, nonce, ct)
end

-- Base64 ---------------------------------------------------------------------

local ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local DECODE: {[number]: number} = {}
for i = 1, 64 do
	DECODE[string.byte(ALPHABET, i)] = i - 1
end

function Crypto.b64encode(s: string): string
	local out: {string} = {}
	for i = 1, #s, 3 do
		local a, b, c = string.byte(s, i, i + 2)
		local v = a * 65536 + (b or 0) * 256 + (c or 0)
		local q1 = bit32.extract(v, 18, 6) + 1
		local q2 = bit32.extract(v, 12, 6) + 1
		local q3 = bit32.extract(v, 6, 6) + 1
		local q4 = bit32.extract(v, 0, 6) + 1
		table.insert(
			out,
			string.sub(ALPHABET, q1, q1)
				.. string.sub(ALPHABET, q2, q2)
				.. (if b then string.sub(ALPHABET, q3, q3) else "=")
				.. (if c then string.sub(ALPHABET, q4, q4) else "=")
		)
	end
	return table.concat(out)
end

function Crypto.b64decode(s: string): string?
	local n = #s
	if n % 4 ~= 0 then
		return nil
	end
	local out: {string} = {}
	for i = 1, n, 4 do
		local c1, c2, c3, c4 = string.byte(s, i, i + 3)
		local last = i + 3 == n
		local v1, v2 = DECODE[c1], DECODE[c2]
		if v1 == nil or v2 == nil then
			return nil
		end
		local pad = 0
		local v3, v4
		if last and c4 == 61 then
			pad = 1
			v4 = 0
			if c3 == 61 then
				pad = 2
				v3 = 0
			else
				v3 = DECODE[c3]
			end
		else
			v3, v4 = DECODE[c3], DECODE[c4]
		end
		if v3 == nil or v4 == nil then
			return nil
		end
		-- Non-zero bits under the padding are rejected, as the Rust decoder does.
		if (pad == 2 and v2 % 16 ~= 0) or (pad == 1 and v3 % 4 ~= 0) then
			return nil
		end
		local v = v1 * 262144 + v2 * 4096 + v3 * 64 + v4
		local a, b, c = bit32.extract(v, 16, 8), bit32.extract(v, 8, 8), bit32.extract(v, 0, 8)
		if pad == 0 then
			table.insert(out, string.char(a, b, c))
		elseif pad == 1 then
			table.insert(out, string.char(a, b))
		else
			table.insert(out, string.char(a))
		end
	end
	return table.concat(out)
end

function Crypto.sealB64(key: string, nonce: string, aad: string, plaintext: string): string
	return Crypto.b64encode(Crypto.seal(key, nonce, aad, plaintext))
end

function Crypto.openB64(key: string, aad: string, b64: string): string?
	local bytes = Crypto.b64decode(b64)
	if bytes == nil then
		return nil
	end
	return Crypto.open(key, aad, bytes)
end

-- AADs and journal tag -------------------------------------------------------

local function int(n: number): string
	return string.format("%.0f", n)
end

function Crypto.stateAad(room: string, userId: number, client: string): string
	return "rbx-presence/state/v2\n" .. room .. "\n" .. int(userId) .. "\n" .. client
end

function Crypto.journalAad(room: string, userId: number, at: number): string
	return "rbx-presence/journal/v2\n" .. room .. "\n" .. int(userId) .. "\n" .. int(at)
end

function Crypto.opsAad(room: string, userId: number, client: string): string
	return "rbx-presence/ops/v2\n" .. room .. "\n" .. int(userId) .. "\n" .. client
end

function Crypto.journalTag(key: string, path: string, property: string, at: number): string
	return string.sub(Crypto.hex(hmacRaw(key, path .. "\n" .. property .. "\n" .. int(at))), 1, 16)
end

return Crypto
