--!strict
-- Key derivation and request signing for the presence relay protocol.

local Sha256 = require(script.Parent.Sha256)
local Hmac = require(script.Parent.Hmac)

local Auth = {}

local function int(n: number): string
	return string.format("%.0f", n)
end

function Auth.deriveKey(secret: string, placeId: number): string
	return Hmac.sha256Hex(secret, "rbx-presence/key/v1/" .. int(placeId))
end

function Auth.roomId(key: string): string
	return string.sub(Sha256.hex("rbx-presence/room/v1/" .. key), 1, 32)
end

function Auth.canonical(method: string, path: string, userId: number, ts: number, body: string): string
	return method .. "\n" .. path .. "\n" .. int(userId) .. "\n" .. int(ts) .. "\n" .. Sha256.hex(body)
end

function Auth.sign(key: string, canonical: string): string
	return Hmac.sha256Hex(key, canonical)
end

function Auth.headers(
	key: string,
	method: string,
	path: string,
	userId: number,
	ts: number,
	body: string,
	withKey: boolean
): { [string]: string }
	local headers: { [string]: string } = {
		["X-Presence-User"] = int(userId),
		["X-Presence-Time"] = int(ts),
		["X-Presence-Sig"] = Auth.sign(key, Auth.canonical(method, path, userId, ts, body)),
	}
	if withKey then
		headers["X-Presence-Key"] = key
	end
	return headers
end

return Auth
