--!strict
-- HMAC-SHA256 (RFC 2104) with the 64-byte SHA-256 block size.

local Sha256 = require(script.Parent.Sha256)

local BLOCK = 64

local Hmac = {}

function Hmac.sha256Hex(key: string, msg: string): string
	if #key > BLOCK then
		key = Sha256.raw(key)
	end
	key = key .. string.rep("\0", BLOCK - #key)
	local inner: {string} = {}
	local outer: {string} = {}
	for i = 1, BLOCK do
		local b = string.byte(key, i)
		inner[i] = string.char(bit32.bxor(b, 0x36))
		outer[i] = string.char(bit32.bxor(b, 0x5c))
	end
	local innerHash = Sha256.raw(table.concat(inner) .. msg)
	return Sha256.hex(table.concat(outer) .. innerHash)
end

return Hmac
