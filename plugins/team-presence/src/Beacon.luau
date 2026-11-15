--!strict
--! Which presence key each editor with this place open uses, mirrored from
--! crates/rbx_presence/src/beacon.rs. The key lives in the place, so two
--! editors can end up with different ones (each made its own because the
--! place had none saved yet, or one opened an older copy) and would wait for
--! each other forever. Each editor posts a short fingerprint of its room to
--! a topic named after the place; seeing another fingerprint there is how
--! that gets said out loud. Pure (no HttpService): the Luau tests check it
--! against the Rust encoder.

local Json = require(script.Parent.Json)
local Sha256 = require(script.Parent.Sha256)

local Beacon = {}

-- How long a beacon counts; each editor posts again well within it.
Beacon.FRESH_MS = 25 * 60 * 1000
Beacon.CLOCK_SKEW_MS = 60000
-- Seconds between this editor's own beacons.
Beacon.EVERY = 10 * 60

function Beacon.topic(placeId: number): string
	return "rbxnp-place-" .. string.sub(Sha256.hex(string.format("rbx-presence/place/v1/%.0f", placeId)), 1, 32)
end

-- Says which key without saying which room, so the place's topic does not
-- point at the room's.
function Beacon.fingerprint(room: string): string
	return string.sub(Sha256.hex("rbx-presence/fingerprint/v1/" .. room), 1, 16)
end

-- `fp` empty says the sender left.
function Beacon.encode(fp: string, from: string, client: string, at: number): string
	return Json.encode({ v = 1, fp = fp, from = from, client = client, at = at })
end

-- The client ("studio" or "rbx-native") of the newest current beacon from
-- another sender whose key differs from `fp`, or nil.
function Beacon.conflict(fp: string, from: string, now: number, texts: { string }): string?
	local newest: { [string]: any } = {}
	for _, text in texts do
		local ok, m = pcall(Json.decode, text)
		if
			ok
			and type(m) == "table"
			and m.v == 1
			and type(m.fp) == "string"
			and type(m.from) == "string"
			and type(m.client) == "string"
			and type(m.at) == "number"
			and m.from ~= from
		then
			local best = newest[m.from]
			if best == nil or m.at > best.at then
				newest[m.from] = m
			end
		end
	end
	local found: any = nil
	for _, m in newest do
		local current = now - m.at <= Beacon.FRESH_MS and m.at <= now + Beacon.CLOCK_SKEW_MS
		if current and m.fp ~= "" and m.fp ~= fp and (found == nil or m.at > found.at) then
			found = m
		end
	end
	return if found then found.client else nil
end

return Beacon
