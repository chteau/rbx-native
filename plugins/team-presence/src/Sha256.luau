--!strict
-- SHA-256 (FIPS 180-4) over a string's bytes, using only bit32 so it runs in
-- Studio and in plain Luau alike.

local K: {number} = {
	0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
	0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
	0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
	0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
	0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
	0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
	0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
	0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
}

local Sha256 = {}

local function digestWords(msg: string): {number}
	local len = #msg
	local padLen = (55 - len) % 64
	local hi = math.floor(len / 0x20000000)
	local lo = (len * 8) % 0x100000000
	local tail = "\128"
		.. string.rep("\0", padLen)
		.. string.char(
			bit32.rshift(hi, 24) % 256, bit32.rshift(hi, 16) % 256, bit32.rshift(hi, 8) % 256, hi % 256,
			bit32.rshift(lo, 24), bit32.rshift(lo, 16) % 256, bit32.rshift(lo, 8) % 256, lo % 256
		)
	local data = msg .. tail

	local h: {number} = {
		0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
	}
	local w: {number} = table.create(64, 0)

	for base = 1, #data, 64 do
		for i = 1, 16 do
			local a, b, c, d = string.byte(data, base + (i - 1) * 4, base + (i - 1) * 4 + 3)
			w[i] = a * 0x1000000 + b * 0x10000 + c * 0x100 + d
		end
		for i = 17, 64 do
			local x = w[i - 15]
			local y = w[i - 2]
			local s0 = bit32.bxor(bit32.rrotate(x, 7), bit32.rrotate(x, 18), bit32.rshift(x, 3))
			local s1 = bit32.bxor(bit32.rrotate(y, 17), bit32.rrotate(y, 19), bit32.rshift(y, 10))
			w[i] = (w[i - 16] + s0 + w[i - 7] + s1) % 0x100000000
		end

		local a, b, c, d, e, f, g, hh = h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8]
		for i = 1, 64 do
			local s1 = bit32.bxor(bit32.rrotate(e, 6), bit32.rrotate(e, 11), bit32.rrotate(e, 25))
			local ch = bit32.bxor(bit32.band(e, f), bit32.band(bit32.bnot(e), g))
			local t1 = (hh + s1 + ch + K[i] + w[i]) % 0x100000000
			local s0 = bit32.bxor(bit32.rrotate(a, 2), bit32.rrotate(a, 13), bit32.rrotate(a, 22))
			local maj = bit32.bxor(bit32.band(a, b), bit32.band(a, c), bit32.band(b, c))
			local t2 = (s0 + maj) % 0x100000000
			hh = g
			g = f
			f = e
			e = (d + t1) % 0x100000000
			d = c
			c = b
			b = a
			a = (t1 + t2) % 0x100000000
		end
		h[1] = (h[1] + a) % 0x100000000
		h[2] = (h[2] + b) % 0x100000000
		h[3] = (h[3] + c) % 0x100000000
		h[4] = (h[4] + d) % 0x100000000
		h[5] = (h[5] + e) % 0x100000000
		h[6] = (h[6] + f) % 0x100000000
		h[7] = (h[7] + g) % 0x100000000
		h[8] = (h[8] + hh) % 0x100000000
	end
	return h
end

function Sha256.hex(msg: string): string
	local h = digestWords(msg)
	local parts: {string} = {}
	for i = 1, 8 do
		parts[i] = string.format("%08x", h[i])
	end
	return table.concat(parts)
end

function Sha256.raw(msg: string): string
	local h = digestWords(msg)
	local parts: {string} = {}
	for i = 1, 8 do
		local v = h[i]
		parts[i] = string.char(
			bit32.rshift(v, 24), bit32.rshift(v, 16) % 256, bit32.rshift(v, 8) % 256, v % 256
		)
	end
	return table.concat(parts)
end

return Sha256
